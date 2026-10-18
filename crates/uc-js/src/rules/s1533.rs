//! Wrapper objects should not be used for primitive types
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const WRAPPERS: &[&str] = &["Boolean", "Number", "String"];

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::NewExpression, AstType::TSTypeReference]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::NewExpression(n) => {
                if let Expression::Identifier(id) = &n.callee
                    && WRAPPERS.contains(&id.name.as_str())
                {
                    ctx.report(
                        n.span,
                        format!("Remove this use of \"{}\" constructor.", id.name),
                    );
                }
            }
            AstKind::TSTypeReference(t) => {
                let text = ctx.text(t.span);
                if WRAPPERS.contains(&text) {
                    ctx.report(
                        t.span,
                        format!(
                            "Replace this \"{text}\" wrapper object with primitive type \"{}\".",
                            text.to_ascii_lowercase()
                        ),
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
    fn wrappers() {
        check(
            "S1533",
            "a.ts",
            r#"
let x = new Number(1); // Noncompliant {{Remove this use of "Number" constructor.}}
let y = new String("a"), z = new Boolean(true); // Noncompliant 2
let n: Number; // Noncompliant {{Replace this "Number" wrapper object with primitive type "number".}}
function f(s: String): Boolean {} // Noncompliant 2
let ok = Number("1");
let ok2: number = 1;
let ok3 = new Date();
"#,
        );
    }
}
