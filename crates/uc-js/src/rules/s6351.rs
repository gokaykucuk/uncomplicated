//! Regular expressions with the global flag should be used with caution
use crate::helpers::batch_regex::{const_init, regexp_ctor_args};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Flags of a regex node (literal or `RegExp` constructor with literal flags); `None` otherwise.
fn regex_flags<'a>(ctx: &Ctx<'a, '_>, e: &'a Expression<'a>) -> Option<&'a str> {
    match strip_parens(e) {
        Expression::RegExpLiteral(l) => Some(flags_text(ctx, l)),
        e => {
            let args = regexp_ctor_args(ctx, e)?;
            Some(
                match args
                    .get(1)
                    .and_then(Argument::as_expression)
                    .map(strip_parens)
                {
                    None => "",
                    Some(Expression::StringLiteral(s)) => s.value.as_str(),
                    Some(Expression::TemplateLiteral(t))
                        if t.expressions.is_empty() && t.quasis.len() == 1 =>
                    {
                        t.quasis[0].value.cooked.as_ref().map_or("", |c| c.as_str())
                    }
                    Some(_) => "",
                },
            )
        }
    }
}

fn flags_text<'a>(ctx: &Ctx<'a, '_>, l: &RegExpLiteral<'a>) -> &'a str {
    let raw = ctx.text(l.span);
    raw.rfind('/').map_or("", |i| &raw[i + 1..])
}

/// True if `id` is (inside) the test of a `while` / `do-while` loop, without crossing functions.
fn in_while_condition(ctx: &Ctx<'_, '_>, id: NodeId) -> bool {
    let mut child = ctx.node(id).kind().span();
    for parent in ctx.ancestors(id) {
        match parent.kind() {
            k if is_function(&k) => return false,
            AstKind::WhileStatement(w) => return w.test.span() == child,
            AstKind::DoWhileStatement(w) => return w.test.span() == child,
            k => child = k.span(),
        }
    }
    false
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let mut order: Vec<SymbolId> = Vec::new();
        let mut usages: FxHashMap<SymbolId, Vec<(Span, &'a str)>> = FxHashMap::default();
        let mut resets: FxHashSet<SymbolId> = FxHashSet::default();
        // Symbol of a variable initialized with a regex (and never reassigned), plus its flags.
        let regex_var = |e: &Expression<'a>| -> Option<(SymbolId, &'a str)> {
            let Expression::Identifier(id) = strip_parens(e) else {
                return None;
            };
            let init = const_init(ctx, id)?;
            Some((ctx.symbol_of(id)?, regex_flags(ctx, init)?))
        };
        for node in ctx.nodes().iter() {
            match node.kind() {
                AstKind::RegExpLiteral(l) => {
                    let f = flags_text(ctx, l);
                    if f.contains('g') && f.contains('y') {
                        ctx.report(l.span, "Remove the 'g' flag from this regex as it is shadowed by the 'y' flag.");
                    }
                }
                AstKind::NewExpression(n) => {
                    check_ctor_gy(ctx, n.span, &n.callee, &n.arguments, true)
                }
                AstKind::CallExpression(c) => {
                    check_ctor_gy(ctx, c.span, &c.callee, &c.arguments, false);
                    let Expression::StaticMemberExpression(m) = &c.callee else {
                        continue;
                    };
                    let method = m.property.name.as_str();
                    if method != "exec" && method != "test" {
                        continue;
                    }
                    // `while (m = /x/g.exec(s))` creates a new regex on each evaluation.
                    if method == "exec" {
                        let obj = strip_parens(&m.object);
                        if matches!(
                            obj,
                            Expression::RegExpLiteral(_)
                                | Expression::NewExpression(_)
                                | Expression::CallExpression(_)
                        ) && regex_flags(ctx, obj).is_some_and(|f| f.contains('g'))
                            && in_while_condition(ctx, node.id())
                        {
                            ctx.report(
                                obj.span(),
                                "Extract this regular expression to avoid infinite loop.",
                            );
                        }
                    }
                    if c.arguments.len() != 1 {
                        continue;
                    }
                    let Some(arg) = c.arguments[0].as_expression() else {
                        continue;
                    };
                    if let Some((sym, flags)) = regex_var(&m.object)
                        && flags.contains('g')
                    {
                        let list = usages.entry(sym).or_default();
                        if list.is_empty() {
                            order.push(sym);
                        }
                        list.push((c.span, ctx.text(arg.span())));
                    }
                }
                AstKind::AssignmentExpression(a) => {
                    if let AssignmentTarget::StaticMemberExpression(m) = &a.left
                        && m.property.name == "lastIndex"
                        && let Some((sym, _)) = regex_var(&m.object)
                    {
                        resets.insert(sym);
                    }
                }
                _ => {}
            }
        }
        for sym in order {
            if resets.contains(&sym) {
                continue;
            }
            let list = &usages[&sym];
            let mut inputs: Vec<&str> = list.iter().map(|(_, t)| *t).collect();
            inputs.sort_unstable();
            inputs.dedup();
            if inputs.len() < 2 || inputs.iter().any(|t| *t == "''" || *t == "\"\"") {
                continue;
            }
            let AstKind::VariableDeclarator(v) = ctx.symbol_decl(sym).kind() else {
                continue;
            };
            let Some(init) = &v.init else { continue };
            let secondary = list
                .iter()
                .enumerate()
                .map(|(i, (s, _))| (*s, Some(format!("Usage {}", i + 1))));
            ctx.report_with(
                init.span(),
                "Remove the 'g' flag from this regex as it is used on different inputs.",
                secondary,
            );
        }
    }
}

fn check_ctor_gy<'a>(
    ctx: &Ctx<'a, '_>,
    span: Span,
    callee: &Expression<'a>,
    args: &'a [Argument<'a>],
    is_new: bool,
) {
    if args.is_empty() || !crate::helpers::batch_regex::regexp_callee(ctx, callee, is_new) {
        return;
    }
    let flags = match args
        .get(1)
        .and_then(Argument::as_expression)
        .map(strip_parens)
    {
        Some(Expression::StringLiteral(s)) => s.value.as_str(),
        Some(Expression::TemplateLiteral(t)) if t.expressions.is_empty() && t.quasis.len() == 1 => {
            t.quasis[0].value.cooked.as_ref().map_or("", |c| c.as_str())
        }
        _ => return,
    };
    if flags.contains('g') && flags.contains('y') {
        ctx.report(
            span,
            "Remove the 'g' flag from this regex as it is shadowed by the 'y' flag.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn stateful_regex() {
        check(
            "S6351",
            "a.js",
            r#"
/a/gy;                              // Noncompliant {{Remove the 'g' flag from this regex as it is shadowed by the 'y' flag.}}
new RegExp("a", "gy");              // Noncompliant
const re = /foo/g;                  // Noncompliant {{Remove the 'g' flag from this regex as it is used on different inputs.}}
re.test("foo");
re.test("bar");
const re2 = /foo/g;
re2.exec(s);
re2.exec(s);
const re3 = /foo/g;
re3.test("a");
re3.lastIndex = 0;
re3.test("b");
const re4 = /foo/;
re4.test("a");
re4.test("b");
const re5 = /foo/g;
re5.test("a");
re5.test('');
while ((m = /x/g.exec(s))) {}       // Noncompliant {{Extract this regular expression to avoid infinite loop.}}
const ext = /x/g;
while ((m = ext.exec(s))) {}
while (c) { /x/g.exec(s); }
/a/y;
"#,
        );
    }
}
