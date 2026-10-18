//! Cognitive Complexity of functions should not be too high.
use crate::helpers::cognitive;
use crate::prelude::*;

pub struct Rule {
    threshold: u32,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self { threshold: p.u32("threshold", 15) }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Function, AstType::ArrowFunctionExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let points = match node.kind() {
            AstKind::Function(f) => match &f.body {
                Some(body) => cognitive::function_points(ctx.tokens, body),
                None => return,
            },
            AstKind::ArrowFunctionExpression(a) => cognitive::arrow_points(ctx.tokens, a),
            _ => return,
        };
        let total: u32 = points.iter().map(|p| p.value).sum();
        if total > self.threshold {
            let secondary = points.iter().map(|p| {
                let msg = if p.value == 1 { "+1".to_string() } else { format!("+{} (incl. {} for nesting)", p.value, p.value - 1) };
                (p.span, Some(msg))
            });
            ctx.report_issue(
                function_report_span(ctx, node),
                format!(
                    "Refactor this function to reduce its Cognitive Complexity from {total} to the {} allowed.",
                    self.threshold
                ),
                secondary.collect(),
                Some((total - self.threshold) as f32),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn cognitive() {
        check_with(
            "S3776",
            "a.js",
            serde_json::json!({"threshold": 3}),
            r#"
function ok(a) {          // 3
  if (a) {                // +1
    for (;;) {}           // +2
  }
}
function ko(a, b) {       // Noncompliant {{Refactor this function to reduce its Cognitive Complexity from 7 to the 3 allowed.}}
  if (a && b) {           // +1 +1
    while (a) {           // +2
      if (b) {}           // +3
    }
  }
}
const f = () => {         // Noncompliant
  if (a) {} else if (b) {} else {}  // +1 +1 +1
  if (c || d) {}          // +1 (|| not counted)
};
function outer() {
  return function inner(x) { // nested functions are reported on their own
    if (x) {}
  };
}
"#,
        );
    }
}
