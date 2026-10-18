//! Function types should be preferred
use oxc_ast_visit::Visit;

use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Span of the single member, if it is a call or construct signature with a return type.
fn signature_span(members: &[TSSignature<'_>]) -> Option<Span> {
    match members {
        [TSSignature::TSCallSignatureDeclaration(c)] if c.return_type.is_some() => Some(c.span),
        [TSSignature::TSConstructSignatureDeclaration(c)] if c.return_type.is_some() => {
            Some(c.span)
        }
        _ => None,
    }
}

/// Finds the first `this` type, not looking into nested type literals.
#[derive(Default)]
struct ThisType(Option<Span>);

impl<'a> Visit<'a> for ThisType {
    fn visit_ts_this_type(&mut self, t: &TSThisType) {
        if self.0.is_none() {
            self.0 = Some(t.span);
        }
    }
    fn visit_ts_type_literal(&mut self, _: &TSTypeLiteral<'a>) {}
}

fn message(kind: &str) -> String {
    format!("{kind} has only a call signature, you should use a function type instead.")
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::TSInterfaceDeclaration, AstType::TSTypeLiteral]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::TSInterfaceDeclaration(i) => {
                let one_supertype = match i.extends.as_slice() {
                    [] => false,
                    [h] => {
                        !matches!(&h.type_name, TSTypeName::IdentifierReference(id) if id.name == "Function")
                    }
                    _ => true,
                };
                if one_supertype {
                    return;
                }
                let Some(span) = signature_span(&i.body.body) else {
                    return;
                };
                let mut this = ThisType::default();
                this.visit_ts_interface_declaration(i);
                match this.0 {
                    Some(t) => ctx.report(
                        t,
                        format!(
                            "`this` refers to the function type '{}', did you intend to use a generic `this` parameter like `<Self>(this: Self, ...) => Self` instead?",
                            i.id.name
                        ),
                    ),
                    None => ctx.report(span, message("Interface")),
                }
            }
            AstKind::TSTypeLiteral(l) => {
                if let Some(span) = signature_span(&l.members) {
                    ctx.report(span, message("Type literal"));
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
    fn function_type() {
        check(
            "S6598",
            "a.ts",
            r#"
interface Example {
  (): string; // Noncompliant {{Interface has only a call signature, you should use a function type instead.}}
}
function foo(example: { (): number }): number { // Noncompliant {{Type literal has only a call signature, you should use a function type instead.}}
  return example();
}
interface F extends Function {
  (x: number): void; // Noncompliant
}
interface G extends H {
  (x: number): void;
}
interface Two {
  (): void;
  prop: string;
}
interface Self {
  (arg: string): this; // Noncompliant {{`this` refers to the function type 'Self', did you intend to use a generic `this` parameter like `<Self>(this: Self, ...) => Self` instead?}}
}
interface Ctor {
  new (): Foo; // Noncompliant
}
type T = () => void;
"#,
        );
    }
}
