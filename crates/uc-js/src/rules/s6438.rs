//! Comments inside JSX expressions should be enclosed in curly braces
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// `/^\s*\/(\/|\*)/m`: some line of the text starts (after blanks) with `//` or `/*`.
fn looks_like_comment(text: &str) -> bool {
    text.split('\n').any(|line| {
        let t = line.trim_start();
        t.starts_with("//") || t.starts_with("/*")
    })
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXText]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::JSXText(t) = node.kind() else {
            return;
        };
        if !matches!(
            ctx.parent_kind(node.id()),
            AstKind::JSXElement(_) | AstKind::JSXFragment(_)
        ) {
            return;
        }
        if looks_like_comment(ctx.text(t.span)) {
            ctx.report(
                t.span,
                "Comments inside children section of tag should be placed inside braces",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn comment_text() {
        check(
            "S6438",
            "a.jsx",
            r#"
const a = <div>// empty div</div>; // Noncompliant {{Comments inside children section of tag should be placed inside braces}}
const b = <div>/* empty div */</div>; // Noncompliant
const c = <div>
  {/* fine */}
  http://example.com
</div>;
const d = <>
  text // Noncompliant@-1
  // not ok
</>;
"#,
        );
    }
}
