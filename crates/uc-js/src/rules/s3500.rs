//! "const" variables should not be reassigned
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::VariableDeclaration]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::VariableDeclaration(decl) = node.kind() else {
            return;
        };
        if decl.kind != VariableDeclarationKind::Const {
            return;
        }
        for d in &decl.declarations {
            for id in d.id.get_binding_identifiers() {
                let sym = id.symbol_id();
                for r in ctx.scoping().get_resolved_references(sym) {
                    if r.is_write() {
                        let span = ctx.node(r.node_id()).kind().span();
                        ctx.report_with(
                            span,
                            format!("Correct this attempt to modify \"{}\" or use \"let\" in its declaration.", id.name),
                            [(decl.span, Some("Const declaration".to_string()))],
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn const_reassign() {
        check(
            "S3500",
            "a.js",
            r#"
const pi = 3.14;
pi = 3; // Noncompliant {{Correct this attempt to modify "pi" or use "let" in its declaration.}}
pi++; // Noncompliant
const { a, b: [c] } = obj;
c += 1; // Noncompliant
[a] = [1]; // Noncompliant
let x = 1;
x = 2;
const o = {};
o.p = 1;
for (const k of ks) { foo(k); }
"#,
        );
    }
}
