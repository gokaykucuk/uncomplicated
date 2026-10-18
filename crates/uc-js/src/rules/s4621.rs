//! Union and intersection types should not include duplicated constituents
use crate::helpers::batch_ts::type_span;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::TSUnionType, AstType::TSIntersectionType]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let types = match node.kind() {
            AstKind::TSUnionType(u) => &u.types,
            AstKind::TSIntersectionType(i) => &i.types,
            _ => return,
        };
        // Groups by source text, in order of first occurrence.
        let mut groups: Vec<(&str, Vec<Span>)> = Vec::new();
        for t in types {
            let span = type_span(t);
            let text = ctx.text(span);
            match groups.iter_mut().find(|(k, _)| *k == text) {
                Some((_, v)) => v.push(span),
                None => groups.push((text, vec![span])),
            }
        }
        for (_, spans) in groups {
            if spans.len() < 2 {
                continue;
            }
            let primary = spans[1];
            let secondary = spans
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != 1)
                .map(|(i, s)| {
                    (
                        *s,
                        Some(
                            if i == 0 {
                                "Original"
                            } else {
                                "Another duplicate"
                            }
                            .to_string(),
                        ),
                    )
                });
            ctx.report_with(
                primary,
                "Remove this duplicated type or replace with another one.",
                secondary,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn duplicates() {
        check(
            "S4621",
            "a.ts",
            r#"
let a: string | number | string; // Noncompliant {{Remove this duplicated type or replace with another one.}}
let b: A & B & A & A; // Noncompliant
let c: string | number;
let d: (A | B) | (A | B); // Noncompliant
let e:
  | string
  | number
  | string; // Noncompliant
"#,
        );
    }
}
