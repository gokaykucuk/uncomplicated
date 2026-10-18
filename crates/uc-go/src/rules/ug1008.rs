//! SQL queries should not be built by string formatting or concatenation (gosec G201/G202).
//!
//! The query argument of `Query`/`Exec`/`QueryRow`/`Prepare` (and their `Context` and sqlx
//! variants) is flagged when it is, or is a local variable initialized with, a SQL literal
//! concatenated with dynamic values or a `fmt.Sprintf` of a SQL format with `%s`/`%v`/`%q`.
use regex::Regex;

use crate::helpers_extra::*;
use crate::prelude::*;
use crate::scope;

pub struct Rule {
    sql: Regex,
    str_verb: Regex,
}

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self {
            sql: Regex::new(
                r"(?i)^\s*(select|insert|update|delete|with|replace|merge|upsert|truncate)\s",
            )
            .unwrap(),
            str_verb: Regex::new(r"%[-+# 0]*[0-9]*(\.[0-9]*)?[svq]").unwrap(),
        }
    }
}

fn query_index(method: &str) -> Option<usize> {
    Some(match method {
        "Query" | "QueryRow" | "Exec" | "Prepare" | "Queryx" | "QueryRowx" | "MustExec" => 0,
        "QueryContext" | "QueryRowContext" | "ExecContext" | "PrepareContext" | "QueryxContext"
        | "QueryRowxContext" | "MustExecContext" | "Select" | "Get" => 1,
        "SelectContext" | "GetContext" => 2,
        _ => return None,
    })
}

fn concat_leaves<'t>(ctx: &Ctx<'t>, n: Nd<'t>, out: &mut Vec<Nd<'t>>) {
    let n = strip_parens(n);
    if n.kind() == "binary_expression" && binary_op(ctx, n) == Some("+") {
        if let (Some(l), Some(r)) = (
            n.child_by_field_name("left"),
            n.child_by_field_name("right"),
        ) {
            concat_leaves(ctx, l, out);
            concat_leaves(ctx, r, out);
        }
    } else {
        out.push(n);
    }
}

impl Rule {
    /// A value that can't carry injected SQL: literals, file constants, numeric formatting, quoting.
    fn safe_leaf(&self, ctx: &Ctx<'_>, n: Nd<'_>, consts: &[&str]) -> bool {
        match n.kind() {
            "interpreted_string_literal"
            | "raw_string_literal"
            | "int_literal"
            | "float_literal" => true,
            "identifier" => consts.contains(&ctx.text(n)),
            "call_expression" => {
                if pkg_call(ctx, n, &["strconv"], &[]).is_some() {
                    return true;
                }
                let f = n
                    .child_by_field_name("function")
                    .map(|f| ctx.text(f).to_ascii_lowercase())
                    .unwrap_or_default();
                f.contains("quote") || f.contains("escape") || f.contains("placeholder")
            }
            _ => false,
        }
    }

    fn sql_literal(&self, ctx: &Ctx<'_>, n: Nd<'_>) -> bool {
        str_lit(ctx, n).is_some_and(|s| self.sql.is_match(s))
    }

    fn dangerous(&self, ctx: &Ctx<'_>, n: Nd<'_>, consts: &[&str]) -> bool {
        let n = strip_parens(n);
        match n.kind() {
            "binary_expression" => {
                let mut leaves = Vec::new();
                concat_leaves(ctx, n, &mut leaves);
                leaves.len() > 1
                    && self.sql_literal(ctx, leaves[0])
                    && leaves.iter().any(|l| !self.safe_leaf(ctx, *l, consts))
            }
            "call_expression" => {
                if pkg_call(ctx, n, &["fmt"], &["Sprintf"]).is_none() {
                    return false;
                }
                let args = call_args(n);
                let Some(fmt) = args.first().and_then(|a| str_lit(ctx, *a)) else {
                    return false;
                };
                self.sql.is_match(fmt)
                    && self.str_verb.is_match(fmt)
                    && args[1..]
                        .iter()
                        .any(|a| !self.safe_leaf(ctx, strip_parens(*a), consts))
            }
            _ => false,
        }
    }

    /// The local variable `ident` is initialized with a dangerous value or a SQL literal that is
    /// later extended with `+=` of a dynamic value.
    fn dangerous_var(&self, ctx: &Ctx<'_>, ident: Nd<'_>, consts: &[&str]) -> bool {
        let Some(func) = ctx.enclosing_function(ident) else {
            return false;
        };
        let scopes = scope::function_scopes(ctx, func);
        let Some(i) = scopes
            .refs
            .iter()
            .position(|r| r.iter().any(|x| x.id() == ident.id()))
        else {
            return false;
        };
        let d = scopes.decls[i];
        let values = match d.decl.kind() {
            "short_var_declaration" => d.decl.child_by_field_name("right").map(expr_list),
            "var_spec" => d.decl.child_by_field_name("value").map(expr_list),
            _ => None,
        }
        .unwrap_or_default();
        let names: Vec<Nd<'_>> = match d.decl.kind() {
            "short_var_declaration" => d
                .decl
                .child_by_field_name("left")
                .map(expr_list)
                .unwrap_or_default(),
            _ => {
                let mut c = d.decl.walk();
                d.decl.children_by_field_name("name", &mut c).collect()
            }
        };
        let Some(pos) = names.iter().position(|n| n.id() == d.ident.id()) else {
            return false;
        };
        let init = (names.len() == values.len()).then(|| values[pos]);
        if init.is_some_and(|v| self.dangerous(ctx, v, consts)) {
            return true;
        }
        let sql_init = init.is_some_and(|v| {
            let mut leaves = Vec::new();
            concat_leaves(ctx, v, &mut leaves);
            self.sql_literal(ctx, leaves[0])
        });
        sql_init
            && scopes.writes[i].iter().any(|w| {
                ctx.parent(*w).and_then(|l| ctx.parent(l)).is_some_and(|a| {
                    a.kind() == "assignment_statement"
                        && a.child_by_field_name("operator")
                            .is_some_and(|o| ctx.text(o) == "+=")
                        && a.child_by_field_name("right").is_some_and(|r| {
                            let mut leaves = Vec::new();
                            for e in expr_list(r) {
                                concat_leaves(ctx, e, &mut leaves);
                            }
                            leaves.iter().any(|l| !self.safe_leaf(ctx, *l, consts))
                        })
                })
            })
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["call_expression"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let Some(idx) = method_name(ctx, node).and_then(query_index) else {
            return;
        };
        let args = call_args(node);
        let Some(&q) = args.get(idx) else { return };
        let q = strip_parens(q);
        let bad = match q.kind() {
            "binary_expression" | "call_expression" => self.dangerous(ctx, q, &file_consts(ctx)),
            "identifier" => self.dangerous_var(ctx, q, &file_consts(ctx)),
            _ => false,
        };
        if bad {
            ctx.report(q, "Use a parameterized query instead of building this SQL query from dynamic strings.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1008",
            r#"package p

import (
	"context"
	"database/sql"
	"fmt"
	"strconv"
)

const table = "users"

func f(ctx context.Context, db *sql.DB, name string, id int) {
	db.Query("SELECT * FROM users WHERE name = '" + name + "'")                // Noncompliant {{Use a parameterized query instead of building this SQL query from dynamic strings.}}
	db.QueryRowContext(ctx, fmt.Sprintf("SELECT * FROM t WHERE n = '%s'", name)) // Noncompliant
	q := "DELETE FROM users WHERE name = '" + name + "'"
	db.Exec(q) // Noncompliant
	q2 := "SELECT * FROM users WHERE 1=1"
	q2 += " AND name = '" + name + "'"
	db.Query(q2) // Noncompliant

	db.Query("SELECT * FROM users WHERE name = ?", name)
	db.Query("SELECT * FROM " + table + " WHERE id = ?", id)
	db.Query("SELECT * FROM users WHERE id = " + strconv.Itoa(id))
	db.Query(fmt.Sprintf("SELECT * FROM users LIMIT %d", id))
	q3 := "SELECT * FROM users"
	q3 += " WHERE id = ?"
	db.Query(q3, id)
	cache.Get("key" + name)
}
"#,
        );
    }
}
