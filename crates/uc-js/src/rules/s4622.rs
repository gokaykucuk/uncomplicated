//! Union types should not have too many elements
use crate::helpers::batch_ts::type_parent;
use crate::prelude::*;

pub struct Rule {
    threshold: u32,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            threshold: p.u32("threshold", 3),
        }
    }
}

const UTILITY_TYPES: &[&str] = &[
    "Awaited",
    "Partial",
    "Required",
    "Readonly",
    "Record",
    "Pick",
    "Omit",
    "Exclude",
    "Extract",
    "NonNullable",
    "Parameters",
    "ConstructorParameters",
    "ReturnType",
    "InstanceType",
    "ThisParameterType",
    "OmitThisParameter",
    "ThisType",
    "Uppercase",
    "Lowercase",
    "Capitalize",
    "Uncapitalize",
];

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::TSUnionType]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::TSUnionType(u) = node.kind() else {
            return;
        };
        if u.types.len() as u32 <= self.threshold {
            return;
        }
        let parent = type_parent(ctx, node.id());
        match parent.kind() {
            AstKind::TSTypeAliasDeclaration(_) => return,
            AstKind::TSTypeParameterInstantiation(_) => {
                if let AstKind::TSTypeReference(r) = ctx.parent_kind(parent.id())
                    && let TSTypeName::IdentifierReference(id) = &r.type_name
                    && UTILITY_TYPES.contains(&id.name.as_str())
                {
                    return;
                }
            }
            _ => {}
        }
        ctx.report(
            u.span,
            format!(
                "Refactor this union type to have less than {} elements.",
                self.threshold
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn union_size() {
        check(
            "S4622",
            "a.ts",
            r#"
let a: 'a' | 'b' | 'c' | 'd'; // Noncompliant {{Refactor this union type to have less than 3 elements.}}
let b: 'a' | 'b' | 'c';
type T = 'a' | 'b' | 'c' | 'd';
let c: Omit<X, 'a' | 'b' | 'c' | 'd'>;
let d: Foo<'a' | 'b' | 'c' | 'd'>; // Noncompliant
function f(x: string | number | boolean | null) {} // Noncompliant
"#,
        );
    }
}
