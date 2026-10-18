//! Labels should not be used
use crate::helpers::batch_smell1::{is_loop, is_loop_stmt};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MSG: &str = "Refactor the code to remove this label and the need for it.";

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let nodes = ctx.nodes();
        if !nodes.contains(AstType::LabeledStatement) {
            return;
        }
        // Labeled statement -> (has references, all references need the label).
        let mut refs: FxHashMap<NodeId, (bool, bool)> = FxHashMap::default();
        let mut labels: Vec<NodeId> = Vec::new();
        for node in nodes.iter() {
            let label = match node.kind() {
                AstKind::LabeledStatement(_) => {
                    labels.push(node.id());
                    continue;
                }
                AstKind::BreakStatement(b) => b.label.as_ref(),
                AstKind::ContinueStatement(c) => c.label.as_ref(),
                _ => continue,
            };
            let Some(label) = label else { continue };
            // Find the matching labeled statement; note whether a loop/switch lies in between
            // (below the labeled body).
            let mut path: Vec<NodeId> = Vec::new();
            for anc in nodes.ancestors(node.id()) {
                if let AstKind::LabeledStatement(l) = anc.kind()
                    && l.label.name == label.name
                {
                    // `path` holds ancestors below the label; the last one is the labeled body.
                    let below_body = &path[..path.len().saturating_sub(1)];
                    let nested = below_body.iter().any(|id| {
                        let k = nodes.kind(*id);
                        is_loop(&k) || matches!(k, AstKind::SwitchStatement(_))
                    });
                    let e = refs.entry(anc.id()).or_insert((false, true));
                    e.0 = true;
                    e.1 &= nested;
                    break;
                }
                if matches!(
                    anc.kind(),
                    AstKind::Function(_) | AstKind::ArrowFunctionExpression(_)
                ) {
                    break;
                }
                path.push(anc.id());
            }
        }
        for id in labels {
            let AstKind::LabeledStatement(l) = nodes.kind(id) else {
                continue;
            };
            let report = if !is_loop_stmt(&l.body) {
                true
            } else {
                match refs.get(&id) {
                    None => true,
                    Some((has, all_nested)) => !*has || !*all_nested,
                }
            };
            if report {
                ctx.report(l.label.span, MSG);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn labels() {
        check(
            "S1119",
            "a.js",
            r#"
outer: for (;;) {          // Noncompliant {{Refactor the code to remove this label and the need for it.}}
  break outer;
}
outer2: for (;;) {
  for (;;) { break outer2; }
}
unused: for (;;) {}       // Noncompliant
block: { break block; }   // Noncompliant
sw: while (a) {
  switch (b) { case 1: continue sw; }
}
mixed: for (;;) {         // Noncompliant
  for (;;) { continue mixed; }
  continue mixed;
}
"#,
        );
    }
}
