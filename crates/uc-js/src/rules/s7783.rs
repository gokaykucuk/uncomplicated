//! "trimLeft()" and "trimRight()" should be replaced with "trimStart()" and "trimEnd()"
use crate::helpers::batch_unicorn::is_method_call;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let Some(m) = is_method_call(call, &["trimLeft", "trimRight"], Some(0), true) else {
            return;
        };
        if call.optional {
            return;
        }
        let method = m.property.name.as_str();
        let replacement = if method == "trimLeft" {
            "trimStart"
        } else {
            "trimEnd"
        };
        ctx.report(
            m.property.span,
            format!("Prefer `String#{replacement}()` over `String#{method}()`."),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn trim_start_end() {
        check(
            "S7783",
            "a.js",
            r#"
const a = str.trimLeft(); // Noncompliant {{Prefer `String#trimStart()` over `String#trimLeft()`.}}
const b = str?.trimRight(); // Noncompliant {{Prefer `String#trimEnd()` over `String#trimRight()`.}}
const c = str.trimStart();
const d = str.trimLeft?.();
const e = str.trimLeft(1);
"#,
        );
    }
}
