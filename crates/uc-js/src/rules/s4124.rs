//! Constructors should not be declared inside interfaces
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const INTERFACE_MSG: &str = "Interfaces cannot be constructed, only classes.";

/// True if `ret` is a reference to the type called `name`.
fn returns_named(ret: Option<&TSTypeAnnotation<'_>>, name: &str) -> bool {
    matches!(ret.map(|r| &r.type_annotation), Some(TSType::TSTypeReference(r))
        if matches!(&r.type_name, TSTypeName::IdentifierReference(id) if id.name == name))
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::TSConstructSignatureDeclaration,
            AstType::TSMethodSignature,
            AstType::MethodDefinition,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::TSConstructSignatureDeclaration(s) => {
                if let AstKind::TSInterfaceBody(_) = ctx.parent_kind(node.id())
                    && let AstKind::TSInterfaceDeclaration(i) =
                        ctx.parent(ctx.parent(node.id()).id()).kind()
                    && returns_named(s.return_type.as_deref(), &i.id.name)
                {
                    ctx.report(s.span, INTERFACE_MSG);
                }
            }
            AstKind::TSMethodSignature(m) => {
                if !m.computed
                    && matches!(&m.key, PropertyKey::StaticIdentifier(id) if id.name == "constructor")
                {
                    ctx.report(m.span, INTERFACE_MSG);
                }
            }
            AstKind::MethodDefinition(m) => {
                if m.computed
                    || !matches!(&m.key, PropertyKey::StaticIdentifier(id) if id.name == "new")
                    || m.value.body.is_some()
                {
                    return;
                }
                if let AstKind::ClassBody(_) = ctx.parent_kind(node.id())
                    && let AstKind::Class(c) = ctx.parent(ctx.parent(node.id()).id()).kind()
                    && let Some(id) = &c.id
                    && returns_named(m.value.return_type.as_deref(), &id.name)
                {
                    ctx.report(m.span, "Class cannot have method named `new`.");
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
    fn misused_new() {
        check(
            "S4124",
            "a.ts",
            r#"
interface I {
  new (): I; // Noncompliant {{Interfaces cannot be constructed, only classes.}}
  constructor(): void; // Noncompliant
}
interface J {
  new (): K;
}
declare class C {
  new(): C; // Noncompliant {{Class cannot have method named `new`.}}
}
class D {
  new(): D { return this; }
}
type T = { new (): T };
"#,
        );
    }
}
