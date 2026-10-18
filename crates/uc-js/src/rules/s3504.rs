//! Variables should be declared with "let" or "const"
//!
//! ESLint `no-var`, skipping ambient declarations, vendored files and files where `var` is the
//! dominant declaration style (at least 3 `var`s making up more than half of the declarations).
use crate::helpers::batch_smell2::is_vendor_file;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MIN_VAR_DECLARATIONS: usize = 3;

fn is_ambient(ctx: &Ctx<'_, '_>, node: &AstNode<'_>, vd: &VariableDeclaration<'_>) -> bool {
    vd.declare
        || ctx.is_ts()
            && ctx.nodes().ancestors(node.id()).any(|a| match a.kind() {
                AstKind::TSNamespaceDeclaration(m) => m.declare,
                AstKind::TSExternalModuleDeclaration(m) => m.declare,
                AstKind::TSGlobalDeclaration(_) => true,
                _ => false,
            })
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let lower = ctx.file.path.to_ascii_lowercase();
        if is_vendor_file(ctx.file.path) || lower.ends_with(".html") || lower.ends_with(".htm") {
            return;
        }
        let mut total = 0usize;
        let mut vars: Vec<Span> = Vec::new();
        for node in ctx.nodes().iter() {
            let AstKind::VariableDeclaration(vd) = node.kind() else {
                continue;
            };
            if is_ambient(ctx, node, vd) {
                continue;
            }
            total += 1;
            if vd.kind == VariableDeclarationKind::Var
                && let Some(first) = vd.declarations.first()
            {
                vars.push(Span::new(vd.span.start, first.id.span().end));
            }
        }
        if vars.len() >= MIN_VAR_DECLARATIONS && vars.len() * 2 > total {
            return;
        }
        for span in vars {
            ctx.report(span, "Replace \"var\" with \"let\" or \"const\", which are scoped to the block that declares them.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn no_var() {
        check(
            "S3504",
            "a.ts",
            r#"
declare var $: any;
function bar() {
  var foo = 42; // Noncompliant {{Replace "var" with "let" or "const", which are scoped to the block that declares them.}}
  var x, y = 1; // Noncompliant
  let a = 1;
  const b = 2;
  const c = 3;
  let d = 4;
}
"#,
        );
    }

    #[test]
    fn var_heavy_file() {
        check(
            "S3504",
            "a.js",
            r#"
var a = 1;
var b = 2;
var c = 3;
let d = 4;
"#,
        );
    }

    #[test]
    fn vendor_file() {
        check("S3504", "vendor/lib.js", "var a = 1;\n");
        check("S3504", "js/jquery-3.7.1.min.js", "var a = 1;\n");
    }
}
