//! Type intersections should use meaningful types
use crate::helpers::batch_ts::{strip_type_parens, type_parent, type_span};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn reference_decl<'s, 'a>(ctx: &Ctx<'a, 's>, r: &TSTypeReference<'a>) -> Option<&'s AstNode<'a>> {
    let TSTypeName::IdentifierReference(id) = &r.type_name else {
        return None;
    };
    let sym = ctx.scoping().get_reference(id.reference_id()).symbol_id()?;
    if !ctx.scoping().symbol_redeclarations(sym).is_empty() {
        return None;
    }
    Some(ctx.symbol_decl(sym))
}

/// True for types known to have no members: `null`, `undefined`, `void`, `{}` and empty
/// standalone interfaces (also through a local, non-generic type alias).
fn without_members<'a>(ctx: &Ctx<'a, '_>, t: &TSType<'a>, depth: u8) -> bool {
    match strip_type_parens(t) {
        TSType::TSNullKeyword(_) | TSType::TSUndefinedKeyword(_) | TSType::TSVoidKeyword(_) => true,
        TSType::TSTypeLiteral(l) => l.members.is_empty(),
        TSType::TSTypeReference(r) if r.type_arguments.is_none() && depth < 3 => {
            match reference_decl(ctx, r).map(|n| n.kind()) {
                Some(AstKind::TSInterfaceDeclaration(i)) => {
                    i.body.body.is_empty() && i.extends.is_empty()
                }
                Some(AstKind::TSTypeAliasDeclaration(a)) if a.type_parameters.is_none() => {
                    without_members(ctx, &a.type_annotation, depth + 1)
                }
                _ => false,
            }
        }
        _ => false,
    }
}

fn is_type_parameter(ctx: &Ctx<'_, '_>, r: &TSTypeReference<'_>) -> bool {
    let TSTypeName::IdentifierReference(id) = &r.type_name else {
        return false;
    };
    ctx.scoping()
        .get_reference(id.reference_id())
        .symbol_id()
        .is_some_and(|s| matches!(ctx.symbol_decl(s).kind(), AstKind::TSTypeParameter(_)))
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::TSIntersectionType]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::TSIntersectionType(inter) = node.kind() else {
            return;
        };
        let types = &inter.types;
        for t in types {
            let ty = match strip_type_parens(t) {
                TSType::TSAnyKeyword(_) => "any",
                TSType::TSNeverKeyword(_) => "never",
                _ => continue,
            };
            ctx.report(
                inter.span,
                format!("Simplify this intersection as it always has type \"{ty}\"."),
            );
            return;
        }
        let literal_union = types.len() == 2
            && matches!(type_parent(ctx, node.id()).kind(), AstKind::TSUnionType(_))
            && types
                .iter()
                .map(strip_type_parens)
                .find(|t| !matches!(t, TSType::TSTypeLiteral(l) if l.members.is_empty()))
                .is_some_and(|o| {
                    matches!(
                        o,
                        TSType::TSStringKeyword(_)
                            | TSType::TSNumberKeyword(_)
                            | TSType::TSTypeReference(_)
                    )
                });
        for (i, t) in types.iter().enumerate() {
            if !without_members(ctx, t, 0) || literal_union {
                continue;
            }
            let generic = types
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .any(|(_, s)| match strip_type_parens(s) {
                    TSType::TSMappedType(_) => true,
                    TSType::TSTypeReference(r) => {
                        r.type_arguments.is_some() || is_type_parameter(ctx, r)
                    }
                    _ => false,
                });
            if !generic {
                ctx.report(
                    type_span(t),
                    "Remove this type without members or change this type intersection.",
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn useless_intersection() {
        check(
            "S4335",
            "a.ts",
            r#"
function foo(p: MyType & null) {} // Noncompliant {{Remove this type without members or change this type intersection.}}
function bar(p: MyType & any) {} // Noncompliant {{Simplify this intersection as it always has type "any".}}
let n: A & never; // Noncompliant {{Simplify this intersection as it always has type "never".}}
interface Empty {}
let e: Foo & Empty; // Noncompliant
let l: Foo & {}; // Noncompliant
let ok1: Foo & Bar;
let ok2: 'a' | 'b' | (string & {});
type Simplify<T> = { [K in keyof T]: T[K] } & {};
function g<T>(x: T & {}) {}
let ok3: Partial<Foo> & {};
interface NotEmpty { a: string }
let ok4: Foo & NotEmpty;
interface Ext extends Foo {}
let ok5: Bar & Ext;
"#,
        );
    }
}
