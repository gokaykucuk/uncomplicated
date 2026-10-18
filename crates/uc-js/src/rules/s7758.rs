//! Unicode-aware string methods should be used for proper character handling
use crate::helpers::batch_unicorn::method_call;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression, AstType::StaticMemberExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::CallExpression(call) => {
                if let Some((m, "charCodeAt")) = method_call(call)
                    && !call.optional
                {
                    ctx.report(
                        m.property.span,
                        "Prefer `String#codePointAt()` over `String#charCodeAt()`.",
                    );
                }
            }
            AstKind::StaticMemberExpression(m)
                if m.property.name == "fromCharCode"
                    && !m.optional
                    && matches!(&m.object, Expression::Identifier(o) if o.name == "String") =>
            {
                ctx.report(
                    m.property.span,
                    "Prefer `String.fromCodePoint()` over `String.fromCharCode()`.",
                );
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn code_point() {
        check(
            "S7758",
            "a.js",
            r#"
const a = str.charCodeAt(0); // Noncompliant {{Prefer `String#codePointAt()` over `String#charCodeAt()`.}}
const b = String.fromCharCode(65); // Noncompliant {{Prefer `String.fromCodePoint()` over `String.fromCharCode()`.}}
const c = String.fromCharCode; // Noncompliant
const d = str.codePointAt(0);
const e = String.fromCodePoint(65);
const f = str.charCodeAt?.(0);
"#,
        );
    }
}
