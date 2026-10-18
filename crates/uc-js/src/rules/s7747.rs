//! Spread syntax should not be used unnecessarily
use crate::helpers::batch_unicorn::{TYPED_ARRAYS, is_method_call, is_static_call, paren_parent};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const COLLECTIONS: &[&str] = &["Map", "WeakMap", "Set", "WeakSet"];

fn new_callee_name<'a>(n: &NewExpression<'a>) -> Option<&'a str> {
    match &n.callee {
        Expression::Identifier(id) => Some(id.name.as_str()),
        _ => None,
    }
}

/// `[...x]`: returns the spread argument.
fn single_spread<'b, 'a>(arr: &'b ArrayExpression<'a>) -> Option<&'b Expression<'a>> {
    match arr.elements.as_slice() {
        [ArrayExpressionElement::SpreadElement(s)] => Some(&s.argument),
        _ => None,
    }
}

fn first_arg_is(args: &[Argument<'_>], span: Span) -> bool {
    args.first()
        .and_then(|a| a.as_expression())
        .is_some_and(|e| strip_parens(e).span() == span)
}

impl Rule {
    /// Array/object literal spread into a list.
    fn spread_in_list(&self, ctx: &Ctx<'_, '_>, node: &AstNode<'_>, is_array: bool) {
        let spread = paren_parent(ctx, node.id());
        let AstKind::SpreadElement(s) = spread.kind() else {
            return;
        };
        let list = ctx.parent(spread.id());
        let desc = match (list.kind(), is_array) {
            (AstKind::ObjectExpression(_), false) => "object literal",
            (AstKind::ArrayExpression(_), true) => "array literal",
            (AstKind::CallExpression(_) | AstKind::NewExpression(_), true) => "arguments",
            _ => return,
        };
        let ty = if is_array { "array" } else { "object" };
        ctx.report(
            Span::new(s.span.start, s.span.start + 3),
            format!("Spread an {ty} literal in {desc} is unnecessary."),
        );
    }

    fn iterable_to_array(&self, ctx: &Ctx<'_, '_>, node: &AstNode<'_>, arr: &ArrayExpression<'_>) {
        let parent = paren_parent(ctx, node.id());
        let msg = match parent.kind() {
            AstKind::ForOfStatement(f) if strip_parens(&f.right).span() == arr.span => {
                "`for…of` can iterate over iterable, it's unnecessary to convert to an array."
                    .to_string()
            }
            AstKind::YieldExpression(y) if y.delegate => {
                "`yield*` can delegate iterable, it's unnecessary to convert to an array."
                    .to_string()
            }
            AstKind::NewExpression(n) if first_arg_is(&n.arguments, arr.span) => {
                let Some(name) = new_callee_name(n) else {
                    return;
                };
                let ok = (COLLECTIONS.contains(&name) && n.arguments.len() == 1)
                    || (TYPED_ARRAYS.contains(&name) && !n.arguments.is_empty());
                if !ok {
                    return;
                }
                format!(
                    "`new {name}(…)` accepts iterable as argument, it's unnecessary to convert to an array."
                )
            }
            AstKind::CallExpression(c) if first_arg_is(&c.arguments, arr.span) => {
                let ok = is_static_call(
                    c,
                    &["Promise"],
                    &["all", "allSettled", "any", "race"],
                    Some(1),
                ) || (is_static_call(c, &["Array"], &["from"], Some(1))
                    || is_static_call(c, TYPED_ARRAYS, &["from"], Some(1)))
                    || is_static_call(c, &["Object"], &["fromEntries"], Some(1));
                if !ok {
                    return;
                }
                let Some((m, name)) = crate::helpers::batch_unicorn::method_call(c) else {
                    return;
                };
                let Expression::Identifier(obj) = &m.object else {
                    return;
                };
                format!(
                    "`{}.{name}(…)` accepts iterable as argument, it's unnecessary to convert to an array.",
                    obj.name
                )
            }
            _ => return,
        };
        ctx.report(arr.span, msg);
    }

    fn clone_array(&self, ctx: &Ctx<'_, '_>, arr: &ArrayExpression<'_>, inner: &Expression<'_>) {
        let is_clone = match strip_parens(inner) {
            Expression::CallExpression(c) => {
                let receiver_is_string = || {
                    crate::helpers::batch_unicorn::method_call(c)
                        .is_some_and(|(m, _)| type_of(ctx, &m.object).is_string())
                };
                (is_method_call(
                    c,
                    &[
                        "copyWithin",
                        "flat",
                        "slice",
                        "splice",
                        "toReversed",
                        "toSorted",
                        "toSpliced",
                        "with",
                    ],
                    None,
                    false,
                )
                .is_some()
                    && !receiver_is_string())
                    || is_method_call(c, &["concat"], None, false).is_some_and(|m| {
                        !matches!(&m.object, Expression::Identifier(o) if o.name == "Iterator")
                            && !receiver_is_string()
                    })
                    || is_method_call(c, &["split"], None, false).is_some()
                    || is_static_call(c, &["Object"], &["keys", "values"], Some(1))
                    || is_static_call(c, &["Array"], &["from", "of"], None)
            }
            Expression::AwaitExpression(a) => {
                matches!(strip_parens(&a.argument), Expression::CallExpression(c)
                if is_static_call(c, &["Promise"], &["all", "allSettled"], Some(1)))
            }
            Expression::NewExpression(n) => new_callee_name(n) == Some("Array"),
            _ => false,
        };
        if is_clone {
            ctx.report(arr.span, "Unnecessarily cloning an array.");
        }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::ArrayExpression,
            AstType::ObjectExpression,
            AstType::NewExpression,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::ObjectExpression(_) => self.spread_in_list(ctx, node, false),
            AstKind::ArrayExpression(arr) => {
                self.spread_in_list(ctx, node, true);
                if let Some(inner) = single_spread(arr) {
                    self.iterable_to_array(ctx, node, arr);
                    self.clone_array(ctx, arr, inner);
                }
            }
            AstKind::NewExpression(n) => {
                let Some(name) = new_callee_name(n) else {
                    return;
                };
                if !COLLECTIONS.contains(&name) || n.arguments.len() != 1 {
                    return;
                }
                if let Argument::SpreadElement(s) = &n.arguments[0]
                    && !matches!(strip_parens(&s.argument), Expression::ArrayExpression(_))
                {
                    ctx.report(s.span, format!("`new {name}(…)` accepts a single iterable argument, spreading is misleading."));
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn useless_spread() {
        check(
            "S7747",
            "a.js",
            r#"
const a = [...[1, 2], 3]; // Noncompliant {{Spread an array literal in array literal is unnecessary.}}
const b = { ...{ x: 1 } }; // Noncompliant {{Spread an object literal in object literal is unnecessary.}}
foo(...[1, 2]); // Noncompliant {{Spread an array literal in arguments is unnecessary.}}
const s = new Set([...iterable]); // Noncompliant {{`new Set(…)` accepts iterable as argument, it's unnecessary to convert to an array.}}
const p = Promise.all([...promises]); // Noncompliant {{`Promise.all(…)` accepts iterable as argument, it's unnecessary to convert to an array.}}
for (const x of [...set]) {} // Noncompliant {{`for…of` can iterate over iterable, it's unnecessary to convert to an array.}}
function* g() { yield* [...it]; } // Noncompliant
const m = new Map(...pairs); // Noncompliant {{`new Map(…)` accepts a single iterable argument, spreading is misleading.}}
const c = [...arr.slice()]; // Noncompliant {{Unnecessarily cloning an array.}}
const d = [...Object.keys(o)]; // Noncompliant
const e = [...text.split(',')]; // Noncompliant
const f = [...new Set(x)];
const h = [...arr];
const i = { ...obj };
foo(...args);
const str = 'abc';
const j = [...str.slice(1)];
const k = [...a, ...b];
const l = [...(cond ? [1] : [])];
"#,
        );
    }
}
