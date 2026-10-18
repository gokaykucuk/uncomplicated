//! Modern Blob methods should be used instead of FileReader
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
        let Some(m) = is_method_call(call, &["readAsText", "readAsArrayBuffer"], Some(1), false)
        else {
            return;
        };
        let method = m.property.name.as_str();
        let replacement = if method == "readAsArrayBuffer" {
            "arrayBuffer"
        } else {
            "text"
        };
        ctx.report(
            m.property.span,
            format!("Prefer `Blob#{replacement}()` over `FileReader#{method}(blob)`."),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn blob_reading() {
        check(
            "S7756",
            "a.js",
            r#"
const reader = new FileReader();
reader.readAsText(blob); // Noncompliant {{Prefer `Blob#text()` over `FileReader#readAsText(blob)`.}}
reader.readAsArrayBuffer(blob); // Noncompliant {{Prefer `Blob#arrayBuffer()` over `FileReader#readAsArrayBuffer(blob)`.}}
reader.readAsDataURL(blob);
reader.readAsText(blob, 'utf-8');
reader?.readAsText(blob);
const text = await blob.text();
"#,
        );
    }
}
