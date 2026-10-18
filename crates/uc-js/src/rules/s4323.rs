//! Type aliases should be used
use crate::helpers::batch_ts::type_span;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const TYPE_THRESHOLD: usize = 2;
const USAGE_THRESHOLD: usize = 2;

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if !ctx.nodes().contains(AstType::TSUnionType)
            && !ctx.nodes().contains(AstType::TSIntersectionType)
        {
            return;
        }
        // key -> (is_union of first occurrence, spans), in order of first occurrence.
        let mut index: FxHashMap<String, usize> = FxHashMap::default();
        let mut usages: Vec<(bool, Vec<Span>)> = Vec::new();
        for node in ctx.nodes().iter() {
            let (types, is_union, span) = match node.kind() {
                AstKind::TSUnionType(u) => (&u.types, true, u.span),
                AstKind::TSIntersectionType(i) => (&i.types, false, i.span),
                _ => continue,
            };
            if types.len() <= TYPE_THRESHOLD {
                continue;
            }
            if is_union
                && types
                    .iter()
                    .filter(|t| {
                        !matches!(t, TSType::TSNullKeyword(_) | TSType::TSUndefinedKeyword(_))
                    })
                    .count()
                    == 1
            {
                continue;
            }
            if ctx
                .ancestors(node.id())
                .any(|a| matches!(a.kind(), AstKind::TSTypeAliasDeclaration(_)))
            {
                continue;
            }
            let mut texts: Vec<&str> = types.iter().map(|t| ctx.text(type_span(t))).collect();
            texts.sort_unstable();
            let key = texts.join("|");
            match index.get(&key) {
                Some(&i) => usages[i].1.push(span),
                None => {
                    index.insert(key, usages.len());
                    usages.push((is_union, vec![span]));
                }
            }
        }
        for (is_union, spans) in usages {
            if spans.len() > USAGE_THRESHOLD {
                let kind = if is_union { "union" } else { "intersection" };
                ctx.report_with(
                    spans[0],
                    format!("Replace this {kind} type with a type alias."),
                    spans[1..]
                        .iter()
                        .map(|s| (*s, Some("Following occurrence.".to_string()))),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn type_alias() {
        check(
            "S4323",
            "a.ts",
            r#"
function foo(x: string | null | number) {} // Noncompliant {{Replace this union type with a type alias.}}
let a: string | null | number;
let b: number | string | null;
let c: string | null | undefined;
let d: A & B & C;
let e: A & B & C;
type T = A | B | C;
let f: A | B;
let g: A | B;
let h: A | B;
"#,
        );
    }
}
