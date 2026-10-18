//! Tests should not execute any code after "done()" is called
use crate::helpers::batch_tests::test_case_callback;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let Some(callback) = test_case_callback(call) else {
            return;
        };
        let Some(done) = callback.first_ident_param() else {
            return;
        };
        let Some(sym) = done.symbol_id.get() else {
            return;
        };
        let mut refs: Vec<NodeId> = ctx
            .scoping()
            .get_resolved_references(sym)
            .map(|r| r.node_id())
            .collect();
        refs.sort_unstable();
        for r in refs {
            let ref_node = ctx.node(r);
            let parent = ctx.parent(r);
            let AstKind::CallExpression(done_call) = parent.kind() else {
                continue;
            };
            if done_call.callee.span() != ref_node.kind().span() {
                continue;
            }
            let stmt = ctx.parent(parent.id());
            if !matches!(stmt.kind(), AstKind::ExpressionStatement(_)) {
                continue;
            }
            if let Some(next) = next_statement(ctx, stmt.id()) {
                ctx.report_with(
                    next,
                    "Move this code before the call to \"done\".",
                    [(done_call.span, None)],
                );
                return;
            }
        }
    }
}

/// First expression statement that runs right after the statement `id`, staying in the same
/// function. Stops at jumps and at compound statements (their flow is not modelled).
fn next_statement(ctx: &Ctx<'_, '_>, mut id: NodeId) -> Option<Span> {
    loop {
        let container = ctx.parent(id);
        let stmts: &[Statement<'_>] = match container.kind() {
            AstKind::BlockStatement(b) => &b.body,
            AstKind::FunctionBody(b) => &b.statements,
            _ => return None,
        };
        let span = ctx.node(id).kind().span();
        let pos = stmts.iter().position(|s| s.span() == span)?;
        for s in &stmts[pos + 1..] {
            match s {
                Statement::ExpressionStatement(e) => return Some(e.span),
                Statement::VariableDeclaration(_)
                | Statement::EmptyStatement(_)
                | Statement::FunctionDeclaration(_) => {}
                _ => return None,
            }
        }
        // fell off the end of the block: continue after the enclosing statement
        if !matches!(container.kind(), AstKind::BlockStatement(_)) {
            return None;
        }
        let outer = ctx.parent(container.id());
        id = match outer.kind() {
            AstKind::IfStatement(_) => outer.id(),
            AstKind::BlockStatement(_) | AstKind::FunctionBody(_) => container.id(),
            AstKind::CatchClause(_) => {
                let try_stmt = ctx.parent(outer.id());
                match try_stmt.kind() {
                    AstKind::TryStatement(t) if t.finalizer.is_none() => try_stmt.id(),
                    _ => return None,
                }
            }
            AstKind::TryStatement(t)
                if t.finalizer.is_none() && t.block.span == container.kind().span() =>
            {
                outer.id()
            }
            _ => return None,
        };
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn code_after_done() {
        check(
            "S6079",
            "a.test.js",
            r#"
describe("Code is executed after Done", function() {
    it("Has asserts after done()", function(done) {
        try {
            expect(1).toEqual(2);
        } catch (err) {
            done();
            expect(err).to.be.an.instanceof(RangeError);  // Noncompliant {{Move this code before the call to "done".}}
        }
    });
    it("Throws an error some time after done()", function(done) {
        fs.readFile("/etc/bashrc", 'utf8', function(err, data) {
            done();
            setTimeout(() => {  // Noncompliant
                expect(data).to.match(/some expected string/);
            }, 3000);
        });
    });
    it("Other test", function(done) {
        done()
    });
    it("Has code after done(err)", function(done) {
        try {
            throw Error("An error");
        } catch (err) {
            done(err);
        }
        fs.readFile("/etc/bashrc", 'utf8', function(err, data) { }); // Noncompliant
    });
    it("'done' inside 'if'", function(done) {
        if (condition) {
            done(err);
        }
        fs.readFile("/etc/bashrc", 'utf8', function(err, data) { }); // Noncompliant
    });
    it ("'done' is inside another function", function(done) {
        function foo(){
            done();
            bar(); // Noncompliant
        }
        foo();
    });
    it ("'done' on top level", function(done) {
        done();
        bar(); // Noncompliant
    });
    it("Ok with try/catch", function(done) {
        try {
            throw Error("An error");
        } catch (err) {
            done(err);
        }
    });
    it("returns after done", function(done) {
        if (x) {
            done();
            return;
        }
        bar();
    });
    it("No done", function() {
        done();
        bar();
    });
    it("parameter is not identifier", function({done}) {
        done();
        bar();
    });
});
"#,
        );
    }
}
