//! "switch" statements should not have too many "case" clauses
use crate::helpers::batch_smell1::first_token;
use crate::prelude::*;

pub struct Rule {
    max: u32,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            max: p.u32("maximum", p.u32("max", 30)),
        }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::SwitchStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::SwitchStatement(s) = node.kind() else {
            return;
        };
        let n = s
            .cases
            .iter()
            .filter(|c| c.test.is_some() && !c.consequent.is_empty())
            .count() as u32;
        if n > self.max {
            ctx.report(
                first_token(ctx, s.span.start),
                format!(
                    "Reduce the number of non-empty switch cases from {n} to at most {}.",
                    self.max
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn max_cases() {
        check_with(
            "S1479",
            "a.js",
            serde_json::json!({"maximum": 2}),
            r#"
switch (a) { // Noncompliant {{Reduce the number of non-empty switch cases from 3 to at most 2.}}
  case 1: f();
  case 2: f();
  case 3: f();
  case 4:
  default: f();
}
switch (a) {
  case 1: f();
  case 2:
  case 3: f();
  default: g();
}
"#,
        );
    }
}
