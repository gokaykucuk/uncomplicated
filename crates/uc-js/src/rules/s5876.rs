//! A new session should be created during user authentication
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Some `x.regenerate(...)` call inside `span`.
fn has_regenerate(ctx: &Ctx<'_, '_>, span: Span) -> bool {
    let toks = ctx.tokens_in(span);
    toks.iter().enumerate().any(|(i, t)| {
        ctx.text(t.span()) == "regenerate"
            && i > 0
            && matches!(toks[i - 1].kind(), Kind::Dot | Kind::QuestionDot)
            && toks.get(i + 1).is_some_and(|n| n.kind() == Kind::LParen)
    })
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if !src_contains(ctx, "passport") {
            return;
        }
        for node in ctx.nodes().iter() {
            let AstKind::CallExpression(call) = node.kind() else {
                continue;
            };
            if fqn_call(ctx, call).as_deref() != Some("passport.authenticate") {
                continue;
            }
            if let Some(Expression::ObjectExpression(o)) = arg(&call.arguments, 1).map(strip)
                && property_with_value(ctx, o, "session", Lit::Bool(false)).is_some()
            {
                continue;
            }
            let AstKind::CallExpression(parent) = ctx.parent_kind(node.id()) else {
                continue;
            };
            let Some(cb) = arg(&parent.arguments, 2) else {
                continue;
            };
            let Expression::FunctionExpression(f) = value_of(ctx, cb) else {
                continue;
            };
            if !has_regenerate(ctx, f.span) {
                ctx.report(
                    f.span,
                    "Create a new session during user authentication to prevent session fixation attacks.",
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S5876",
            "a.js",
            r#"
const passport = require('passport');
app.post('/login', passport.authenticate('local', { failureRedirect: '/login' }), function(req, res) { // Noncompliant {{Create a new session during user authentication to prevent session fixation attacks.}}
  res.redirect('/');
});
app.post('/login', passport.authenticate('local'), function(req, res) {
  req.session.regenerate(function(err) { res.redirect('/'); });
});
app.post('/api', passport.authenticate('bearer', { session: false }), function(req, res) {
  res.json(req.user);
});
app.post('/login', passport.authenticate('local'), (req, res) => res.redirect('/'));
"#,
        );
    }
}
