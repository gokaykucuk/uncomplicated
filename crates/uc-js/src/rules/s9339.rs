//! Native APIs should be preferred over Axios utility methods
use crate::helpers::batch_smell4::fqn;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const CANCEL_TOKEN: &str = "CancelToken is a deprecated Axios API; AbortController is the standard cancellation mechanism.";

fn has_computed_member(e: &Expression<'_>) -> bool {
    match e {
        Expression::ComputedMemberExpression(_) => true,
        Expression::StaticMemberExpression(m) => has_computed_member(&m.object),
        Expression::ChainExpression(ch) => match &ch.expression {
            ChainElement::ComputedMemberExpression(_) => true,
            ChainElement::StaticMemberExpression(m) => has_computed_member(&m.object),
            _ => false,
        },
        _ => false,
    }
}

/// Positions of the request config argument of axios request functions.
fn config_indices(fqn: &str) -> &'static [usize] {
    match fqn {
        "axios" => &[0, 1],
        "axios.request" => &[0],
        "axios.get" | "axios.delete" | "axios.head" | "axios.options" => &[1],
        "axios.post" | "axios.put" | "axios.patch" => &[2],
        _ => &[],
    }
}

fn property_or_self(e: &Expression<'_>) -> Span {
    match e {
        Expression::StaticMemberExpression(m) => m.property.span,
        e => e.span(),
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::CallExpression,
            AstType::NewExpression,
            AstType::StaticMemberExpression,
            AstType::ObjectProperty,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::CallExpression(call) => {
                if has_computed_member(&call.callee)
                    || matches!(call.callee, Expression::CallExpression(_))
                {
                    return;
                }
                match fqn(ctx, &call.callee).as_deref() {
                    Some("axios.all") => ctx.report(
                        property_or_self(&call.callee),
                        "axios.all() is a deprecated Axios helper; Promise.all() is the native equivalent.",
                    ),
                    Some("axios.spread") => ctx.report(
                        property_or_self(&call.callee),
                        "axios.spread() is a deprecated Axios helper; array destructuring is the native equivalent.",
                    ),
                    Some("axios.CancelToken.source") => {
                        if let Expression::StaticMemberExpression(m) = &call.callee {
                            ctx.report(property_or_self(&m.object), CANCEL_TOKEN);
                        }
                    }
                    _ => {}
                }
            }
            AstKind::NewExpression(new) => {
                if !has_computed_member(&new.callee)
                    && fqn(ctx, &new.callee).as_deref() == Some("axios.CancelToken")
                {
                    ctx.report(property_or_self(&new.callee), CANCEL_TOKEN);
                }
            }
            AstKind::StaticMemberExpression(m) => {
                if m.property.name != "CancelToken" || has_computed_member(&m.object) {
                    return;
                }
                let Some(name) = fqn(ctx, &m.object) else {
                    return;
                };
                if name != "axios" {
                    return;
                }
                // Covered by the `new` / `.source()` checks.
                let parent = ctx.parent(node.id());
                match parent.kind() {
                    AstKind::NewExpression(n) if n.callee.span() == m.span => return,
                    AstKind::StaticMemberExpression(p)
                        if p.property.name == "source" && p.object.span() == m.span =>
                    {
                        if matches!(ctx.parent_kind(parent.id()), AstKind::CallExpression(c) if c.callee.span() == p.span)
                        {
                            return;
                        }
                    }
                    _ => {}
                }
                ctx.report(m.property.span, CANCEL_TOKEN);
            }
            AstKind::ObjectProperty(p) => {
                let is_key = match &p.key {
                    PropertyKey::StaticIdentifier(k) => !p.computed && k.name == "cancelToken",
                    PropertyKey::StringLiteral(s) => s.value == "cancelToken",
                    _ => false,
                };
                if !is_key {
                    return;
                }
                let object = ctx.parent(node.id());
                let AstKind::CallExpression(call) = ctx.parent_kind(object.id()) else {
                    return;
                };
                let Some(index) = call
                    .arguments
                    .iter()
                    .position(|a| a.span() == object.kind().span())
                else {
                    return;
                };
                if let Some(name) = fqn(ctx, &call.callee)
                    && config_indices(&name).contains(&index)
                {
                    ctx.report(p.key.span(), CANCEL_TOKEN);
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
    fn rule() {
        check(
            "S9339",
            "a.js",
            r#"
import axios, { CancelToken } from 'axios';
axios.all([a, b]); // Noncompliant {{axios.all() is a deprecated Axios helper; Promise.all() is the native equivalent.}}
axios.spread((x, y) => x + y); // Noncompliant {{axios.spread() is a deprecated Axios helper; array destructuring is the native equivalent.}}
const source = axios.CancelToken.source(); // Noncompliant {{CancelToken is a deprecated Axios API; AbortController is the standard cancellation mechanism.}}
const token = new axios.CancelToken(c => c); // Noncompliant
const t2 = new CancelToken(c => c); // Noncompliant
const CT = axios.CancelToken; // Noncompliant
axios.get('/u', { cancelToken: source.token }); // Noncompliant
axios({ cancelToken: source.token }); // Noncompliant
axios.get({ cancelToken: source.token });
Promise.all([a, b]);
other.all([a, b]);
axios.get('/u', { signal: controller.signal });
"#,
        );
    }
}
