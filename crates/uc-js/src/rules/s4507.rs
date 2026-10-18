//! Debugging features should not be enabled in production
use crate::helpers::batch_security1::{call_fqn, flatten_args, parents, src_mentions};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if !src_mentions(ctx, &["errorhandler"]) {
            return;
        }
        for node in ctx.nodes().iter() {
            let AstKind::CallExpression(call) = node.kind() else {
                continue;
            };
            let Expression::StaticMemberExpression(m) = strip_parens(&call.callee) else {
                continue;
            };
            if m.property.name != "use" || call.arguments.is_empty() {
                continue;
            }
            if parents(ctx, node.id()).any(|a| matches!(a.kind(), AstKind::IfStatement(_))) {
                continue;
            }
            let mut middlewares = Vec::new();
            flatten_args(ctx, &call.arguments, &mut middlewares);
            for mw in middlewares {
                if let Expression::CallExpression(c) = mw
                    && call_fqn(ctx, c).as_deref() == Some("errorhandler")
                {
                    ctx.report(
                        c.span,
                        "Make sure this debug feature is deactivated before delivering the code in production.",
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn production_debug() {
        check(
            "S4507",
            "a.js",
            r#"
const errorhandler = require('errorhandler');
app.use(errorhandler()); // Noncompliant {{Make sure this debug feature is deactivated before delivering the code in production.}}
if (process.env.NODE_ENV === 'development') {
  app.use(errorhandler());
}
const eh = errorhandler(); // Noncompliant
app.use([eh]);
app.use(other());
"#,
        );
    }
}
