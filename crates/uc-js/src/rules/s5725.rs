//! Remote artifacts should not be used without integrity checks
use regex::Regex;

use crate::prelude::*;

pub struct Rule {
    semver: Regex,
    alias: Regex,
}

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self {
            semver: Regex::new(r"/v?\d+\.\d+(?:\.\d+)?(?:-[a-zA-Z0-9][a-zA-Z0-9.-]*)?(?:[/?#]|$)")
                .unwrap(),
            alias: Regex::new(r"/[^/@]*@[\d.]+(?:-[a-zA-Z0-9][a-zA-Z0-9.-]*)?(?:[/?#]|$)").unwrap(),
        }
    }

    fn is_versioned_remote(&self, url: &str) -> bool {
        (url.starts_with("http") || url.starts_with("//"))
            && (self.semver.is_match(url) || self.alias.is_match(url))
    }
}

/// `document.createElement('script')` (the TS type is then `HTMLScriptElement`).
fn is_script_creation<'a>(ctx: &Ctx<'a, '_>, d: &'a VariableDeclarator<'a>) -> bool {
    if let Some(t) = &d.type_annotation
        && !matches!(&t.type_annotation, TSType::TSTypeReference(r)
            if matches!(&r.type_name, TSTypeName::IdentifierReference(id) if id.name == "HTMLScriptElement"))
    {
        return false;
    }
    let Some(Expression::CallExpression(c)) = d.init.as_ref().map(|e| strip_ts(e)) else {
        return false;
    };
    let Expression::StaticMemberExpression(m) = &c.callee else {
        return false;
    };
    m.property.name == "createElement"
        && matches!(&m.object, Expression::Identifier(id) if ctx.is_global(id, "document"))
        && matches!(c.arguments.first(), Some(Argument::StringLiteral(s)) if s.value == "script")
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::VariableDeclarator]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::VariableDeclarator(d) = node.kind() else {
            return;
        };
        let BindingPattern::BindingIdentifier(id) = &d.id else {
            return;
        };
        if !is_script_creation(ctx, d) {
            return;
        }
        let Some(sym) = id.symbol_id.get() else {
            return;
        };
        let mut src_assignments = 0;
        let mut versioned = false;
        let mut integrity = false;
        let mut cross_origin_ok = false;
        for r in ctx.scoping().get_resolved_references(sym) {
            let ref_id = r.node_id();
            let parent = ctx.parent(ref_id);
            let AstKind::StaticMemberExpression(m) = parent.kind() else {
                continue;
            };
            let assignment = match ctx.parent_kind(parent.id()) {
                AstKind::AssignmentExpression(a) if a.left.span() == m.span => Some(a),
                _ => None,
            };
            match m.property.name.as_str() {
                "integrity" => integrity = true,
                "src" => {
                    if let Some(a) = assignment {
                        src_assignments += 1;
                        if let Expression::StringLiteral(s) = &a.right
                            && self.is_versioned_remote(&s.value)
                        {
                            versioned = true;
                        }
                    }
                }
                "crossOrigin" => {
                    if let Some(a) = assignment {
                        cross_origin_ok = match &a.right {
                            Expression::StringLiteral(s) => s.value == "anonymous",
                            e => !is_literal(e),
                        };
                    }
                }
                _ => {}
            }
        }
        if src_assignments != 1 || !versioned {
            return;
        }
        let message = match (integrity, cross_origin_ok) {
            (false, false) => {
                "Add integrity and crossorigin=\"anonymous\" attributes to this element to enforce integrity checks."
            }
            (false, true) => {
                "Add an integrity attribute to this element to enforce integrity checks."
            }
            (true, false) => {
                "Add a crossorigin=\"anonymous\" attribute to this element to enforce integrity checks."
            }
            (true, true) => return,
        };
        ctx.report(d.span, message);
    }
}

/// ESTree `Literal` nodes.
fn is_literal(e: &Expression<'_>) -> bool {
    matches!(
        e,
        Expression::StringLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::RegExpLiteral(_)
            | Expression::BigIntLiteral(_)
    )
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S5725",
            "a.js",
            r#"
const script = document.createElement('script'); // Noncompliant {{Add integrity and crossorigin="anonymous" attributes to this element to enforce integrity checks.}}
script.src = 'https://cdn.example.com/lib/1.2.3/lib.min.js';
document.head.appendChild(script);

const s2 = document.createElement('script'); // Noncompliant {{Add an integrity attribute to this element to enforce integrity checks.}}
s2.src = 'https://cdn.jsdelivr.net/npm/lib@4.17.21/lib.min.js';
s2.crossOrigin = 'anonymous';

const s3 = document.createElement('script');
s3.src = 'https://cdn.example.com/lib/1.2.3/lib.min.js';
s3.integrity = 'sha384-abc';
s3.crossOrigin = 'anonymous';

const s4 = document.createElement('script');
s4.src = 'https://cdn.example.com/lib/latest/lib.min.js';

const s5 = document.createElement('script');
s5.src = '/local/1.2.3/lib.js';

const d = document.createElement('div');
d.src = 'https://cdn.example.com/lib/1.2.3/lib.min.js';
"#,
        );
    }
}
