//! All code should be reachable
use crate::helpers::batch_security1::{FlowOpts, list_flow};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Reports only code that is certainly unreachable.
const OPTS: FlowOpts = FlowOpts {
    optimistic: true,
    process_exit: false,
};

/// Statements ESLint's `no-unreachable` reports (hoisted declarations are fine).
fn is_reportable(s: &Statement<'_>) -> bool {
    match s {
        Statement::FunctionDeclaration(_) | Statement::EmptyStatement(_) => false,
        Statement::VariableDeclaration(d) => {
            d.kind != VariableDeclarationKind::Var
                || d.declarations.iter().any(|v| v.init.is_some())
        }
        Statement::TSTypeAliasDeclaration(_)
        | Statement::TSInterfaceDeclaration(_)
        | Statement::TSEnumDeclaration(_)
        | Statement::TSExternalModuleDeclaration(_)
        | Statement::TSNamespaceDeclaration(_)
        | Statement::TSGlobalDeclaration(_)
        | Statement::TSImportEqualsDeclaration(_)
        | Statement::TSExportAssignment(_)
        | Statement::TSNamespaceExportDeclaration(_) => false,
        Statement::ClassDeclaration(c) => !c.declare,
        _ => true,
    }
}

fn check(ctx: &Ctx<'_, '_>, stmts: &[Statement<'_>]) {
    let mut unreachable = Vec::new();
    list_flow(stmts, OPTS, &mut Some(&mut unreachable));
    for slice in unreachable {
        let mut range: Option<Span> = None;
        for s in slice {
            if is_reportable(s) {
                let sp = s.span();
                range = Some(range.map_or(sp, |r| Span::new(r.start, sp.end)));
            } else if let Some(r) = range.take() {
                ctx.report(r, "Unreachable code.");
            }
        }
        if let Some(r) = range {
            ctx.report(r, "Unreachable code.");
        }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::Program,
            AstType::FunctionBody,
            AstType::StaticBlock,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::Program(p) => check(ctx, &p.body),
            AstKind::FunctionBody(b) => check(ctx, &b.statements),
            AstKind::StaticBlock(b) => check(ctx, &b.body),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn unreachable() {
        check(
            "S1763",
            "a.js",
            r#"
function f(x) {
  return 1;
  foo(); // Noncompliant {{Unreachable code.}}
  bar();
}
function g(x) {
  if (x) { return 1; } else { throw new Error(); }
  foo(); // Noncompliant
}
function h(x) {
  while (true) { foo(); }
  bar(); // Noncompliant
}
function i(x) {
  while (true) { if (x) break; }
  bar();
  for (;;) {}
  baz(); // Noncompliant
}
function j(x) {
  return;
  function hoisted() {}
  var y;
}
function k(x) {
  switch (x) {
    case 1:
      break;
      foo(); // Noncompliant
    default:
      bar();
  }
  try { return 1; } catch (e) { log(e); }
  after();
  try { return 1; } finally { cleanup(); }
  after(); // Noncompliant
}
function l() {
  throw new Error();
  if (a) { // Noncompliant
    b();
    return;
    c();
  }
}
const m = () => { return 1; x(); }; // Noncompliant
outer: for (;;) { for (;;) { break outer; } }
reachable();
"#,
        );
    }
}
