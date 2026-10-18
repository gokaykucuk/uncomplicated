//! Static Assets should not serve session cookies
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

#[derive(Clone, Copy)]
struct State<'a> {
    app: Option<&'a str>,
    last_session: Option<Span>,
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if !src_contains(ctx, "express") || !src_contains(ctx, "session") {
            return;
        }
        let mut st = State {
            app: None,
            last_session: None,
        };
        // (end offset of the function, state to restore on exit)
        let mut stack: Vec<(u32, State<'a>)> = Vec::new();
        let mut args = Vec::new();
        for node in ctx.nodes().iter() {
            let start = node.kind().span().start;
            while let Some(&(end, saved)) = stack.last() {
                if start < end {
                    break;
                }
                st = saved;
                stack.pop();
            }
            match node.kind() {
                AstKind::Function(_) | AstKind::ArrowFunctionExpression(_) => {
                    stack.push((node.kind().span().end, st));
                    if let Some(app) = express_app_injection(ctx, node) {
                        st = State {
                            app: Some(app.name.as_str()),
                            last_session: None,
                        };
                    }
                }
                AstKind::VariableDeclarator(d) => {
                    if let Some(app) = express_app_instantiation(ctx, d) {
                        st = State {
                            app: Some(app.name.as_str()),
                            last_session: None,
                        };
                    }
                }
                AstKind::CallExpression(call) => {
                    let Some(app) = st.app else { continue };
                    if !is_method_invocation(call, app, "use", 1) {
                        continue;
                    }
                    args.clear();
                    flatten_args(ctx, &call.arguments, &mut args);
                    for m in &args {
                        if is_middleware_instance(ctx, m, &["express-session", "cookie-session"]) {
                            st.last_session = Some(call.span);
                            continue;
                        }
                        if let Some(session) = st.last_session
                            && is_middleware_instance(ctx, m, &["express.static"])
                        {
                            ctx.report_with(
                                call.span,
                                "Move this static middleware before the session middleware.",
                                [(
                                    session,
                                    Some("Session middleware declared here.".to_string()),
                                )],
                            );
                        }
                    }
                }
                _ => {}
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
            "S8441",
            "a.ts",
            r#"
import express from 'express';
import session from 'express-session';
const app = express();
app.use(express.static('public-before'));
app.use(session({ secret: 's' }));
app.use(express.static('public')); // Noncompliant {{Move this static middleware before the session middleware.}}
app.use('/x', express.static('x')); // Noncompliant
module.exports = function (app) {
  app.use(express.static('inner'));
};
"#,
        );
    }
}
