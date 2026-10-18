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

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
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
                if matches!(strip_ts(arg), Expression::ThisExpression(_)) {
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
  return {};
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
