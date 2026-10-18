//! "for...in" loops should filter properties before acting on them
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// `a[k] = ...`: copying attributes is a legitimate unfiltered use.
fn is_attr_copy(s: &Statement<'_>) -> bool {
    let Statement::ExpressionStatement(e) = s else {
        return false;
    };
    matches!(
        strip_parens(&e.expression),
        Expression::AssignmentExpression(a) if matches!(a.left, AssignmentTarget::ComputedMemberExpression(_))
    )
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ForInStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::ForInStatement(f) = node.kind() else {
            return;
        };
        match &f.body {
            Statement::BlockStatement(b) => match b.body.first() {
                None => return,
                Some(first)
                    if matches!(first, Statement::IfStatement(_)) || is_attr_copy(first) =>
                {
                    return;
                }
                _ => {}
            },
            Statement::EmptyStatement(_) | Statement::IfStatement(_) => return,
            s if is_attr_copy(s) => return,
            _ => {}
        }
        ctx.report(
            f.span,
            "Restrict what this loop acts on by testing each property.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn for_in() {
        check(
            "S1535",
            "a.js",
            r#"
for (name in object) { // Noncompliant {{Restrict what this loop acts on by testing each property.}}
  doSomething(name);
}
for (name in object) doSomething(name); // Noncompliant
for (name in object) {
  if (object.hasOwnProperty(name)) { doSomething(name); }
}
for (name in object) {}
for (name in object);
for (name in object) if (x) foo();
for (key in src) { dst[key] = src[key]; }
for (key in src) dst[key] = src[key];
for (const x of xs) { foo(x); }
"#,
        );
    }
}
