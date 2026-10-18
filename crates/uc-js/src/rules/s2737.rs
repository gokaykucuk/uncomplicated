//! "catch" clauses should do more than rethrow
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
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
        let [Statement::ThrowStatement(t)] = c.body.body.as_slice() else {
            return;
        };
        if !ctx.same_tokens(param.pattern.span(), t.argument.span()) {
            return;
        }
        let span = find_token(ctx, c.span, Kind::Catch)
            .unwrap_or(Span::new(c.span.start, c.span.start + 5));
        ctx.report(span, "Add logic to this catch clause or eliminate it and rethrow the exception automatically.");
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn useless_catch() {
        check(
            "S2737",
            "a.js",
            r#"
try {
  doSomething();
} catch (e) { // Noncompliant {{Add logic to this catch clause or eliminate it and rethrow the exception automatically.}}
  throw e;
}
try { a(); } catch (e) { log(e); throw e; }
try { a(); } catch (e) { throw new Error(e); }
try { a(); } catch { throw err; }
try { a(); } catch (e) { throw err; }
"#,
        );
    }
}
