//! Switch cases should end with an unconditional "break" statement
use crate::helpers::batch_security1::{FlowOpts, list_flow};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Reports only when the end of the case is certainly reachable.
const OPTS: FlowOpts = FlowOpts {
    optimistic: false,
    process_exit: true,
};

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::SwitchStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::SwitchStatement(s) = node.kind() else {
            return;
        };
        for (i, case) in s.cases.iter().enumerate() {
            let Some(next) = s.cases.get(i + 1) else {
                break;
            };
            if case.consequent.is_empty() {
                continue;
            }
            if !list_flow(&case.consequent, OPTS, &mut None).normal {
                continue;
            }
            // A comment after the case documents an intentional fall-through.
            if ctx.has_comments_in(Span::new(case.span.end, next.span.start)) {
                continue;
            }
            let kw = token_after(ctx, case.span.start).unwrap_or(case.span);
            ctx.report(kw, "End this switch case with an unconditional break, continue, return or throw statement.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn fallthrough() {
        check(
            "S128",
            "a.js",
            r#"
function f(x) {
  switch (x) {
    case 0:
    case 1: // Noncompliant {{End this switch case with an unconditional break, continue, return or throw statement.}}
      foo();
    case 2:
      foo();
      break;
    case 3:
      if (a) { return 1; } else { throw new Error(); }
    case 4: // Noncompliant
      if (a) { break; }
    case 5:
      foo();
      // falls through
    case 6:
      process.exit(1);
    case 7:
      while (true) { foo(); }
    case 8: { // Noncompliant
      bar();
    }
    case 9:
      try { return 1; } finally { cleanup(); }
    case 10: // Noncompliant
      try { foo(); } catch (e) { return 2; }
    default:
      bar();
  }
}
"#,
        );
    }
}
