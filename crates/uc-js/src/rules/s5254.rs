//! HTML elements should have a valid language attribute
use crate::helpers::batch_a11y::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// ISO 639-1 language codes (including deprecated codes still in the IANA registry).
static LANGUAGES: &[&str] = &[
    "aa", "ab", "ae", "af", "ak", "am", "an", "ar", "as", "av", "ay", "az", "ba", "be", "bg", "bh",
    "bi", "bm", "bn", "bo", "br", "bs", "ca", "ce", "ch", "co", "cr", "cs", "cu", "cv", "cy", "da",
    "de", "dv", "dz", "ee", "el", "en", "eo", "es", "et", "eu", "fa", "ff", "fi", "fj", "fo", "fr",
    "fy", "ga", "gd", "gl", "gn", "gu", "gv", "ha", "he", "hi", "ho", "hr", "ht", "hu", "hy", "hz",
    "ia", "id", "ie", "ig", "ii", "ik", "in", "io", "is", "it", "iu", "iw", "ja", "ji", "jv", "jw",
    "ka", "kg", "ki", "kj", "kk", "kl", "km", "kn", "ko", "kr", "ks", "ku", "kv", "kw", "ky", "la",
    "lb", "lg", "li", "ln", "lo", "lt", "lu", "lv", "mg", "mh", "mi", "mk", "ml", "mn", "mo", "mr",
    "ms", "mt", "my", "na", "nb", "nd", "ne", "ng", "nl", "nn", "no", "nr", "nv", "ny", "oc", "oj",
    "om", "or", "os", "pa", "pi", "pl", "ps", "pt", "qu", "rm", "rn", "ro", "ru", "rw", "sa", "sc",
    "sd", "se", "sg", "sh", "si", "sk", "sl", "sm", "sn", "so", "sq", "sr", "ss", "st", "su", "sv",
    "sw", "ta", "te", "tg", "th", "ti", "tk", "tl", "tn", "to", "tr", "ts", "tt", "tw", "ty", "ug",
    "uk", "ur", "uz", "ve", "vi", "vo", "wa", "wo", "xh", "yi", "yo", "za", "zh", "zu",
];

/// ISO 3166-1 alpha-2 regions plus deprecated / exceptionally reserved codes from the IANA registry.
static REGIONS: &[&str] = &[
    "ac", "ad", "ae", "af", "ag", "ai", "al", "am", "an", "ao", "aq", "ar", "as", "at", "au", "aw",
    "ax", "az", "ba", "bb", "bd", "be", "bf", "bg", "bh", "bi", "bj", "bl", "bm", "bn", "bo", "bq",
    "br", "bs", "bt", "bu", "bv", "bw", "by", "bz", "ca", "cc", "cd", "cf", "cg", "ch", "ci", "ck",
    "cl", "cm", "cn", "co", "cp", "cr", "cs", "cu", "cv", "cw", "cx", "cy", "cz", "dd", "de", "dg",
    "dj", "dk", "dm", "do", "dz", "ea", "ec", "ee", "eg", "eh", "er", "es", "et", "eu", "ez", "fi",
    "fj", "fk", "fm", "fo", "fr", "fx", "ga", "gb", "gd", "ge", "gf", "gg", "gh", "gi", "gl", "gm",
    "gn", "gp", "gq", "gr", "gs", "gt", "gu", "gw", "gy", "hk", "hm", "hn", "hr", "ht", "hu", "ic",
    "id", "ie", "il", "im", "in", "io", "iq", "ir", "is", "it", "je", "jm", "jo", "jp", "ke", "kg",
    "kh", "ki", "km", "kn", "kp", "kr", "kw", "ky", "kz", "la", "lb", "lc", "li", "lk", "lr", "ls",
    "lt", "lu", "lv", "ly", "ma", "mc", "md", "me", "mf", "mg", "mh", "mk", "ml", "mm", "mn", "mo",
    "mp", "mq", "mr", "ms", "mt", "mu", "mv", "mw", "mx", "my", "mz", "na", "nc", "ne", "nf", "ng",
    "ni", "nl", "no", "np", "nr", "nt", "nu", "nz", "om", "pa", "pe", "pf", "pg", "ph", "pk", "pl",
    "pm", "pn", "pr", "ps", "pt", "pw", "py", "qa", "re", "ro", "rs", "ru", "rw", "sa", "sb", "sc",
    "sd", "se", "sg", "sh", "si", "sj", "sk", "sl", "sm", "sn", "so", "sr", "ss", "st", "su", "sv",
    "sx", "sy", "sz", "ta", "tc", "td", "tf", "tg", "th", "tj", "tk", "tl", "tm", "tn", "to", "tp",
    "tr", "tt", "tv", "tw", "tz", "ua", "ug", "um", "un", "us", "uy", "uz", "va", "vc", "ve", "vg",
    "vi", "vn", "vu", "wf", "ws", "yd", "ye", "yt", "yu", "za", "zm", "zr", "zw",
];

fn is_alpha(s: &str) -> bool {
    s.bytes().all(|b| b.is_ascii_alphabetic())
}

fn is_alnum(s: &str) -> bool {
    s.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn is_region(s: &str) -> bool {
    (s.len() == 2 && is_alpha(s) && {
        let b = s.as_bytes();
        // Private use ranges: AA, QM-QZ, XA-XZ, ZZ.
        REGIONS.binary_search(&s).is_ok()
            || s == "aa"
            || s == "zz"
            || (b[0] == b'q' && b[1] >= b'm')
            || b[0] == b'x'
    }) || (s.len() == 3 && s.bytes().all(|b| b.is_ascii_digit()))
}

/// BCP 47 well-formedness, with the language and region subtags checked against the registry.
/// Subtags that can't be checked without the full IANA registry are accepted.
pub fn is_valid_lang(tag: &str) -> bool {
    let tag = tag.to_ascii_lowercase();
    if tag.starts_with("x-") || tag.starts_with("i-") {
        return tag.len() > 2
            && tag
                .split('-')
                .all(|s| !s.is_empty() && s.len() <= 8 && is_alnum(s));
    }
    let mut parts = tag.split('-').peekable();
    let Some(lang) = parts.next() else {
        return false;
    };
    let lang_ok = match lang.len() {
        2 => LANGUAGES.binary_search(&lang).is_ok(),
        3 => is_alpha(lang),
        _ => false,
    };
    if !lang_ok {
        return false;
    }
    // extlang
    let mut extlangs = 0;
    while let Some(p) = parts.peek() {
        if extlangs < 3 && p.len() == 3 && is_alpha(p) {
            extlangs += 1;
            parts.next();
        } else {
            break;
        }
    }
    if let Some(p) = parts.peek()
        && p.len() == 4
        && is_alpha(p)
    {
        parts.next();
    }
    if let Some(p) = parts.peek()
        && (p.len() == 2 || (p.len() == 3 && p.bytes().all(|b| b.is_ascii_digit())))
    {
        if !is_region(p) {
            return false;
        }
        parts.next();
    }
    while let Some(p) = parts.peek() {
        let variant = (5..=8).contains(&p.len()) && is_alnum(p)
            || (p.len() == 4 && p.as_bytes()[0].is_ascii_digit() && is_alnum(p));
        if !variant {
            break;
        }
        parts.next();
    }
    // extensions and private use
    while let Some(singleton) = parts.next() {
        if singleton.len() != 1 || !is_alnum(singleton) {
            return false;
        }
        let private = singleton == "x";
        let mut count = 0;
        while let Some(p) = parts.peek() {
            if p.len() == 1 && !private {
                break;
            }
            let min = if private { 1 } else { 2 };
            if p.len() < min || p.len() > 8 || !is_alnum(p) {
                return false;
            }
            parts.next();
            count += 1;
        }
        if count == 0 {
            return false;
        }
    }
    true
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXOpeningElement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::JSXOpeningElement(el) = node.kind() else {
            return;
        };
        if element_name(el) != "html" {
            return;
        }
        // html-has-lang
        if !prop_value_of(el, "lang").truthy() {
            ctx.report(el.name.span(), "<html> elements must have the lang prop.");
        }
        // lang
        for item in &el.attributes {
            let JSXAttributeItem::Attribute(a) = item else {
                continue;
            };
            if !attr_name(a).eq_ignore_ascii_case("lang") {
                continue;
            }
            let p = attr_prop(a);
            if is_interpolated(&p) {
                continue;
            }
            let valid = match literal_value(&p) {
                Val::Dynamic => true,
                Val::Str(s) => is_valid_lang(&s),
                _ => false,
            };
            if !valid {
                ctx.report(a.span, "lang attribute must have a valid value.");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn html_lang() {
        check(
            "S5254",
            "a.jsx",
            r##"
<html></html>; // Noncompliant {{<html> elements must have the lang prop.}}
<html lang="yolo"></html>; // Noncompliant {{lang attribute must have a valid value.}}
<html lang=""></html>; // Noncompliant 2
<html lang={undefined}></html>; // Noncompliant 2
<html lang="en_US"></html>; // Noncompliant
<html lang="zz"></html>; // Noncompliant
<html lang="en-JJ"></html>; // Noncompliant
<html lang="en"></html>;
<html lang="en-US"></html>;
<html lang="zh-Hant-TW"></html>;
<html lang="es-419"></html>;
<html lang="de-CH-1901"></html>;
<html lang="EN-gb"></html>;
<html lang="x-private"></html>;
<html lang={lang}></html>;
<html lang={`${lang}`}></html>;
<div lang="foo"></div>;
<HTML></HTML>;
"##,
        );
        assert!(super::is_valid_lang("sr-Latn-RS"));
        assert!(super::is_valid_lang("en-US-u-ca-gregory"));
        assert!(!super::is_valid_lang("en-"));
    }
}
