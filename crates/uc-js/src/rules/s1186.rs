//! Functions should not be empty
use crate::helpers::batch_smell1::{function_name_with_kind, parent_no_parens};
use crate::prelude::*;

pub struct Rule {
    allow: Vec<String>,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        let allow = p
            .list(
                "allow",
                &["arrowFunctions", "constructors", "private-constructors"],
            )
            .into_iter()
            .map(|s| {
                s.trim_matches(|c: char| {
                    c == '[' || c == ']' || c == '\'' || c == '"' || c.is_whitespace()
                })
                .to_string()
            })
            .collect();
        Self { allow }
    }

    fn allows(&self, what: &str) -> bool {
        self.allow.iter().any(|a| a == what)
    }
}

/// `onXxx` handlers and `noop`-like names are expected to be empty.
fn is_exceptional(name: &str) -> bool {
    let b = name.as_bytes();
    (b.len() > 2 && b[0] == b'o' && b[1] == b'n' && b[2].is_ascii_uppercase())
        || name.to_ascii_lowercase().contains("noop")
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Function, AstType::ArrowFunctionExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let (body, is_arrow, is_decl, id) = match node.kind() {
            AstKind::Function(f) => match &f.body {
                Some(b) => (&**b, false, f.is_function_declaration(), f.id.as_ref()),
                None => return,
            },
            AstKind::ArrowFunctionExpression(a) => match a.get_function_body() {
                Some(b) => (b, true, false, None),
                None => return,
            },
            _ => return,
        };
        if !body.statements.is_empty()
            || !body.directives.is_empty()
            || ctx.has_comments_in(body.span)
        {
            return;
        }
        let parent = parent_no_parens(ctx, node.id());
        // Sonar only reports function declarations, methods and functions assigned to variables.
        let applicable = if is_decl {
            !id.is_some_and(|i| is_exceptional(&i.name))
        } else {
            match parent.kind() {
                AstKind::MethodDefinition(m) => {
                    !matches!(&m.key, PropertyKey::StaticIdentifier(k) if is_exceptional(&k.name))
                }
                AstKind::VariableDeclarator(v) => {
                    v.init
                        .as_ref()
                        .is_some_and(|i| strip_parens(i).span() == node.kind().span())
                        && !matches!(&v.id, BindingPattern::BindingIdentifier(k) if is_exceptional(&k.name))
                }
                _ => false,
            }
        };
        if !applicable {
            return;
        }
        // typescript-eslint `no-empty-function` options.
        if is_arrow && self.allows("arrowFunctions") {
            return;
        }
        if let AstKind::MethodDefinition(m) = parent.kind() {
            if m.kind == MethodDefinitionKind::Constructor {
                let has_param_props = node_params(node).is_some_and(|ps| {
                    ps.items
                        .iter()
                        .any(|p| p.accessibility.is_some() || p.readonly || p.r#override)
                });
                let allowed = self.allows("constructors")
                    || (m.accessibility == Some(TSAccessibility::Private)
                        && self.allows("private-constructors"))
                    || (m.accessibility == Some(TSAccessibility::Protected)
                        && self.allows("protected-constructors"))
                    || has_param_props;
                if allowed {
                    return;
                }
            }
            if !m.decorators.is_empty() && self.allows("decoratedFunctions") {
                return;
            }
            if m.r#override && self.allows("overrideMethods") {
                return;
            }
            let k = match m.kind {
                MethodDefinitionKind::Get => "getters",
                MethodDefinitionKind::Set => "setters",
                MethodDefinitionKind::Constructor => "constructors",
                MethodDefinitionKind::Method => "methods",
            };
            if k != "constructors" && self.allows(k) {
                return;
            }
        } else {
            let k = if is_arrow {
                "arrowFunctions"
            } else {
                "functions"
            };
            if self.allows(k) {
                return;
            }
        }
        if let AstKind::Function(f) = node.kind() {
            if f.r#async
                && self.allows("asyncFunctions")
                && !matches!(parent.kind(), AstKind::MethodDefinition(_))
            {
                return;
            }
            if f.generator
                && self.allows("generatorFunctions")
                && !matches!(parent.kind(), AstKind::MethodDefinition(_))
            {
                return;
            }
        }
        let name = function_name_with_kind(ctx, node);
        ctx.report(body.span, format!("Unexpected empty {name}."));
    }
}

fn node_params<'a>(node: &AstNode<'a>) -> Option<&'a FormalParameters<'a>> {
    match node.kind() {
        AstKind::Function(f) => Some(&f.params),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn empty_functions() {
        check(
            "S1186",
            "a.ts",
            r#"
function foo() {}                 // Noncompliant {{Unexpected empty function 'foo'.}}
function bar() { /* comment */ }
function onClick() {}
function noopFn() {}
async function af() {}            // Noncompliant {{Unexpected empty async function 'af'.}}
const v = function () {};         // Noncompliant {{Unexpected empty function.}}
const arrow = () => {};
const onChange = function () {};
call(function () {});
class A {
  constructor() {}
  m() {}                          // Noncompliant {{Unexpected empty method 'm'.}}
  static s() {}                   // Noncompliant {{Unexpected empty static method 's'.}}
  get g() {}                      // Noncompliant {{Unexpected empty getter 'g'.}}
  onFocus() {}
  #p() {}                         // Noncompliant {{Unexpected empty private method #p.}}
}
class B { protected constructor() {} }
const o = { m() {} };
"#,
        );
    }
}
