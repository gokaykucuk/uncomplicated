//! Interfaces should not be empty
use crate::prelude::*;

pub struct Rule {
    allow_single_extends: bool,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            allow_single_extends: p.bool("allowSingleExtends", true),
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
        &[AstType::TSInterfaceDeclaration]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::TSInterfaceDeclaration(i) = node.kind() else {
            return;
        };
        if !i.body.body.is_empty() {
            return;
        }
        match i.extends.len() {
            0 => ctx.report(i.id.span, "An empty interface is equivalent to `{}`."),
            1 if !self.allow_single_extends => {
                let utility = matches!(&i.extends[0].type_name,
                    TSTypeName::IdentifierReference(id) if UTILITY_TYPES.contains(&id.name.as_str()));
                if !utility {
                    ctx.report(
                        i.id.span,
                        "An interface declaring no members is equivalent to its supertype.",
                    );
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn empty_interface() {
        check(
            "S4023",
            "a.ts",
            r#"
interface A {} // Noncompliant {{An empty interface is equivalent to `{}`.}}
interface B extends A {}
interface C extends A, B {}
interface D { x: number }
"#,
        );
        check_with(
            "S4023",
            "a.ts",
            serde_json::json!({"allowSingleExtends": false}),
            r#"
interface B extends A {} // Noncompliant {{An interface declaring no members is equivalent to its supertype.}}
interface P extends Partial<A> {}
"#,
        );
    }
}
