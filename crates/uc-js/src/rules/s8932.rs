//! Lodash and Underscore.js `memoize` calls on functions with multiple parameters should use an explicit cache key function
use crate::helpers::batch_smell4::{fqn, is_null_or_undefined, require_module, unique_init};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn has_multiple_params(params: &FormalParameters<'_>) -> bool {
    params.items.len() >= 2 || params.rest.is_some()
}

/// Parameters of the memoized function, when it can be resolved locally.
fn memoized_params<'a>(
    ctx: &Ctx<'a, '_>,
    e: &'a Expression<'a>,
) -> Option<&'a FormalParameters<'a>> {
    match strip_parens(e) {
        Expression::ArrowFunctionExpression(a) => Some(&a.params),
        Expression::FunctionExpression(f) => Some(&f.params),
        Expression::Identifier(id) => {
            let sym = ctx.symbol_of(id)?;
            if let AstKind::Function(f) = ctx.symbol_decl(sym).kind() {
                return Some(&f.params);
            }
            match strip_parens(unique_init(ctx, sym)?) {
                Expression::ArrowFunctionExpression(a) => Some(&a.params),
                Expression::FunctionExpression(f) => Some(&f.params),
                _ => None,
            }
        }
        _ => None,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let (syntactic, report_span) = match &call.callee {
            Expression::StaticMemberExpression(m) => {
                (Some(m.property.name.as_str()), m.property.span)
            }
            Expression::ComputedMemberExpression(m) => match &m.expression {
                Expression::StringLiteral(s) => (Some(s.value.as_str()), s.span),
                _ => return,
            },
            Expression::Identifier(id) => (None, id.span),
            _ => return,
        };
        if let Some(name) = syntactic {
            if name != "memoize" {
                return;
            }
            let obj = call.callee.as_member_expression().map(|m| m.object());
            if obj.is_some_and(|o| {
                matches!(strip_parens(o), Expression::CallExpression(_))
                    && require_module(o).is_none()
            }) {
                return;
            }
        }
        // An explicit resolver (anything but a missing / nullish one).
        if let Some(resolver) = call.arguments.get(1) {
            match resolver.as_expression() {
                None => return,
                Some(e) if !is_null_or_undefined(e) => return,
                _ => {}
            }
        }
        let Some(name) = fqn(ctx, &call.callee) else {
            return;
        };
        let mut parts = name.split('.');
        let (Some(module), Some("memoize"), None) = (parts.next(), parts.next(), parts.next())
        else {
            return;
        };
        if !matches!(module, "lodash" | "lodash-es" | "underscore") {
            return;
        }
        let Some(func) = call.arguments.first().and_then(|a| a.as_expression()) else {
            return;
        };
        if memoized_params(ctx, func).is_some_and(has_multiple_params) {
            ctx.report(
                report_span,
                "Provide an explicit function to compute the cache key.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S8932",
            "a.ts",
            r#"
import _ from 'lodash';
import lodashEs from 'lodash-es';
import { memoize as memo } from 'lodash';
import memoize from 'lodash/memoize';
const underscore = require('underscore');
const resolver = (amount, locale) => `${amount}:${locale}`;
const formatAmount = (amount, locale) => amount.toLocaleString(locale);
function formatDate(date, locale) { return date.toLocaleDateString(locale); }
function withThis(this: Intl.NumberFormat, amount: number) { return this.format(amount); }
_.memoize((amount, locale) => amount.toLocaleString(locale)); // Noncompliant {{Provide an explicit function to compute the cache key.}}
_['memoize']((a, b) => a + b); // Noncompliant
lodashEs.memoize(function (a, b) { return a + b; }); // Noncompliant
memo((a, b) => a + b); // Noncompliant
memoize((a, b) => a + b); // Noncompliant
underscore.memoize((a, b) => a + b); // Noncompliant
_.memoize((a, b) => a + b, undefined); // Noncompliant
_.memoize(formatAmount); // Noncompliant
_.memoize(formatDate); // Noncompliant
_.memoize((...parts) => parts.join(':')); // Noncompliant
_.memoize(amount => amount.toLocaleString());
_.memoize(({ amount, locale }) => amount.toLocaleString(locale));
_.memoize((a, b) => a + b, resolver);
_.memoize(unresolved);
_.memoize(withThis);
other.memoize((a, b) => a + b);
"#,
        );
    }
}
