//! Optional property declarations should not use both '?' and 'undefined' syntax
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Finds an `undefined` keyword among the (nested) members of a union.
fn find_undefined(t: &TSType<'_>) -> Option<Span> {
    match t {
        TSType::TSUndefinedKeyword(k) => Some(k.span),
        TSType::TSUnionType(u) => u.types.iter().find_map(find_undefined),
        TSType::TSParenthesizedType(p) if matches!(p.type_annotation, TSType::TSUnionType(_)) => {
            find_undefined(&p.type_annotation)
        }
        _ => None,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::PropertyDefinition, AstType::TSPropertySignature]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        if crate::helpers::batch_ts::no_type_info(ctx) {
            return;
        }
        let (optional, key, ann) = match node.kind() {
            AstKind::PropertyDefinition(p) => {
                (p.optional, p.key.span(), p.type_annotation.as_deref())
            }
            AstKind::TSPropertySignature(p) => {
                (p.optional, p.key.span(), p.type_annotation.as_deref())
            }
            _ => return,
        };
        let Some(ann) = ann else { return };
        if !optional {
            return;
        }
        let TSType::TSUnionType(_) = &ann.type_annotation else {
            return;
        };
        let Some(undef) = find_undefined(&ann.type_annotation) else {
            return;
        };
        let Some(q) = ctx
            .tokens_in(Span::new(key.end, ann.span.start))
            .iter()
            .find(|t| ctx.text(t.span()) == "?")
        else {
            return;
        };
        ctx.report_with(
            q.span(),
            "Consider removing 'undefined' type or '?' specifier, one of them is redundant.",
            [(undef, None)],
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn redundant_optional() {
        check(
            "S4782",
            "a.ts",
            r#"
interface I {
  a?: string | undefined; // Noncompliant {{Consider removing 'undefined' type or '?' specifier, one of them is redundant.}}
  b?: string;
  c: string | undefined;
  d?: (number | undefined) | string; // Noncompliant
  e?: undefined;
}
class C {
  x?: number | undefined; // Noncompliant
  y?: number;
}
"#,
        );
    }
}
