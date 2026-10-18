//! Redundant type aliases should not be used
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::TSTypeAliasDeclaration]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::TSTypeAliasDeclaration(a) = node.kind() else {
            return;
        };
        let ann = &a.type_annotation;
        let redundant = match ann {
            TSType::TSAnyKeyword(_)
            | TSType::TSBigIntKeyword(_)
            | TSType::TSBooleanKeyword(_)
            | TSType::TSNeverKeyword(_)
            | TSType::TSNullKeyword(_)
            | TSType::TSNumberKeyword(_)
            | TSType::TSStringKeyword(_)
            | TSType::TSSymbolKeyword(_)
            | TSType::TSUndefinedKeyword(_)
            | TSType::TSUnknownKeyword(_)
            | TSType::TSVoidKeyword(_) => true,
            TSType::TSTypeReference(r) if r.type_arguments.is_none() => match &r.type_name {
                TSTypeName::IdentifierReference(id) => ctx
                    .scoping()
                    .get_reference(id.reference_id())
                    .symbol_id()
                    .is_some_and(|s| {
                        matches!(
                            ctx.symbol_decl(s).kind(),
                            AstKind::TSTypeAliasDeclaration(_)
                        )
                    }),
                _ => false,
            },
            _ => false,
        };
        if !redundant {
            return;
        }
        let Some(tok) = ctx.tokens_in(ann.span()).first() else {
            return;
        };
        ctx.report(
            a.id.span,
            format!(
                "Remove this redundant type alias and replace its occurrences with \"{}\".",
                ctx.text(tok.span())
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn redundant_alias() {
        check(
            "S6564",
            "a.ts",
            r#"
type Numbers = number[];
type MaybeString = string | undefined;
type pi = 3.14;
type NoIdea = WhoKnows;
type StringArray = Array<string>;
type MyAny = any; // Noncompliant {{Remove this redundant type alias and replace its occurrences with "any".}}
type MyString = string; // Noncompliant {{Remove this redundant type alias and replace its occurrences with "string".}}
type MyVoid = void; // Noncompliant
type MyPiAlias = pi; // Noncompliant {{Remove this redundant type alias and replace its occurrences with "pi".}}
interface I {}
type MyI = I;
"#,
        );
    }
}
