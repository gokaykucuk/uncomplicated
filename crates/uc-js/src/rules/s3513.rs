//! "arguments" should not be accessed directly
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let scoping = ctx.scoping();
        let Some(ids) = scoping.root_unresolved_references().get("arguments") else {
            return;
        };
        // Group the implicit `arguments` references by their (non-arrow) function.
        let mut by_function: FxHashMap<NodeId, Vec<Span>> = FxHashMap::default();
        let mut order: Vec<NodeId> = Vec::new();
        for id in ids {
            let r = scoping.get_reference(*id);
            let node = ctx.node(r.node_id());
            if let AstKind::StaticMemberExpression(m) = ctx.parent_kind(node.id())
                && m.property.name == "length"
                && m.object.span() == node.span()
            {
                continue;
            }
            let Some(f) = ctx
                .nodes()
                .ancestors(node.id())
                .find(|a| matches!(a.kind(), AstKind::Function(_)))
            else {
                continue;
            };
            by_function
                .entry(f.id())
                .or_insert_with(|| {
                    order.push(f.id());
                    Vec::new()
                })
                .push(node.span());
        }
        for f in order {
            let mut spans = by_function.remove(&f).unwrap_or_default();
            spans.sort_by_key(|s| s.start);
            let Some((first, rest)) = spans.split_first() else {
                continue;
            };
            ctx.report_with(
                *first,
                "Use the rest syntax to declare this function's arguments.",
                rest.iter()
                    .map(|s| {
                        (
                            *s,
                            Some("Replace this reference to \"arguments\".".to_string()),
                        )
                    })
                    .collect::<Vec<_>>(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn arguments_usage() {
        check(
            "S3513",
            "a.js",
            r#"
function concatenate() {
  let args = Array.prototype.slice.call(arguments); // Noncompliant {{Use the rest syntax to declare this function's arguments.}}
  const f = () => arguments[0];
  return args.join(", ");
}
function count() {
  return arguments.length;
}
function shadow(arguments) {
  return arguments[0];
}
function local() {
  var arguments = [];
  return arguments;
}
var top = arguments;
"#,
        );
    }
}
