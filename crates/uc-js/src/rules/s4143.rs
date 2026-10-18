//! Collection elements should not be replaced unconditionally
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

struct KeyWrite<'e> {
    collection: &'e Expression<'e>,
    key: String,
    span: Span,
}

fn number_key(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e21 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

fn extract_key(e: &Expression<'_>) -> Option<String> {
    match e {
        Expression::StringLiteral(s) => Some(s.value.to_string()),
        Expression::NumericLiteral(n) => Some(number_key(n.value)),
        Expression::Identifier(id) => Some(id.name.to_string()),
        _ => None,
    }
}

/// True if the tokens of `value` appear contiguously in `expr`.
fn is_used(ctx: &Ctx<'_, '_>, value: Span, expr: Span) -> bool {
    let v = ctx.tokens_in(value);
    let e = ctx.tokens_in(expr);
    if v.is_empty() || v.len() > e.len() {
        return false;
    }
    e.windows(v.len()).any(|w| {
        w.iter()
            .zip(v)
            .all(|(a, b)| ctx.text(a.span()) == ctx.text(b.span()))
    })
}

fn key_write<'e>(ctx: &Ctx<'_, '_>, stmt: &'e Statement<'e>) -> Option<KeyWrite<'e>> {
    let Statement::ExpressionStatement(es) = stmt else {
        return None;
    };
    match &es.expression {
        Expression::AssignmentExpression(a) if a.operator == AssignmentOperator::Assign => {
            let AssignmentTarget::ComputedMemberExpression(m) = &a.left else {
                return None;
            };
            let key = extract_key(&m.expression)?;
            if is_used(ctx, m.object.span(), a.right.span()) {
                return None;
            }
            Some(KeyWrite {
                collection: &m.object,
                key,
                span: a.span,
            })
        }
        Expression::CallExpression(c) => {
            let Expression::StaticMemberExpression(m) = &c.callee else {
                return None;
            };
            let ok = match m.property.name.as_str() {
                "add" => c.arguments.len() == 1,
                "set" => c.arguments.len() == 2,
                _ => false,
            };
            if !ok {
                return None;
            }
            let key = extract_key(c.arguments[0].as_expression()?).filter(|k| !k.is_empty())?;
            Some(KeyWrite {
                collection: &m.object,
                key,
                span: c.span,
            })
        }
        _ => None,
    }
}

fn check<'e>(ctx: &Ctx<'_, '_>, stmts: &'e [Statement<'e>]) {
    let mut used: FxHashMap<String, Span> = FxHashMap::default();
    let mut collection: Option<&Expression<'_>> = None;
    for s in stmts {
        let Some(w) = key_write(ctx, s) else {
            used.clear();
            continue;
        };
        if let Some(c) = collection
            && !(std::mem::discriminant(c) == std::mem::discriminant(w.collection)
                && ctx.same_tokens(c.span(), w.collection.span()))
        {
            used.clear();
        }
        if let Some(prev) = used.get(&w.key) {
            let line = ctx.line(prev.start);
            ctx.report_with(
                w.span,
                format!("Verify this is the index that was intended; \"{}\" was already set on line {line}.", w.key),
                [(*prev, Some("Original value".to_string()))],
            );
        }
        collection = Some(w.collection);
        used.insert(w.key, w.span);
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::Program,
            AstType::BlockStatement,
            AstType::FunctionBody,
            AstType::SwitchCase,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::Program(p) => check(ctx, &p.body),
            AstKind::BlockStatement(b) => check(ctx, &b.body),
            AstKind::FunctionBody(b) => check(ctx, &b.statements),
            AstKind::SwitchCase(c) => check(ctx, &c.consequent),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn overwrite() {
        check(
            "S4143",
            "a.js",
            r#"
function f(fruits, myMap, mySet) {
  fruits[1] = "banana";
  fruits[1] = "apple"; // Noncompliant {{Verify this is the index that was intended; "1" was already set on line 3.}}
  myMap.set("key", 1);
  myMap.set("key", 2); // Noncompliant
  mySet.add(1);
  mySet.add(1); // Noncompliant
  fruits[i] = 1;
  fruits[i] = 2; // Noncompliant
}
function g(a, b) {
  a[0] = 1;
  b[0] = 1;
  a[0] = 2;
  a[1] = a[1] + 1;
  a[1] = a[1] * 2;
  a[2] = 1;
  foo();
  a[2] = 2;
  a["x"] = 1;
  a["y"] = 2;
}
"#,
        );
    }
}
