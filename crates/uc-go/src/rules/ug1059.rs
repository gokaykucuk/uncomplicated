//! Loop variables should not be captured by goroutines and deferred closures before Go 1.22
//! (go vet loopclosure).
//!
//! Before Go 1.22 a `for` loop has one variable for all iterations, so a closure that outlives the
//! iteration sees the last value. Reported, only when the module's `go.mod` declares a version
//! below 1.22: a closure run by `go` (or `errgroup`'s `g.Go`) as the last statement of the loop
//! body, by `defer`, or appended to a slice, that reads a loop variable.
use crate::helpers_extra::*;
use crate::prelude::*;
use crate::scope;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Loop variables declared by the loop header.
fn loop_vars<'t>(ctx: &Ctx<'t>, for_stmt: Nd<'t>) -> Vec<Nd<'t>> {
    let mut out = Vec::new();
    for h in ctx.named_children(for_stmt) {
        match h.kind() {
            "for_clause" => {
                if let Some(init) = h.child_by_field_name("initializer")
                    && init.kind() == "short_var_declaration"
                    && let Some(l) = init.child_by_field_name("left")
                {
                    out.extend(expr_list(l).into_iter().filter(|i| i.kind() == "identifier"));
                }
            }
            "range_clause" => {
                let define = ctx.children(h).iter().any(|c| c.kind() == ":=");
                if define && let Some(l) = h.child_by_field_name("left") {
                    out.extend(expr_list(l).into_iter().filter(|i| i.kind() == "identifier"));
                }
            }
            _ => {}
        }
    }
    out.retain(|i| ctx.text(*i) != "_");
    out
}

/// The function literal called by `go f()` / `defer f()` / `g.Go(f)`.
fn literal_of<'t>(call: Nd<'t>) -> Option<Nd<'t>> {
    let f = strip_parens(call.child_by_field_name("function")?);
    (f.kind() == "func_literal").then_some(f)
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["for_statement"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        if !ctx.file.go_version.is_some_and(|v| v < (1, 22)) {
            return;
        }
        let vars = loop_vars(ctx, node);
        let Some(body) = node.child_by_field_name("body") else { return };
        if vars.is_empty() {
            return;
        }
        let home = ctx.enclosing_function(node);
        let same_func = |n: Nd<'_>| ctx.enclosing_function(n).map(|f| f.id()) == home.map(|f| f.id());
        let last = last_statement(body);
        let mut closures: Vec<(Nd<'_>, &str)> = Vec::new();
        for n in descendants(ctx, body) {
            match n.kind() {
                "go_statement" if last.is_some_and(|l| l.id() == n.id()) => {
                    if let Some(lit) = n.named_child(0).and_then(literal_of) {
                        closures.push((lit, "goroutine"));
                    }
                }
                "expression_statement" if last.is_some_and(|l| l.id() == n.id()) => {
                    if let Some(call) = n.named_child(0)
                        && call.kind() == "call_expression"
                        && method_name(ctx, call) == Some("Go")
                        && let [arg] = call_args(call)[..]
                        && strip_parens(arg).kind() == "func_literal"
                    {
                        closures.push((strip_parens(arg), "goroutine"));
                    }
                }
                "defer_statement" if same_func(*n) => {
                    if let Some(lit) = n.named_child(0).and_then(literal_of) {
                        closures.push((lit, "deferred function"));
                    }
                }
                "call_expression" if same_func(*n) => {
                    if n.child_by_field_name("function")
                        .is_some_and(|f| f.kind() == "identifier" && ctx.text(f) == "append")
                    {
                        for a in call_args(*n).into_iter().skip(1) {
                            if strip_parens(a).kind() == "func_literal" {
                                closures.push((strip_parens(a), "stored function"));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if closures.is_empty() {
            return;
        }
        let Some(func) = ctx.ancestors(node).filter(|a| is_function(*a)).last() else { return };
        let scopes = scope::function_scopes(ctx, func);
        for (lit, what) in closures {
            for v in &vars {
                let Some(i) = scopes.decls.iter().position(|d| d.ident.id() == v.id()) else { continue };
                if let Some(r) = scopes.refs[i].iter().find(|r| contains(lit, **r)) {
                    ctx.report(
                        *r,
                        format!(
                            "Copy the loop variable \"{}\" before capturing it in this {what}: before Go 1.22 all iterations share it.",
                            ctx.text(*r)
                        ),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    const CODE: &str = r#"package p

func f(xs []int, g *errgroup.Group) {
	for _, x := range xs {
		go func() {
			use(x) // Noncompliant {{Copy the loop variable "x" before capturing it in this goroutine: before Go 1.22 all iterations share it.}}
		}()
	}
	for i := 0; i < 3; i++ {
		defer func() { use(i) }() // Noncompliant
	}
	var fns []func()
	for _, x := range xs {
		fns = append(fns, func() { use(x) }) // Noncompliant
	}
	for _, x := range xs {
		g.Go(func() error { return use(x) }) // Noncompliant
	}
	for _, x := range xs {
		x := x
		go func() { use(x) }()
	}
	for _, x := range xs {
		go func(x int) { use(x) }(x)
	}
	for _, x := range xs {
		go func() { use(x) }()
		wg.Wait()
	}
	for _, x := range xs {
		func() { use(x) }()
	}
}
"#;

    #[test]
    fn old_go() {
        check_version("UG1059", (1, 21), CODE);
    }

    #[test]
    fn new_or_unknown_go() {
        let silent = CODE.lines().map(|l| l.split(" // Noncompliant").next().unwrap()).collect::<Vec<_>>().join("\n");
        check_version("UG1059", (1, 22), &silent);
        check("UG1059", &silent);
    }
}
