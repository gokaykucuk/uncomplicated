//! Template literal placeholder syntax should not be used in regular strings
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Equivalent of the regex `/\$\{[^}]+\}/`.
fn has_placeholder(s: &str) -> bool {
    let mut from = 0;
    while let Some(i) = s[from..].find("${") {
        let start = from + i + 2;
        if let Some(j) = s[start..].find('}')
            && j > 0
        {
            return true;
        }
        from = start;
    }
    false
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::StringLiteral]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::StringLiteral(s) = node.kind() else {
            return;
        };
        if has_placeholder(&s.value) {
            ctx.report(s.span, "Unexpected template string expression.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn template_curly() {
        check(
            "S3786",
            "a.js",
            r#"
const a = "Hello ${name}"; // Noncompliant {{Unexpected template string expression.}}
const b = 'Time: ${12 * 60 * 1000}'; // Noncompliant
const c = `Hello ${name}`;
const d = "${}";
const e = "$ {x}";
const f = "{x}";
"#,
        );
    }
}
