//! Enum values should be unique
use crate::helpers::batch_ts::fmt_number;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

#[derive(Clone, Copy)]
enum Value<'a> {
    Str(&'a str),
    Num(f64),
}

impl PartialEq for Value<'_> {
    /// `Object.is` semantics.
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Num(a), Value::Num(b)) => a.to_bits() == b.to_bits(),
            _ => false,
        }
    }
}

fn to_number(v: Value<'_>) -> Option<f64> {
    match v {
        Value::Num(n) => Some(n),
        Value::Str(s) => {
            let s = s.trim();
            if s.is_empty() {
                Some(0.0)
            } else {
                s.parse::<f64>().ok().filter(|n| n.is_finite())
            }
        }
    }
}

fn member_value<'a>(e: &Expression<'a>) -> Option<Value<'a>> {
    match e {
        Expression::StringLiteral(s) => Some(Value::Str(s.value.as_str())),
        Expression::NumericLiteral(n) => Some(Value::Num(n.value)),
        Expression::UnaryExpression(u)
            if matches!(
                u.operator,
                UnaryOperator::UnaryNegation | UnaryOperator::UnaryPlus
            ) =>
        {
            let inner = to_number(member_value(&u.argument)?)?;
            Some(Value::Num(if u.operator == UnaryOperator::UnaryNegation {
                -inner
            } else {
                inner
            }))
        }
        Expression::TemplateLiteral(t) if t.expressions.is_empty() && t.quasis.len() == 1 => t
            .quasis[0]
            .value
            .cooked
            .as_ref()
            .map(|c| Value::Str(c.as_str())),
        _ => None,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::TSEnumDeclaration]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::TSEnumDeclaration(e) = node.kind() else {
            return;
        };
        let mut seen: Vec<Value<'a>> = Vec::new();
        for m in &e.body.members {
            let Some(value) = m.initializer.as_ref().and_then(member_value) else {
                continue;
            };
            if seen.contains(&value) {
                let text = match value {
                    Value::Str(s) => s.to_string(),
                    Value::Num(n) => fmt_number(n),
                };
                ctx.report(m.span, format!("Duplicate enum member value {text}."));
            } else {
                seen.push(value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn duplicates() {
        check(
            "S6578",
            "a.ts",
            r#"
enum E {
  A = 0,
  B = 0, // Noncompliant {{Duplicate enum member value 0.}}
  C = 'A',
  D = 'A', // Noncompliant {{Duplicate enum member value A.}}
  F = `A`, // Noncompliant
  G = '0',
  H,
  I = -1,
  J = -1, // Noncompliant {{Duplicate enum member value -1.}}
}
enum F { A = 1, B = 2, C = 'x' }
"#,
        );
    }
}
