//! Regular expressions should not be too complicated
use oxc_regular_expression::ast::{CharacterClass, CharacterClassContents, Disjunction, Term};

use crate::helpers::batch_regex::{
    const_init, is_string_regex_method_call, regexp_callee, with_expr_regex,
};
use crate::prelude::*;

pub struct Rule {
    threshold: u32,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            threshold: p.u32("threshold", 20),
        }
    }
}

/// Collects the regex parts of a node: each entry is a list of literals parsed separately whose
/// complexities add up (string concatenations).
fn find_parts<'a>(
    ctx: &Ctx<'a, '_>,
    e: &'a Expression<'a>,
    depth: u8,
    parts: &mut Vec<Vec<&'a Expression<'a>>>,
) {
    if depth > 4 {
        return;
    }
    match strip_parens(e) {
        Expression::RegExpLiteral(_) | Expression::StringLiteral(_) => parts.push(vec![e]),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() && t.quasis.len() == 1 => {
            parts.push(vec![e])
        }
        Expression::Identifier(id) => {
            if let Some(init) = const_init(ctx, id) {
                find_parts(ctx, init, depth + 1, parts);
            }
        }
        Expression::BinaryExpression(b) if b.operator == BinaryOperator::Addition => {
            let mut literals = Vec::new();
            concat_parts(ctx, &b.left, depth, &mut literals, parts);
            concat_parts(ctx, &b.right, depth, &mut literals, parts);
            if !literals.is_empty() {
                parts.push(literals);
            }
        }
        _ => {}
    }
}

fn concat_parts<'a>(
    ctx: &Ctx<'a, '_>,
    e: &'a Expression<'a>,
    depth: u8,
    literals: &mut Vec<&'a Expression<'a>>,
    parts: &mut Vec<Vec<&'a Expression<'a>>>,
) {
    match strip_parens(e) {
        Expression::StringLiteral(_) => literals.push(e),
        Expression::BinaryExpression(b) if b.operator == BinaryOperator::Addition => {
            concat_parts(ctx, &b.left, depth, literals, parts);
            concat_parts(ctx, &b.right, depth, literals, parts);
        }
        _ => find_parts(ctx, e, depth + 1, parts),
    }
}

struct Calc {
    nesting: u32,
    complexity: u32,
}

impl Calc {
    fn disjunction(&mut self, d: &Disjunction<'_>) {
        let alts = d.body.len() as u32;
        if alts > 1 {
            self.complexity += self.nesting + (alts - 2);
            self.nesting += 1;
        }
        for alt in &d.body {
            for t in &alt.body {
                self.term(t);
            }
        }
        if alts > 1 {
            self.nesting -= 1;
        }
    }

    fn term(&mut self, t: &Term<'_>) {
        match t {
            Term::LookAroundAssertion(l) => {
                self.complexity += self.nesting;
                self.nesting += 1;
                self.disjunction(&l.body);
                self.nesting -= 1;
            }
            Term::IndexedReference(_) | Term::NamedReference(_) => self.complexity += 1,
            Term::CapturingGroup(g) => self.disjunction(&g.body),
            Term::IgnoreGroup(g) => self.disjunction(&g.body),
            Term::CharacterClass(c) => self.class(c),
            Term::Quantifier(q) => {
                self.complexity += self.nesting;
                self.nesting += 1;
                self.term(&q.body);
                self.nesting -= 1;
            }
            _ => {}
        }
    }

    fn class(&mut self, c: &CharacterClass<'_>) {
        self.complexity += 1;
        self.nesting += 1;
        for e in &c.body {
            if let CharacterClassContents::NestedCharacterClass(n) = e {
                self.class(n);
            }
        }
        self.nesting -= 1;
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::RegExpLiteral,
            AstType::NewExpression,
            AstType::CallExpression,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let mut parts = Vec::new();
        match node.kind() {
            AstKind::RegExpLiteral(l) => {
                if let Some(p) = l.regex.pattern.pattern.as_deref() {
                    let mut c = Calc {
                        nesting: 1,
                        complexity: 0,
                    };
                    c.disjunction(&p.body);
                    self.check(ctx, l.span, c.complexity);
                }
                return;
            }
            AstKind::NewExpression(n) if regexp_callee(ctx, &n.callee, true) => {
                if let Some(arg) = n.arguments.first().and_then(Argument::as_expression) {
                    find_parts(ctx, arg, 0, &mut parts);
                }
            }
            AstKind::CallExpression(c)
                if regexp_callee(ctx, &c.callee, false) || is_string_regex_method_call(ctx, c) =>
            {
                if let Some(arg) = c.arguments.first().and_then(Argument::as_expression) {
                    find_parts(ctx, arg, 0, &mut parts);
                }
            }
            _ => return,
        }
        for list in parts {
            let Some(first) = list.first() else { continue };
            let mut complexity = 0;
            for part in &list {
                // Regex literals reached through constants are counted where they are declared.
                with_expr_regex(ctx, part, |re| {
                    let mut c = Calc {
                        nesting: 1,
                        complexity: 0,
                    };
                    c.disjunction(&re.pattern.body);
                    complexity += c.complexity;
                });
            }
            self.check(ctx, first.span(), complexity);
        }
    }
}

impl Rule {
    fn check(&self, ctx: &Ctx<'_, '_>, span: Span, complexity: u32) {
        if complexity > self.threshold {
            ctx.report_gap(
                span,
                format!(
                    "Simplify this regular expression to reduce its complexity from {complexity} to the {} allowed.",
                    self.threshold
                ),
                (complexity - self.threshold) as f32,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn complexity() {
        check_with(
            "S5843",
            "a.js",
            serde_json::json!({"threshold": 3}),
            r#"
/(a|b|c)*d/;                // Noncompliant {{Simplify this regular expression to reduce its complexity from 4 to the 3 allowed.}}
/[a-z]+[0-9]+/;             // Noncompliant {{Simplify this regular expression to reduce its complexity from 4 to the 3 allowed.}}
new RegExp("(?=a|b)c*");    // Noncompliant
const p = "[a]" + "[b]" + "[c]" + "[d]"; // Noncompliant
new RegExp(p);
/abc/;
/a|b|c/;
/[a]x*/;
"#,
        );
    }

    #[test]
    fn default_threshold() {
        check(
            "S5843",
            "a.js",
            r#"
/^(?:(?:[a-z0-9!#$%&'*+/=?^_`{|}~-]+(?:\.[a-z0-9!#$%&'*+/=?^_`{|}~-]+)*)|(?:"(?:[\x01-\x08\x0b\x0c\x0e-\x1f\x21\x23-\x5b\x5d-\x7f]|\\[\x01-\x09\x0b\x0c\x0e-\x7f])*"))@(?:(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.)+[a-z0-9](?:[a-z0-9-]*[a-z0-9])?)$/; // Noncompliant
/^[a-z]+$/;
"#,
        );
    }
}
