//! Optional boolean parameters should have default value
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_optional_boolean(p: &FormalParameter<'_>) -> bool {
    let Some(ann) = &p.type_annotation else {
        return false;
    };
    match &ann.type_annotation {
        TSType::TSBooleanKeyword(_) => p.optional,
        TSType::TSUnionType(u) => {
            u.types.len() == 2
                && u.types
                    .iter()
                    .any(|t| matches!(t, TSType::TSBooleanKeyword(_)))
                && u.types
                    .iter()
                    .any(|t| matches!(t, TSType::TSUndefinedKeyword(_)))
        }
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Function, AstType::ArrowFunctionExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let params = match node.kind() {
            AstKind::Function(f)
                if matches!(
                    f.r#type,
                    FunctionType::FunctionDeclaration | FunctionType::FunctionExpression
                ) =>
            {
                &f.params
            }
            AstKind::ArrowFunctionExpression(a) => &a.params,
            _ => return,
        };
        for p in &params.items {
            let BindingPattern::BindingIdentifier(id) = &p.pattern else {
                continue;
            };
            if p.initializer.is_some() || p.accessibility.is_some() || p.readonly || p.r#override {
                continue;
            }
            if is_optional_boolean(p) {
                ctx.report(
                    p.span,
                    format!(
                        "Provide a default value for '{}' so that the logic of the function is more evident when this parameter is missing. Consider defining another function if providing default value is not possible.",
                        id.name
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn bool_default() {
        check(
            "S4798",
            "a.ts",
            r#"
function f(a?: boolean) {} // Noncompliant {{Provide a default value for 'a' so that the logic of the function is more evident when this parameter is missing. Consider defining another function if providing default value is not possible.}}
function g(a: boolean | undefined) {} // Noncompliant
const h = (b?: boolean) => {}; // Noncompliant
function ok(a = false, b: boolean, c?: string) {}
declare function decl(a?: boolean): void;
interface I { m(a?: boolean): void; }
class C {
  m(a?: boolean) {} // Noncompliant
  constructor(private a?: boolean) {}
}
"#,
        );
    }
}
