//! Nested properties of Lodash and Underscore.js clone results should not be mutated
use crate::helpers::batch_smell4::{fqn, is_write_target, unique_init};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const LODASH_MESSAGE: &str = "Mutating a nested property of this shallow clone changes the original value; use structuredClone() or _.cloneDeep() when isolation is required.";
const UNDERSCORE_MESSAGE: &str = "Mutating a nested property of this shallow clone changes the original value; use structuredClone() when isolation is required.";

fn is_static_member(m: &MemberExpression<'_>) -> bool {
    match m {
        MemberExpression::StaticMemberExpression(_) => true,
        MemberExpression::ComputedMemberExpression(c) => {
            matches!(
                c.expression,
                Expression::StringLiteral(_) | Expression::NumericLiteral(_)
            )
        }
        MemberExpression::PrivateFieldExpression(_) => false,
    }
}

/// Root identifier of a static member chain of depth >= 2, with the spans of its strict prefixes.
fn member_chain<'a>(
    m: &'a MemberExpression<'a>,
) -> Option<(&'a IdentifierReference<'a>, Vec<Span>)> {
    if !is_static_member(m) {
        return None;
    }
    let mut prefixes = Vec::new();
    let mut cur = strip_ts(m.object());
    loop {
        match cur.as_member_expression() {
            Some(inner) if is_static_member(inner) => {
                prefixes.push(inner.span());
                cur = strip_ts(inner.object());
            }
            Some(_) => return None,
            None => break,
        }
    }
    match cur {
        Expression::Identifier(id) if !prefixes.is_empty() => Some((id, prefixes)),
        _ => None,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::AssignmentExpression,
            AstType::UpdateExpression,
            AstType::UnaryExpression,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let (target, span) = match node.kind() {
            AstKind::AssignmentExpression(a) => (a.left.as_member_expression(), a.span),
            AstKind::UpdateExpression(u) => (u.argument.as_member_expression(), u.span),
            AstKind::UnaryExpression(u) if u.operator == UnaryOperator::Delete => {
                (strip_ts(&u.argument).as_member_expression(), u.span)
            }
            _ => return,
        };
        let Some(target) = target else { return };
        let Some((root, prefixes)) = member_chain(target) else {
            return;
        };
        let Some(sym) = ctx.symbol_of(root) else {
            return;
        };
        let Some(Expression::CallExpression(clone)) = unique_init(ctx, sym).map(strip_parens)
        else {
            return;
        };
        if clone.arguments.len() != 1 || clone.arguments[0].is_spread() {
            return;
        }
        let message = match fqn(ctx, &clone.callee).as_deref() {
            Some("lodash.clone" | "lodash-es.clone") => LODASH_MESSAGE,
            Some("underscore.clone") => UNDERSCORE_MESSAGE,
            _ => return,
        };
        // A prefix reassigned before the mutation may hold a fresh object: stay silent.
        for r in ctx.scoping().get_resolved_references(sym) {
            let mut cur = ctx.node(r.node_id());
            if cur.kind().span().start >= span.start {
                continue;
            }
            loop {
                let parent = ctx.parent(cur.id());
                let is_member_of_cur = match parent.kind() {
                    AstKind::StaticMemberExpression(m) => m.object.span() == cur.kind().span(),
                    AstKind::ComputedMemberExpression(m) => m.object.span() == cur.kind().span(),
                    AstKind::TSNonNullExpression(_)
                    | AstKind::TSAsExpression(_)
                    | AstKind::ParenthesizedExpression(_) => true,
                    _ => false,
                };
                if !is_member_of_cur {
                    break;
                }
                let pspan = parent.kind().span();
                if is_write_target(ctx, parent)
                    && prefixes.iter().any(|p| ctx.same_tokens(*p, pspan))
                {
                    return;
                }
                cur = parent;
            }
        }
        ctx.report_with(
            span,
            message,
            [(clone.span, Some("Shallow clone created here.".to_string()))],
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S9135",
            "a.js",
            r#"
import _ from 'lodash';
import underscore from 'underscore';
const copy = _.clone(original);
copy.settings.theme = 'dark'; // Noncompliant {{Mutating a nested property of this shallow clone changes the original value; use structuredClone() or _.cloneDeep() when isolation is required.}}
copy.a.b++; // Noncompliant
delete copy.a.b; // Noncompliant
copy.top = 1;
const u = underscore.clone(original);
u.x.y = 1; // Noncompliant {{Mutating a nested property of this shallow clone changes the original value; use structuredClone() when isolation is required.}}
const deep = _.cloneDeep(original);
deep.a.b = 1;
const iso = _.clone(original);
iso.settings = { ...iso.settings };
iso.settings.theme = 'dark';
let re = _.clone(original);
re = other;
re.a.b = 1;
const plain = clone(original);
plain.a.b = 1;
"#,
        );
    }
}
