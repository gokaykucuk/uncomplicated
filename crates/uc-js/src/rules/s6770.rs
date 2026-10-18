//! User-defined JSX components should use Pascal case
use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_upper(c: char) -> bool {
    c.to_uppercase().eq(std::iter::once(c)) && !c.to_lowercase().eq(std::iter::once(c))
}

fn is_lower(c: char) -> bool {
    c.to_lowercase().eq(std::iter::once(c)) && !c.to_uppercase().eq(std::iter::once(c))
}

/// `testPascalCase` of `jsx-pascal-case`.
fn is_pascal_case(name: &str) -> bool {
    let mut chars = name.chars();
    if !chars.next().is_some_and(is_upper) {
        return false;
    }
    let rest: Vec<char> = chars.collect();
    let non_alnum = rest
        .iter()
        .any(|&c| !is_upper(c) && !is_lower(c) && !c.is_ascii_digit());
    !non_alnum && rest.iter().any(|&c| is_lower(c) || c.is_ascii_digit())
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXOpeningElement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::JSXOpeningElement(el) = node.kind() else {
            return;
        };
        if is_dom_component(ctx, el) {
            return;
        }
        let name = jsx_name_text(ctx, &el.name);
        let parts: Vec<&str> = if name.contains(':') {
            name.split(':').collect()
        } else {
            name.split('.').collect()
        };
        for part in parts {
            if part.chars().count() == 1 {
                return;
            }
            if !is_pascal_case(part) {
                ctx.report(
                    el.span,
                    format!("Imported JSX component {part} must be in PascalCase"),
                );
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn pascal_case() {
        check(
            "S6770",
            "a.jsx",
            r#"
const a = <TestComponent />;
const b = <Test_component />; // Noncompliant {{Imported JSX component Test_component must be in PascalCase}}
const c = <TESTCOMPONENT />; // Noncompliant
const d = <div />;
const e = <Foo.Bar />;
const f = <Foo.bar />; // Noncompliant {{Imported JSX component bar must be in PascalCase}}
const g = <X />;
const h = <H1 />;
const i = <_Foo />; // Noncompliant
const j = <motion.div />;
"#,
        );
    }
}
