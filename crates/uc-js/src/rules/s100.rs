//! Function and method names should comply with a naming convention
use crate::helpers::batch_smell1::{js_regex, parent_no_parens, returns_jsx};
use crate::prelude::*;

const DEFAULT_FORMAT: &str = "^[_a-z][a-zA-Z0-9]*$";

pub struct Rule {
    format: String,
    re: Option<regex::Regex>,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        let format = p.str("format", DEFAULT_FORMAT).to_string();
        let re = js_regex(&format, "");
        Self { format, re }
    }
}

/// Name identifier of a key: plain identifiers, including computed `[name]` keys.
fn key_ident<'a>(key: &PropertyKey<'a>) -> Option<(&'a str, Span)> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some((id.name.as_str(), id.span)),
        PropertyKey::Identifier(id) => Some((id.name.as_str(), id.span)),
        _ => None,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Function, AstType::ArrowFunctionExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let Some(re) = &self.re else { return };
        let kind = node.kind();
        let (span, is_decl) = match kind {
            AstKind::Function(f) => {
                if f.body.is_none() {
                    return;
                }
                (f.span, f.is_function_declaration())
            }
            AstKind::ArrowFunctionExpression(a) => (a.span, false),
            _ => return,
        };
        let name: Option<(&str, Span)> = if is_decl {
            let AstKind::Function(f) = kind else { return };
            f.id.as_ref().map(|id| (id.name.as_str(), id.span))
        } else {
            let parent = parent_no_parens(ctx, node.id());
            match parent.kind() {
                AstKind::ObjectProperty(p) if strip_parens(&p.value).span() == span => {
                    key_ident(&p.key)
                }
                AstKind::VariableDeclarator(v) => match (&v.id, &v.init) {
                    (BindingPattern::BindingIdentifier(id), Some(init))
                        if strip_parens(init).span() == span =>
                    {
                        Some((id.name.as_str(), id.span))
                    }
                    _ => None,
                },
                AstKind::MethodDefinition(m) if matches!(kind, AstKind::Function(_)) => {
                    key_ident(&m.key)
                }
                _ => None,
            }
        };
        let Some((name, name_span)) = name else {
            return;
        };
        if re.is_match(name) || returns_jsx(&kind) {
            return;
        }
        ctx.report(
            name_span,
            format!(
                "Rename this '{name}' function to match the regular expression '{}'.",
                self.format
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn function_names() {
        check(
            "S100",
            "a.jsx",
            r#"
function doSomething() {}
function DoSomething() {}   // Noncompliant {{Rename this 'DoSomething' function to match the regular expression '^[_a-z][a-zA-Z0-9]*$'.}}
function do_something() {}  // Noncompliant
const f_g = function () {}; // Noncompliant
const h_i = () => 1;        // Noncompliant
const Comp = () => <div/>;
const Comp2 = function () { return (<div/>); };
function Comp3() { if (x) { return <div/>; } return null; }
function Not_Comp() { return () => <div/>; } // Noncompliant
const o = {
  Foo() {},                 // Noncompliant
  bar: function () {},
  Baz: () => {},            // Noncompliant
  'Quoted': () => {},
};
class A {
  Method() {}               // Noncompliant
  method() {}
  constructor() {}
  #Priv() {}
}
const x = function Inner_Name() {};
"#,
        );
    }

    #[test]
    fn custom_format() {
        check_with(
            "S100",
            "a.ts",
            serde_json::json!({"format": "^[A-Z]"}),
            r#"
function foo() {} // Noncompliant
function Foo() {}
declare function bar(): void;
"#,
        );
    }
}
