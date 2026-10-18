//! XML parsers should not be vulnerable to XXE attacks
use crate::helpers::batch_security1::{call_fqn, get_property, sec};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const TARGETS: &[&str] = &[
    "libxmljs.parseXml",
    "libxmljs.parseXmlString",
    "libxmljs2.parseXml",
    "libxmljs2.parseXmlString",
];

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        if !matches!(callee_name(call), Some("parseXml" | "parseXmlString")) {
            return;
        }
        let Some(q) = call_fqn(ctx, call) else { return };
        if !TARGETS.contains(&q.as_str()) {
            return;
        }
        // Sonar only looks at an object literal passed directly.
        let Some(Expression::ObjectExpression(opts)) = call
            .arguments
            .get(1)
            .and_then(|a| a.as_expression())
            .map(strip_parens)
        else {
            return;
        };
        if let Some(p) = get_property(ctx, opts, "noent").found()
            && matches!(strip_parens(&p.value), Expression::BooleanLiteral(b) if b.value)
        {
            ctx.report_with(
                p.span,
                "Disable access to external entities in XML parsing.",
                [sec(call.callee.span(), None)],
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn xxe() {
        check(
            "S2755",
            "a.js",
            r#"
const libxmljs = require("libxmljs");
libxmljs.parseXmlString(xml, { noblanks: true, noent: true }); // Noncompliant {{Disable access to external entities in XML parsing.}}
libxmljs.parseXmlString(xml, { noent: false });
libxmljs.parseXml(xml);
import { parseXml } from 'libxmljs2';
parseXml(xml, {
  noent: true, // Noncompliant
});
other.parseXml(xml, { noent: true });
"#,
        );
    }
}
