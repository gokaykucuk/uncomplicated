//! Exceptions should not be ignored
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_single_simple_statement(body: &[Statement<'_>]) -> bool {
    let [first] = body else { return false };
    let mut s = first;
    while let Statement::LabeledStatement(l) = s {
        s = &l.body;
    }
    !matches!(
        s,
        Statement::BlockStatement(_)
            | Statement::ForStatement(_)
            | Statement::ForInStatement(_)
            | Statement::ForOfStatement(_)
            | Statement::WhileStatement(_)
            | Statement::DoWhileStatement(_)
            | Statement::SwitchStatement(_)
    )
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CatchClause]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CatchClause(c) = node.kind() else {
            return;
        };
        let Some(param) = &c.param else { return };
        let BindingPattern::BindingIdentifier(id) = &param.pattern else {
            return;
        };
        let Some(sym) = id.symbol_id.get() else {
            return;
        };
        if !ctx.scoping().get_resolved_reference_ids(sym).is_empty() {
            return;
        }
        // An empty handler documented by a comment is fine.
        if c.body.body.is_empty()
            && ctx.comments().iter().any(|cm| {
                cm.span.start >= c.body.span.start
                    && cm.span.end <= c.body.span.end
                    && !ctx.comment_text(cm).trim().is_empty()
            })
        {
            return;
        }
        let AstKind::TryStatement(t) = ctx.parent_kind(node.id()) else {
            return;
        };
        if is_single_simple_statement(&t.block.body) {
            return;
        }
        ctx.report(c.span, "Handle this exception, don't catch it at all, or explain in a comment why it is ignored.");
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn ignored_exceptions() {
        check(
            "S2486",
            "a.js",
            r#"
try { a(); b(); } catch (e) {} // Noncompliant {{Handle this exception, don't catch it at all, or explain in a comment why it is ignored.}}
try { a(); b(); } catch (e) { console.log(e); }
try { a(); b(); } catch (e) { /* expected */ }
try { a(); } catch (e) {}
try { a(); b(); } catch {}
try {
  a(); b();
} catch (err) { // Noncompliant
  cleanup();
}
try { for (;;) {} } catch (e) {} // Noncompliant
"#,
        );
    }
}
