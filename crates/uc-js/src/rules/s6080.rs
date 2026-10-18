//! Disabling Mocha timeouts should be explicit
use crate::helpers::batch_tests::{
    MOCHA_TEST_CONSTRUCTS, TestEnv, inside_named_construct, resolve_value,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MAX_DELAY: f64 = 2_147_483_647.0;

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if !TestEnv::new(ctx).imports("chai") {
            return;
        }
        for node in ctx.nodes().iter() {
            let AstKind::CallExpression(call) = node.kind() else {
                continue;
            };
            let Expression::StaticMemberExpression(m) = &call.callee else {
                continue;
            };
            if m.property.name != "timeout" || !matches!(m.object, Expression::ThisExpression(_)) {
                continue;
            }
            let Some(value) = call.arguments.first().and_then(Argument::as_expression) else {
                continue;
            };
            let disabling = matches!(resolve_value(ctx, value), Expression::NumericLiteral(n) if n.value > MAX_DELAY);
            if disabling && inside_named_construct(ctx, node.id(), MOCHA_TEST_CONSTRUCTS) {
                ctx.report(
                    value.span(),
                    "Set this timeout to 0 if you want to disable it, otherwise use a value lower than 2147483648.",
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn timeouts() {
        check(
            "S6080",
            "a.test.js",
            r#"
import { expect } from 'chai';
describe('test suite', () => {
  it('unexpected timeout disabling', () => {
    this.timeout(2147483648); // Noncompliant {{Set this timeout to 0 if you want to disable it, otherwise use a value lower than 2147483648.}}
  });
  it('expected timeout disabling', () => {
    this.timeout(0);
    this.timeout(2147483647);
  });
  beforeAll(() => {
    const big = 3000000000;
    this.timeout(big); // Noncompliant
  });
});
this.timeout(2147483648);
"#,
        );
        check(
            "S6080",
            "a.test.js",
            "describe('x', () => { this.timeout(2147483648); });",
        );
    }
}
