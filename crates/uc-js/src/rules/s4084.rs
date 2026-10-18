//! Media elements should have captions
use crate::helpers::batch_a11y::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXElement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::JSXElement(e) = node.kind() else {
            return;
        };
        let el = &e.opening_element;
        if !matches!(element_name(el), "audio" | "video") {
            return;
        }
        if literal_value_of(el, "muted") == Val::Bool(true) {
            return;
        }
        let has_caption = e.children.iter().any(|c| match c {
            JSXChild::Element(t) if element_name(&t.opening_element) == "track" => {
                literal_value_of(&t.opening_element, "kind")
                    .as_str()
                    .is_some_and(|k| k.eq_ignore_ascii_case("captions"))
            }
            _ => false,
        });
        if !has_caption {
            ctx.report(
                el.name.span(),
                "Media elements such as <audio> and <video> must have a <track> for captions.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn media_caption() {
        check(
            "S4084",
            "a.jsx",
            r##"
<video />; // Noncompliant {{Media elements such as <audio> and <video> must have a <track> for captions.}}
<video {...props} />; // Noncompliant
<audio><track kind="subtitles" /></audio>; // Noncompliant
<video muted={false} />; // Noncompliant
<video><track kind="captions" /></video>;
<audio><track kind="Captions" /></audio>;
<video muted />;
<video muted="true" />;
<Video />;
"##,
        );
    }
}
