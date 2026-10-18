//! Images should have a non-redundant alternate description
use crate::helpers::batch_a11y::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const REDUNDANT_WORDS: &[&str] = &["image", "photo", "picture"];

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `/(?!{)\bword\b(?!})/i`
fn contains_word(text: &str, word: &str) -> bool {
    let hay = text.as_bytes();
    let w = word.as_bytes();
    if hay.len() < w.len() {
        return false;
    }
    (0..=hay.len() - w.len()).any(|i| {
        hay[i..i + w.len()].eq_ignore_ascii_case(w)
            && (i == 0 || !is_word_byte(hay[i - 1]))
            && hay
                .get(i + w.len())
                .is_none_or(|&b| !is_word_byte(b) && b != b'}')
    })
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXOpeningElement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::JSXOpeningElement(el) = node.kind() else {
            return;
        };
        let tag = element_name(el);
        if tag != "img" {
            return;
        }
        let Some(alt) = get_prop(el, "alt") else {
            return;
        };
        let Val::Str(value) = literal_value(&alt) else {
            return;
        };
        if is_hidden_from_screen_reader(tag, el) {
            return;
        }
        if REDUNDANT_WORDS.iter().any(|w| contains_word(&value, w)) {
            ctx.report(
                el.span,
                "Redundant alt attribute. Screen-readers already announce `img` tags as an image. You don\u{2019}t need to use the words `image`, `photo,` or `picture` (or any specified custom words) in the alt prop.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn redundant_alt() {
        check(
            "S6851",
            "a.jsx",
            r##"
<img src="foo" alt="Photo of foo being weird." />; // Noncompliant {{Redundant alt attribute. Screen-readers already announce `img` tags as an image. You don’t need to use the words `image`, `photo,` or `picture` (or any specified custom words) in the alt prop.}}
<img src="foo" alt="Image of me at a bar!" />; // Noncompliant
<img src="foo" alt="Picture of baz fixing a bug." />; // Noncompliant
<img src="foo" alt={`picture of ${name}`} />; // Noncompliant
<img src="foo" alt="Foo eating a sandwich." />;
<img src="foo" alt="Photography of the sea" />;
<img src="foo" alt="photo" aria-hidden />;
<img src="foo" alt={`${photo}`} />;
<img src="foo" alt={alt} />;
<img src="foo" />;
<Image alt="photo" />;
"##,
        );
    }
}
