//! Loops should not contain more than a single "break" or "continue" statement
use crate::helpers::batch_smell1::{first_token, is_loop};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let nodes = ctx.nodes();
        if !nodes.contains(AstType::BreakStatement) && !nodes.contains(AstType::ContinueStatement) {
            return;
        }
        // Loop node -> jumps counted for it.
        let mut jumps: FxHashMap<NodeId, Vec<(Span, bool)>> = FxHashMap::default();
        let mut order: Vec<NodeId> = Vec::new();
        for node in nodes.iter() {
            let (label, span, is_break) = match node.kind() {
                AstKind::BreakStatement(b) => {
                    (b.label.as_ref().map(|l| l.name.as_str()), b.span, true)
                }
                AstKind::ContinueStatement(c) => {
                    (c.label.as_ref().map(|l| l.name.as_str()), c.span, false)
                }
                _ => continue,
            };
            // Walk jump targets from the innermost one, counting the jump in every loop until the
            // target matching the label (no label: the innermost loop or switch).
            for anc in nodes.ancestors(node.id()) {
                let k = anc.kind();
                match k {
                    AstKind::Function(_)
                    | AstKind::ArrowFunctionExpression(_)
                    | AstKind::Program(_) => break,
                    AstKind::LabeledStatement(l) => {
                        if label == Some(l.label.name.as_str()) {
                            break;
                        }
                    }
                    AstKind::SwitchStatement(_) => {
                        if label.is_none() {
                            break;
                        }
                    }
                    _ if is_loop(&k) => {
                        let v = jumps.entry(anc.id()).or_default();
                        if v.is_empty() {
                            order.push(anc.id());
                        }
                        v.push((span, is_break));
                        if label.is_none() {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
        for id in order {
            let js = &jumps[&id];
            if js.len() > 1 {
                let start = ctx.node(id).kind().span().start;
                let mut js = js.clone();
                js.sort_by_key(|(s, _)| s.start);
                ctx.report_with(
                    first_token(ctx, start),
                    "Reduce the total number of \"break\" and \"continue\" statements in this loop to use one at most.",
                    js.into_iter().map(|(s, b)| {
                        (s, Some(if b { "\"break\" statement." } else { "\"continue\" statement." }.to_string()))
                    }),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn jumps() {
        check(
            "S135",
            "a.js",
            r#"
for (;;) {        // Noncompliant {{Reduce the total number of "break" and "continue" statements in this loop to use one at most.}}
  if (a) break;
  if (b) continue;
}
while (a) {
  if (b) break;
  switch (c) { case 1: break; case 2: break; }
}
outer: for (;;) {  // Noncompliant
  for (;;) {       // Noncompliant
    if (a) continue outer;
    if (b) break;
  }
  break;
}
do {
  [1].forEach(() => { for (;;) { break; } });
  break;
} while (a);
"#,
        );
    }
}
