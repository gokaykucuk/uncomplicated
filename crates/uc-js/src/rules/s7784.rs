//! "structuredClone()" should be used instead of other deep cloning methods
use crate::helpers::batch_smell4::is_global_member;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// `JSON.<method>(x)` with exactly one plain argument.
fn json_call<'b, 'a>(
    ctx: &Ctx<'a, '_>,
    e: &'b Expression<'a>,
    method: &str,
) -> Option<&'b CallExpression<'a>> {
    let Expression::CallExpression(c) = strip_parens(e) else {
        return None;
    };
    let Expression::StaticMemberExpression(m) = &c.callee else {
        return None;
    };
    (!c.optional
        && !m.optional
        && c.arguments.len() == 1
        && !c.arguments[0].is_spread()
        && is_global_member(ctx, &c.callee, "JSON", method))
    .then_some(c)
}

/// The clone result is serialized again: normalization, not a deep clone.
fn is_inside_json_stringify(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> bool {
    let mut child_span = node.kind().span();
    let mut cur = ctx.parent(node.id());
    loop {
        match cur.kind() {
            AstKind::ObjectExpression(_)
            | AstKind::ArrayExpression(_)
            | AstKind::SpreadElement(_)
            | AstKind::ParenthesizedExpression(_) => {}
            AstKind::ObjectProperty(p) if p.value.span() == child_span => {}
            AstKind::CallExpression(c) => {
                return c.arguments.iter().any(|a| a.span() == child_span)
                    && is_global_member(ctx, &c.callee, "JSON", "stringify");
            }
            _ => return false,
        }
        if cur.id() == NodeId::ROOT {
            return false;
        }
        child_span = cur.kind().span();
        cur = ctx.parent(cur.id());
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
        let Expression::StaticMemberExpression(callee) = &call.callee else {
            return;
        };
        match callee.property.name.as_str() {
            "parse" => {
                if call.optional || callee.optional || call.arguments.len() != 1 {
                    return;
                }
                if !is_global_member(ctx, &call.callee, "JSON", "parse") {
                    return;
                }
                let Some(arg) = call.arguments[0].as_expression() else {
                    return;
                };
                let Some(stringify) = json_call(ctx, arg, "stringify") else {
                    return;
                };
                if is_inside_json_stringify(ctx, node) {
                    return;
                }
                ctx.report(
                    Span::new(call.span.start, stringify.callee.span().end),
                    "Prefer `structuredClone(…)` over `JSON.parse(JSON.stringify(…))` to create a deep clone.",
                );
            }
            "cloneDeep" => {
                let path = dotted_path(&call.callee);
                if matches!(path.as_deref(), Some("_.cloneDeep" | "lodash.cloneDeep"))
                    && call.arguments.len() == 1
                    && !call.optional
                {
                    ctx.report(
                        callee.span,
                        format!(
                            "Prefer `structuredClone(…)` over `{}(…)` to create a deep clone.",
                            path.unwrap_or_default()
                        ),
                    );
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
            "S7784",
            "a.js",
            r#"
const clone = JSON.parse(JSON.stringify(original)); // Noncompliant {{Prefer `structuredClone(…)` over `JSON.parse(JSON.stringify(…))` to create a deep clone.}}
const obj = { copy: JSON.parse(JSON.stringify(state)) }; // Noncompliant
const c2 = _.cloneDeep(original); // Noncompliant {{Prefer `structuredClone(…)` over `_.cloneDeep(…)` to create a deep clone.}}
const s = JSON.stringify(JSON.parse(JSON.stringify(data)));
const s2 = JSON.stringify({ items: JSON.parse(JSON.stringify(self.items)) });
const s3 = JSON.stringify([JSON.parse(JSON.stringify(items[0]))]);
const s4 = JSON.stringify({ ...JSON.parse(JSON.stringify(config)) });
const p = JSON.parse(text);
const q = structuredClone(original);
"#,
        );
    }
}
