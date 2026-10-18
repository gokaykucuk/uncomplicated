//! Existence checks should use ".includes()" instead of ".indexOf()" or ".lastIndexOf()"
use crate::helpers::batch_unicorn::{is_negative_one, simple_array_search};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression, AstType::BinaryExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::CallExpression(call) => {
                if let Some(span) = simple_array_search(ctx, call, "some") {
                    ctx.report(
                        span,
                        "Use `.includes()` instead of `.some()` when checking value existence.",
                    );
                }
            }
            AstKind::BinaryExpression(b) => {
                let Expression::CallExpression(call) = strip_parens(&b.left) else {
                    return;
                };
                let (object, prop, name) = match strip_parens(&call.callee) {
                    Expression::StaticMemberExpression(m) => {
                        (&m.object, m.property.span, m.property.name.as_str())
                    }
                    _ => return,
                };
                if !matches!(name, "indexOf" | "lastIndexOf") || call.arguments.len() > 2 {
                    return;
                }
                if matches!(object, Expression::Identifier(o) if matches!(o.name.as_str(), "_" | "lodash" | "underscore"))
                {
                    return;
                }
                use BinaryOperator::*;
                let ok = (matches!(
                    b.operator,
                    StrictInequality | Inequality | GreaterThan | StrictEquality | Equality
                ) && is_negative_one(&b.right))
                    || (matches!(b.operator, GreaterEqualThan | LessThan)
                        && matches!(strip_parens(&b.right), Expression::NumericLiteral(n) if n.value == 0.0));
                if ok {
                    ctx.report(prop, format!("Use `.includes()`, rather than `.{name}()`, when checking for existence."));
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
    fn includes() {
        check(
            "S7765",
            "a.js",
            r#"
if (str.indexOf('foo') !== -1) {} // Noncompliant {{Use `.includes()`, rather than `.indexOf()`, when checking for existence.}}
if (arr.lastIndexOf(x) >= 0) {} // Noncompliant {{Use `.includes()`, rather than `.lastIndexOf()`, when checking for existence.}}
if (arr.indexOf(x) < 0) {} // Noncompliant
const a = arr.some(x => x === 'foo'); // Noncompliant {{Use `.includes()` instead of `.some()` when checking value existence.}}
if (str.indexOf('foo') > 0) {}
if (_.indexOf(arr, x) !== -1) {}
if (str.indexOf('foo') === 2) {}
const b = arr.some(x => x.id === 'foo');
"#,
        );
    }
}
