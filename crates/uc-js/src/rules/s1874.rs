//! Deprecated APIs should not be used
//!
//! Sonar relays the TypeScript checker's deprecation suggestions, which cover every declaration
//! (including libraries) and print call signatures. Without a type checker we report references
//! to variables and functions declared in the same file with a `/** @deprecated */` JSDoc tag.
//! Calls of deprecated functions are left out: TypeScript reports those with the printed
//! signature ("The signature '(…)' of 'f' is deprecated."), which we cannot reproduce.
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_function_like(e: &Expression<'_>) -> bool {
    matches!(
        strip_parens(e),
        Expression::FunctionExpression(_)
            | Expression::ArrowFunctionExpression(_)
            | Expression::ClassExpression(_)
    )
}

/// Deprecated symbols declared by a statement, with whether they are functions.
fn declared<'a>(stmt: AstKind<'a>, out: &mut Vec<(SymbolId, bool)>) {
    match stmt {
        AstKind::VariableDeclaration(d) => {
            for v in &d.declarations {
                if let BindingPattern::BindingIdentifier(b) = &v.id
                    && let Some(sym) = b.symbol_id.get()
                {
                    let func = v.init.as_ref().is_some_and(is_function_like);
                    if !func {
                        out.push((sym, false));
                    }
                }
            }
        }
        AstKind::Function(f) if f.is_declaration() => {
            if let Some(sym) = f.id.as_ref().and_then(|id| id.symbol_id.get()) {
                out.push((sym, true));
            }
        }
        AstKind::ExportDeclaration(e) => match &e.declaration {
            Declaration::VariableDeclaration(d) => declared(AstKind::VariableDeclaration(d), out),
            Declaration::FunctionDeclaration(f) => declared(AstKind::Function(f), out),
            _ => {}
        },
        _ => {}
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let mut starts: FxHashSet<u32> = FxHashSet::default();
        for c in ctx.comments() {
            if !c.is_block() {
                continue;
            }
            let text = ctx.comment_text(c);
            if text.starts_with('*')
                && text.contains("@deprecated")
                && let Some(next) = token_after(ctx, c.span.end)
            {
                starts.insert(next.start);
            }
        }
        if starts.is_empty() {
            return;
        }
        let mut symbols = Vec::new();
        for node in ctx.nodes().iter() {
            let kind = node.kind();
            if !matches!(
                kind,
                AstKind::VariableDeclaration(_)
                    | AstKind::Function(_)
                    | AstKind::ExportDeclaration(_)
            ) {
                continue;
            }
            if starts.contains(&kind.span().start) {
                declared(kind, &mut symbols);
            }
        }
        for (sym, is_function) in symbols {
            let name = ctx.scoping().symbol_name(sym);
            for r in ctx.scoping().get_resolved_references(sym) {
                let span = ctx.node(r.node_id()).kind().span();
                let parent = ctx.parent(r.node_id());
                let skip = match parent.kind() {
                    AstKind::ExportSpecifier(_) => true,
                    AstKind::CallExpression(c) => is_function && c.callee.span() == span,
                    AstKind::NewExpression(n) => is_function && n.callee.span() == span,
                    _ => false,
                };
                if !skip {
                    ctx.report(span, format!("'{name}' is deprecated."));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn deprecation() {
        check(
            "S1874",
            "a.ts",
            r#"
/** @deprecated use NEW_LIMIT */
const OLD_LIMIT = 10;
/**
 * Old helper.
 * @deprecated
 */
export function oldHelper() { return 1; }
// @deprecated not a JSDoc
const plain = 1;
const NEW_LIMIT = 20;
use(OLD_LIMIT); // Noncompliant {{'OLD_LIMIT' is deprecated.}}
const fn = oldHelper; // Noncompliant {{'oldHelper' is deprecated.}}
oldHelper();
use(plain, NEW_LIMIT);
"#,
        );
    }
}
