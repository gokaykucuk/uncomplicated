//! Functions should always return the same type
use crate::helpers::batch_smell3::{function_jsdoc, returns_by_function};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Sonar's printed category of a return value type; `None` when we can't tell.
fn category(t: &Ty) -> Option<&'static str> {
    Some(match t {
        Ty::String => "string",
        Ty::Number => "number",
        Ty::Boolean => "boolean",
        Ty::BigInt => "bigint",
        Ty::Symbol => "symbol",
        Ty::Array | Ty::Tuple => "array",
        Ty::Function => "function",
        Ty::Object
        | Ty::Class
        | Ty::Map
        | Ty::Set
        | Ty::Date
        | Ty::RegExp
        | Ty::Error
        | Ty::Promise => "object",
        _ => return None,
    })
}

/// `{}`, or a never-reassigned variable initialized with `{}`.
fn is_empty_object(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::ObjectExpression(o) => o.properties.is_empty(),
        Expression::Identifier(id) => ctx.symbol_of(id).is_some_and(|sym| {
            !ctx.scoping().symbol_is_mutated(sym)
                && matches!(ctx.symbol_decl(sym).kind(), AstKind::VariableDeclarator(v)
                    if matches!(v.init.as_ref().map(strip_parens), Some(Expression::ObjectExpression(o)) if o.properties.is_empty()))
                && !has_expando_assignment(ctx, sym)
        }),
        _ => false,
    }
}

/// `x.prop = ...` somewhere: TypeScript then infers the declared properties (JS expandos).
fn has_expando_assignment(ctx: &Ctx<'_, '_>, sym: SymbolId) -> bool {
    let scoping = ctx.scoping();
    scoping.get_resolved_reference_ids(sym).iter().any(|&r| {
        let member = ctx.parent(scoping.get_reference(r).node_id());
        matches!(member.kind(), AstKind::StaticMemberExpression(_))
            && matches!(ctx.parent_kind(member.id()), AstKind::AssignmentExpression(a) if a.left.span() == member.kind().span())
    })
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if !helpers::batch_smell3::has_type_services(ctx) {
            return;
        }
        let returns = returns_by_function(ctx);
        for (func_id, rets) in &returns {
            let node = ctx.node(*func_id);
            let (is_async, generator, annotated) = match node.kind() {
                AstKind::Function(f) => (f.r#async, f.generator, f.return_type.is_some()),
                AstKind::ArrowFunctionExpression(a) => (a.r#async, false, a.return_type.is_some()),
                _ => continue,
            };
            if is_async || generator || annotated {
                continue;
            }
            let mut entries: Vec<(&ReturnStatement<'a>, &'static str)> = Vec::new();
            let mut ok = true;
            for r in rets {
                let Some(arg) = &r.argument else { continue };
                // `this` returns are excluded; `{}` absorbs every other object type in the
                // inferred (subtype-reduced) return type union.
                if matches!(strip_ts(arg), Expression::ThisExpression(_))
                    || is_empty_object(ctx, arg)
                {
                    ok = false;
                    break;
                }
                let t = type_of(ctx, arg);
                if matches!(t, Ty::Null | Ty::Undefined | Ty::Void) {
                    continue;
                }
                match category(&t) {
                    Some(c) => entries.push((r, c)),
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                continue;
            }
            let mut cats: Vec<&str> = entries.iter().map(|(_, c)| *c).collect();
            cats.sort_unstable();
            cats.dedup();
            if cats.len() <= 1 {
                continue;
            }
            // `string | true` sanitizers are accepted.
            if cats == ["boolean", "string"]
                && entries.iter().filter(|(_, c)| *c == "boolean").all(|(r, _)| {
                    matches!(r.argument.as_ref().map(strip_parens), Some(Expression::BooleanLiteral(b)) if b.value)
                })
            {
                continue;
            }
            if function_jsdoc(ctx, *func_id).is_some_and(|d| d.contains("@return")) {
                continue;
            }
            ctx.report_with(
                function_report_span(ctx, node),
                "Refactor this function to always return the same type.",
                entries
                    .iter()
                    .map(|(r, c)| (r.span, Some(format!("Returns {c}")))),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn return_type() {
        check(
            "S3800",
            "a.js",
            r#"
function foo(x) {          // Noncompliant {{Refactor this function to always return the same type.}}
  if (x) {
    return 1;
  }
  return 'one';
}
function same(x) {
  if (x) return 1;
  return 2;
}
function nullable(x) {
  if (x) return null;
  return 'a';
}
function sanitize(x) {
  if (x) return true;
  return 'error';
}
function unknown(x) {
  if (x) return bar();
  return 'a';
}
/** @returns {string|number} */
function documented(x) {
  if (x) return 1;
  return 'a';
}
const arrow = (x) => {     // Noncompliant
  if (x) return [];
  return { a: 1 };
};
function self(x) {
  if (x) return this;
  return 1;
}
function nested() {
  const inner = () => 'a';
  return 1;
}
"#,
        );
    }
}
