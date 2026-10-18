//! Functions should use "return" consistently
use crate::helpers::batch_smell3::{list_terminates, returns_by_function};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn contains_void_or_never(t: &TSType<'_>) -> bool {
    match t {
        TSType::TSUndefinedKeyword(_) | TSType::TSVoidKeyword(_) | TSType::TSNeverKeyword(_) => {
            true
        }
        TSType::TSUnionType(u) => u.types.iter().any(contains_void_or_never),
        _ => false,
    }
}

/// Names of calls that may be typed as returning `never` (which makes the implicit return
/// unreachable for Sonar's type-aware check).
fn may_be_never_call(name: &str) -> bool {
    matches!(
        name,
        "exit"
            | "fail"
            | "abort"
            | "assertNever"
            | "unreachable"
            | "panic"
            | "throwError"
            | "raise"
            | "die"
    ) || name.starts_with("throw")
}

/// Last expression-statement call of the function body (outside nested functions).
fn last_call_statement<'a>(stmts: &'a [Statement<'a>]) -> Option<&'a CallExpression<'a>> {
    let mut last = None;
    for s in stmts {
        if let Some(c) = last_call_in(s) {
            last = Some(c);
        }
    }
    last
}

fn last_call_in<'a>(s: &'a Statement<'a>) -> Option<&'a CallExpression<'a>> {
    match s {
        Statement::ExpressionStatement(e) => match &e.expression {
            Expression::CallExpression(c) => Some(c),
            _ => None,
        },
        Statement::BlockStatement(b) => last_call_statement(&b.body),
        Statement::IfStatement(i) => i
            .alternate
            .as_ref()
            .and_then(last_call_in)
            .or_else(|| last_call_in(&i.consequent)),
        Statement::TryStatement(t) => t
            .finalizer
            .as_ref()
            .and_then(|f| last_call_statement(&f.body))
            .or_else(|| {
                t.handler
                    .as_ref()
                    .and_then(|h| last_call_statement(&h.body.body))
            })
            .or_else(|| last_call_statement(&t.block.body)),
        Statement::SwitchStatement(sw) => sw
            .cases
            .iter()
            .rev()
            .find_map(|c| last_call_statement(&c.consequent)),
        Statement::LabeledStatement(l) => last_call_in(&l.body),
        Statement::WhileStatement(w) => last_call_in(&w.body),
        Statement::DoWhileStatement(w) => last_call_in(&w.body),
        Statement::ForStatement(w) => last_call_in(&w.body),
        Statement::ForInStatement(w) => last_call_in(&w.body),
        Statement::ForOfStatement(w) => last_call_in(&w.body),
        _ => None,
    }
}

fn call_may_return_never(ctx: &Ctx<'_, '_>, c: &CallExpression<'_>) -> bool {
    if callee_name(c).is_some_and(may_be_never_call) {
        return true;
    }
    let Expression::Identifier(id) = strip_ts(&c.callee) else {
        return false;
    };
    let Some(sym) = ctx.symbol_of(id) else {
        return false;
    };
    match ctx.symbol_decl(sym).kind() {
        AstKind::Function(f) => f
            .return_type
            .as_ref()
            .is_some_and(|r| matches!(r.type_annotation, TSType::TSNeverKeyword(_))),
        AstKind::VariableDeclarator(v) => match v.init.as_ref().map(strip_ts) {
            Some(Expression::ArrowFunctionExpression(a)) => {
                a.return_type.is_some()
                    || a.body
                        .as_function_body()
                        .is_some_and(|b| list_terminates(&b.statements, true))
            }
            Some(Expression::FunctionExpression(f)) => {
                f.return_type.is_some()
                    || f.body
                        .as_ref()
                        .is_some_and(|b| list_terminates(&b.statements, true))
            }
            _ => v.type_annotation.is_some() || (v.init.is_none() && ctx.is_ts()),
        },
        // In TypeScript, imports and parameters may be typed `never`.
        _ => ctx.is_ts(),
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let returns = returns_by_function(ctx);
        for (func_id, rets) in &returns {
            let node = ctx.node(*func_id);
            let (body, return_type) = match node.kind() {
                AstKind::Function(f) => (f.body.as_deref(), f.return_type.as_deref()),
                AstKind::ArrowFunctionExpression(a) => {
                    (a.body.as_function_body(), a.return_type.as_deref())
                }
                _ => continue,
            };
            let Some(body) = body else { continue };
            if return_type.is_some_and(|r| contains_void_or_never(&r.type_annotation)) {
                continue;
            }
            let with_value = rets.iter().any(|r| r.argument.is_some());
            if !with_value {
                continue;
            }
            let without_value = rets.iter().any(|r| r.argument.is_none());
            let implicit = !list_terminates(&body.statements, true)
                && !last_call_statement(&body.statements)
                    .is_some_and(|c| call_may_return_never(ctx, c));
            if !(without_value || implicit) {
                continue;
            }
            let mut secondary: Vec<(Span, Option<String>)> = rets
                .iter()
                .map(|r| {
                    let m = if r.argument.is_some() {
                        "Return with value"
                    } else {
                        "Return without value"
                    };
                    (r.span, Some(m.to_string()))
                })
                .collect();
            if implicit && body.span.end > body.span.start {
                secondary.push((
                    Span::new(body.span.end - 1, body.span.end),
                    Some("Implicit return without value".into()),
                ));
            }
            ctx.report_with(
                function_report_span(ctx, node),
                "Refactor this function to use \"return\" consistently.",
                secondary,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn inconsistent_returns() {
        check(
            "S3801",
            "a.ts",
            r#"
function foo(a) {          // Noncompliant {{Refactor this function to use "return" consistently.}}
  if (a) {
    return true;
  }
}
function bar(a) {          // Noncompliant
  if (a) {
    return;
  }
  return 1;
}
function ok1(a) {
  if (a) {
    return 1;
  }
  return 2;
}
function ok2(a) {
  if (a) {
    return 1;
  } else {
    throw new Error();
  }
}
function ok3(a) {
  switch (a) {
    case 1: return 'a';
    default: return 'b';
  }
}
function ok4(a) {
  while (true) {
    if (a()) return 1;
  }
}
function ok5(a): number | undefined {
  if (a) return 1;
}
function ok6(a) {
  if (a) return 1;
  fail('x');
}
function ok7(a) {
  try {
    return a();
  } catch (e) {
    throw e;
  }
}
function ko2(a) {          // Noncompliant
  while (true) {
    if (a()) return 1;
    if (a()) break;
  }
}
function ok8(a) {
  if (a) {
    doSomething();
    return;
  }
  other();
}
const k = (a) => {        // Noncompliant
  for (const x of a) {
    if (x) return x;
  }
};
function ok9(k: 'a' | 'b') {
  switch (k) {
    case 'a': return 1;
    case 'b': return 2;
  }
}
"#,
        );
    }
}
