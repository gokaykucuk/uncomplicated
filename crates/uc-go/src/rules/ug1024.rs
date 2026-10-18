//! Struct tags should be well formed (go vet structtag).
//!
//! Tags must follow the `key:"value" key2:"value2"` convention of `reflect.StructTag`; a key must
//! not appear twice in the same tag.
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Validates a tag; returns the problem.
fn validate(tag: &str) -> Result<(), String> {
    let b = tag.as_bytes();
    let mut i = 0;
    let mut keys: Vec<&str> = Vec::new();
    loop {
        while i < b.len() && b[i] == b' ' {
            i += 1;
        }
        if i >= b.len() {
            return Ok(());
        }
        let start = i;
        while i < b.len() && b[i] > b' ' && b[i] != b':' && b[i] != b'"' && b[i] != 0x7f {
            i += 1;
        }
        if i == start {
            return Err("expected a key".into());
        }
        let key = &tag[start..i];
        if i >= b.len() || b[i] != b':' {
            return Err(format!("missing ':' after key \"{key}\""));
        }
        i += 1;
        if i >= b.len() || b[i] != b'"' {
            return Err(format!(
                "the value of \"{key}\" must be a quoted string without spaces before it"
            ));
        }
        i += 1;
        loop {
            if i >= b.len() {
                return Err(format!("unterminated value for \"{key}\""));
            }
            match b[i] {
                b'\\' => i += 2,
                b'"' => break,
                _ => i += 1,
            }
        }
        i += 1;
        if keys.contains(&key) {
            return Err(format!("duplicate key \"{key}\""));
        }
        keys.push(key);
        if i < b.len() && b[i] != b' ' {
            return Err(format!("missing space after the value of \"{key}\""));
        }
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["field_declaration"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let Some(tag) = node.child_by_field_name("tag") else {
            return;
        };
        let value = match tag.kind() {
            "raw_string_literal" => match string_value(ctx, tag) {
                Some(v) => v.to_string(),
                None => return,
            },
            _ => {
                // Interpreted literal: only plain `\"` and `\\` escapes are handled.
                let Some(v) = string_value(ctx, tag) else {
                    return;
                };
                let mut out = String::with_capacity(v.len());
                let mut it = v.chars();
                while let Some(c) = it.next() {
                    if c == '\\' {
                        match it.next() {
                            Some(e @ ('"' | '\\')) => out.push(e),
                            _ => return,
                        }
                    } else {
                        out.push(c);
                    }
                }
                out
            }
        };
        if let Err(problem) = validate(&value) {
            ctx.report(tag, format!("Malformed struct tag: {problem}."));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::validate;
    use crate::test_util::*;

    #[test]
    fn validation() {
        assert!(validate(r#"json:"a,omitempty" xml:"b""#).is_ok());
        assert!(validate("").is_ok());
        assert!(validate(r#"json:"a\"b""#).is_ok());
        assert!(validate("json:a").is_err());
        assert!(validate(r#"json: "a""#).is_err());
        assert!(validate(r#"json:"a",xml:"b""#).is_err());
        assert!(validate(r#"json:"a" json:"b""#).is_err());
    }

    #[test]
    fn rule() {
        check(
            "UG1024",
            r#"package p

type T struct {
	A int `json:"a,omitempty" db:"a"`
	B int `json:b` // Noncompliant {{Malformed struct tag: the value of "json" must be a quoted string without spaces before it.}}
	C int `json:"c",xml:"c"` // Noncompliant {{Malformed struct tag: missing space after the value of "json".}}
	D int `json:"d" json:"e"` // Noncompliant {{Malformed struct tag: duplicate key "json".}}
	E int "json:\"e\""
	F int `json:"f`  // Noncompliant
	G int
}
"#,
        );
    }
}
