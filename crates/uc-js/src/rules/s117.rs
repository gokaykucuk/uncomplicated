//! Variable, property and parameter names should comply with a naming convention
use crate::helpers::batch_smell1::js_regex;
use crate::prelude::*;

const DEFAULT_FORMAT: &str = "^[_$A-Za-z][$A-Za-z0-9]*$|^[_$A-Z][_$A-Z0-9]+$";

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

/// Identifiers bound by a pattern, skipping shorthand object properties (like Sonar's
/// `resolveIdentifiers`).
fn collect<'b, 'a>(p: &'b BindingPattern<'a>, out: &mut Vec<&'b BindingIdentifier<'a>>) {
    match p {
        BindingPattern::BindingIdentifier(id) => out.push(id),
        BindingPattern::ObjectPattern(o) => {
            for prop in &o.properties {
                if !prop.shorthand {
                    collect(&prop.value, out);
                }
            }
            if let Some(r) = &o.rest {
                collect(&r.argument, out);
            }
        }
        BindingPattern::ArrayPattern(a) => {
            for e in a.elements.iter().flatten() {
                collect(e, out);
            }
            if let Some(r) = &a.rest {
                collect(&r.argument, out);
            }
        }
        BindingPattern::AssignmentPattern(a) => collect(&a.left, out),
    }
}

impl Rule {
    fn check(&self, ctx: &Ctx<'_, '_>, re: &regex::Regex, name: &str, span: Span, what: &str) {
        if !re.is_match(name) {
            ctx.report(
                span,
                format!(
                    "Rename this {what} \"{name}\" to match the regular expression {}.",
                    self.format
                ),
            );
        }
    }

    fn check_params(&self, ctx: &Ctx<'_, '_>, re: &regex::Regex, params: &FormalParameters<'_>) {
        let mut ids = Vec::new();
        for p in &params.items {
            collect(&p.pattern, &mut ids);
        }
        if let Some(r) = &params.rest {
            collect(&r.rest.argument, &mut ids);
        }
        for id in ids {
            self.check(ctx, re, &id.name, id.span, "parameter");
        }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::VariableDeclaration,
            AstType::Function,
            AstType::ArrowFunctionExpression,
            AstType::TSMethodSignature,
            AstType::TSConstructSignatureDeclaration,
            AstType::PropertyDefinition,
            AstType::CatchClause,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let Some(re) = &self.re else { return };
        match node.kind() {
            AstKind::VariableDeclaration(d) => {
                if d.declare {
                    return;
                }
                let mut ids = Vec::new();
                for decl in &d.declarations {
                    collect(&decl.id, &mut ids);
                }
                for id in ids {
                    self.check(ctx, re, &id.name, id.span, "local variable");
                }
            }
            AstKind::Function(f) if !f.declare => self.check_params(ctx, re, &f.params),
            AstKind::ArrowFunctionExpression(a) => self.check_params(ctx, re, &a.params),
            AstKind::TSMethodSignature(m) => self.check_params(ctx, re, &m.params),
            AstKind::TSConstructSignatureDeclaration(c) => self.check_params(ctx, re, &c.params),
            AstKind::PropertyDefinition(p) => {
                if let PropertyKey::StaticIdentifier(id) = &p.key {
                    self.check(ctx, re, &id.name, id.span, "property");
                }
            }
            AstKind::CatchClause(c) => {
                if let Some(param) = &c.param {
                    let mut ids = Vec::new();
                    collect(&param.pattern, &mut ids);
                    for id in ids {
                        self.check(ctx, re, &id.name, id.span, "parameter");
                    }
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
    fn names() {
        check(
            "S117",
            "a.ts",
            r#"
const camelCase = 1, UPPER_CASE = 2;
let snake_case = 3;          // Noncompliant {{Rename this local variable "snake_case" to match the regular expression ^[_$A-Za-z][$A-Za-z0-9]*$|^[_$A-Z][_$A-Z0-9]+$.}}
const { a_b } = obj;
const { x: c_d, ...r_e } = obj; // Noncompliant 2
function f(p_q, ok, [a_r]) {}  // Noncompliant 2
const g = (u_v = 1) => {};   // Noncompliant {{Rename this parameter "u_v" to match the regular expression ^[_$A-Za-z][$A-Za-z0-9]*$|^[_$A-Z][_$A-Z0-9]+$.}}
class C {
  my_prop = 1;               // Noncompliant {{Rename this property "my_prop" to match the regular expression ^[_$A-Za-z][$A-Za-z0-9]*$|^[_$A-Z][_$A-Z0-9]+$.}}
  #priv_x = 1;
  constructor(private c_p: number) {} // Noncompliant
}
try {} catch (e_x) {}        // Noncompliant
declare const d_x: number;
declare function df(p_x): void;
interface I { m(i_p: number): void; } // Noncompliant
type FT = (t_p: number) => void;
"#,
        );
    }
}
