//! Predeclared identifiers should not be shadowed.
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn check(ctx: &Ctx<'_>, id: Nd<'_>) {
    let name = ctx.text(id);
    if id.child_count() == 0 && PREDECLARED.contains(&name) {
        ctx.report(
            id,
            format!("Rename this identifier, \"{name}\" shadows a predeclared identifier."),
        );
    }
}

fn check_names(ctx: &Ctx<'_>, n: Nd<'_>) {
    let mut c = n.walk();
    for id in n.children_by_field_name("name", &mut c) {
        check(ctx, id);
    }
}

/// Names declared by a parameter list (parameters, named results, receiver, type parameters).
fn check_fields(ctx: &Ctx<'_>, list: Option<Nd<'_>>) {
    let Some(list) = list else { return };
    if !matches!(list.kind(), "parameter_list" | "type_parameter_list") {
        return;
    }
    let mut c = list.walk();
    for p in list.named_children(&mut c) {
        if matches!(
            p.kind(),
            "parameter_declaration"
                | "variadic_parameter_declaration"
                | "type_parameter_declaration"
        ) {
            check_names(ctx, p);
        }
    }
}

fn field_names<'t>(list: Option<Nd<'t>>, out: &mut Vec<&'t str>, ctx: &Ctx<'t>) {
    let Some(list) = list else { return };
    if list.kind() != "parameter_list" {
        return;
    }
    let mut c = list.walk();
    for p in list.named_children(&mut c) {
        let mut c2 = p.walk();
        out.extend(
            p.children_by_field_name("name", &mut c2)
                .map(|n| ctx.text(n)),
        );
    }
}

/// The type arguments of a receiver type (`func (p *pair[K]) ...`) declare type parameters.
fn check_receiver_type_args(ctx: &Ctx<'_>, recv: Nd<'_>) {
    let mut c = recv.walk();
    for p in recv.named_children(&mut c) {
        let Some(mut t) = p.child_by_field_name("type") else {
            continue;
        };
        if t.kind() == "pointer_type"
            && let Some(inner) = t.named_child(0)
        {
            t = inner;
        }
        if t.kind() != "generic_type" {
            continue;
        }
        let Some(args) = t.child_by_field_name("type_arguments") else {
            continue;
        };
        let mut c2 = args.walk();
        for a in args.named_children(&mut c2) {
            let a = if a.kind() == "type_elem" && a.named_child_count() == 1 {
                a.named_child(0).unwrap()
            } else {
                a
            };
            if a.kind() == "type_identifier" {
                check(ctx, a);
            }
        }
    }
}

/// Whether `name` is already declared in the scope a `:=` statement belongs to (it is then
/// only assigned by the statement).
fn declared_before(ctx: &Ctx<'_>, stmt: Nd<'_>, name: &str) -> bool {
    let Some(list) = ctx.parent(stmt).filter(|p| p.kind() == "statement_list") else {
        return false;
    };
    let mut c = list.walk();
    for s in list.named_children(&mut c) {
        if s.start_byte() >= stmt.start_byte() {
            break;
        }
        let found = match s.kind() {
            "short_var_declaration" => s.child_by_field_name("left").is_some_and(|l| {
                let mut c2 = l.walk();
                l.named_children(&mut c2).any(|id| ctx.text(id) == name)
            }),
            "var_declaration" | "const_declaration" => {
                ctx.text(s).contains(name) && declares(ctx, s, name)
            }
            _ => false,
        };
        if found {
            return true;
        }
    }
    // The function body shares its scope with the parameters, results and receiver.
    if let Some(block) = ctx.parent(list)
        && block.kind() == "block"
        && let Some(func) = ctx.parent(block)
        && helpers::is_function(func)
    {
        let mut names = Vec::new();
        field_names(func.child_by_field_name("receiver"), &mut names, ctx);
        field_names(func.child_by_field_name("parameters"), &mut names, ctx);
        field_names(func.child_by_field_name("result"), &mut names, ctx);
        return names.contains(&name);
    }
    false
}

fn declares(ctx: &Ctx<'_>, decl: Nd<'_>, name: &str) -> bool {
    let mut stack = vec![decl];
    while let Some(n) = stack.pop() {
        if matches!(n.kind(), "var_spec" | "const_spec") {
            let mut c = n.walk();
            if n.children_by_field_name("name", &mut c)
                .any(|id| ctx.text(id) == name)
            {
                return true;
            }
        } else if matches!(
            n.kind(),
            "var_declaration" | "const_declaration" | "var_spec_list"
        ) {
            let mut c = n.walk();
            stack.extend(n.named_children(&mut c));
        }
    }
    false
}

fn has_define(n: Nd<'_>) -> bool {
    let mut c = n.walk();
    n.children(&mut c).any(|ch| ch.kind() == ":=")
}

fn check_list(ctx: &Ctx<'_>, list: Option<Nd<'_>>) {
    let Some(list) = list else { return };
    let mut c = list.walk();
    for id in list.named_children(&mut c) {
        check(ctx, id);
    }
}

impl GoRule for Rule {
    fn run_once(&self, ctx: &Ctx<'_>) {
        for &n in &ctx.nodes {
            match n.kind() {
                "import_spec" => {
                    if let Some(name) = n.child_by_field_name("name") {
                        check(ctx, name);
                    }
                }
                "var_spec" | "const_spec" => check_names(ctx, n),
                "type_spec" | "type_alias" => {
                    check_names(ctx, n);
                    check_fields(ctx, n.child_by_field_name("type_parameters"));
                }
                "function_declaration" | "method_declaration" | "func_literal" => {
                    if n.kind() == "function_declaration"
                        && let Some(name) = n.child_by_field_name("name")
                    {
                        check(ctx, name);
                    }
                    check_fields(ctx, n.child_by_field_name("parameters"));
                    check_fields(ctx, n.child_by_field_name("type_parameters"));
                    check_fields(ctx, n.child_by_field_name("result"));
                    if let Some(recv) = n.child_by_field_name("receiver") {
                        check_fields(ctx, Some(recv));
                        check_receiver_type_args(ctx, recv);
                    }
                }
                "short_var_declaration" => {
                    let Some(left) = n.child_by_field_name("left") else {
                        continue;
                    };
                    let mut c = left.walk();
                    for id in left.named_children(&mut c) {
                        let name = ctx.text(id);
                        if PREDECLARED.contains(&name) && !declared_before(ctx, n, name) {
                            check(ctx, id);
                        }
                    }
                }
                "range_clause" | "receive_statement" if has_define(n) => {
                    check_list(ctx, n.child_by_field_name("left"))
                }
                "type_switch_statement" => check_list(ctx, n.child_by_field_name("alias")),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn shadowing() {
        check(
            "S978",
            r#"package p

import max "strings" // Noncompliant {{Rename this identifier, "max" shadows a predeclared identifier.}}

const iota = 5 // Noncompliant
var print, ok = 1, 2 // Noncompliant
type len int // Noncompliant
type pair[cap any] struct{ v cap } // Noncompliant

func append() {} // Noncompliant
func (r *T) append() {}

func params(new int, close string, fine bool) (recover int, err error) { // Noncompliant 3
	panic := 1 // Noncompliant
	var false = 4 // Noncompliant
	for len, cap := range xs { // Noncompliant 2
	}
	var copy int // Noncompliant
	for copy = range xs {
	}
	new, x := 1, 2
	if make := 1; make > 0 { // Noncompliant
	}
	switch nil := v.(type) { // Noncompliant
	}
	select {
	case true := <-ch: // Noncompliant
	}
	min, e := f() // Noncompliant
	min, e2 := f()
	_ = func(imag int) {} // Noncompliant
}

func (g *generic[recover]) valueOf() recover { // Noncompliant
	return g.value
}
"#,
        );
    }
}
