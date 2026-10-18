//! Primitive return types should be used
use oxc_ast_visit::{Visit, walk};
use oxc_syntax::scope::ScopeFlags;

use crate::helpers::batch_ts::strip_type_parens;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Coarse TypeScript type flags of an expression, when they are unambiguous.
#[derive(PartialEq, Clone, Copy)]
enum Flags {
    StringLiteral,
    NumberLiteral,
    BooleanLiteral,
    String,
    Number,
    Boolean,
}

fn flags(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> Option<Flags> {
    match strip_parens(e) {
        Expression::StringLiteral(_) => Some(Flags::StringLiteral),
        Expression::TemplateLiteral(t) => Some(if t.expressions.is_empty() {
            Flags::StringLiteral
        } else {
            Flags::String
        }),
        Expression::NumericLiteral(_) => Some(Flags::NumberLiteral),
        Expression::BooleanLiteral(_) => Some(Flags::BooleanLiteral),
        Expression::UnaryExpression(u) => match u.operator {
            UnaryOperator::UnaryNegation
                if matches!(strip_parens(&u.argument), Expression::NumericLiteral(_)) =>
            {
                Some(Flags::NumberLiteral)
            }
            UnaryOperator::Typeof => Some(Flags::String),
            UnaryOperator::LogicalNot => Some(Flags::Boolean),
            _ => None,
        },
        Expression::BinaryExpression(b) => {
            use BinaryOperator::*;
            match b.operator {
                Equality | Inequality | StrictEquality | StrictInequality | LessThan
                | LessEqualThan | GreaterThan | GreaterEqualThan | In | Instanceof => {
                    Some(Flags::Boolean)
                }
                _ => {
                    let t = type_of(ctx, e);
                    if t == Ty::String {
                        Some(Flags::String)
                    } else if t == Ty::Number
                        && type_of(ctx, &b.left).is_number()
                        && type_of(ctx, &b.right).is_number()
                    {
                        Some(Flags::Number)
                    } else {
                        None
                    }
                }
            }
        }
        Expression::CallExpression(c) => match &c.callee {
            Expression::Identifier(id) if ctx.symbol_of(id).is_none() => match id.name.as_str() {
                "String" => Some(Flags::String),
                "Number" => Some(Flags::Number),
                "Boolean" => Some(Flags::Boolean),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

/// Collects the flags of return arguments of a function declaration, including those of nested
/// function expressions and arrows (but not of nested function declarations).
struct Returns<'c, 'a, 's> {
    ctx: &'c Ctx<'a, 's>,
    found: Vec<Option<Flags>>,
}

impl<'a> Visit<'a> for Returns<'_, 'a, '_> {
    fn visit_function(&mut self, f: &Function<'a>, flags: ScopeFlags) {
        if f.r#type != FunctionType::FunctionDeclaration {
            walk::walk_function(self, f, flags);
        }
    }

    fn visit_return_statement(&mut self, r: &ReturnStatement<'a>) {
        let f = r.argument.as_ref().and_then(|e| flags(self.ctx, e));
        self.found.push(f);
        walk::walk_return_statement(self, r);
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Function]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        if crate::helpers::batch_ts::no_type_info(ctx) {
            return;
        }
        let AstKind::Function(f) = node.kind() else {
            return;
        };
        if f.r#type != FunctionType::FunctionDeclaration {
            return;
        }
        let Some(ret) = &f.return_type else { return };
        if !matches!(
            strip_type_parens(&ret.type_annotation),
            TSType::TSAnyKeyword(_)
        ) {
            return;
        }
        let Some(body) = &f.body else { return };
        let mut v = Returns {
            ctx,
            found: Vec::new(),
        };
        v.visit_function_body(body);
        let Some(Some(first)) = v.found.first().copied() else {
            return;
        };
        if v.found.iter().all(|f| *f == Some(first)) {
            ctx.report(
                ret.span,
                "Remove this return type or change it to a more specific.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn return_any() {
        check(
            "S4324",
            "a.ts",
            r#"
function foo(): any { // Noncompliant {{Remove this return type or change it to a more specific.}}
  return 1;
}
function bar(a: number): any { // Noncompliant
  if (a) return a > 1;
  return a === 2;
}
function mixed(a): any {
  if (a) return 1;
  return 'a';
}
function lit(a): any {
  if (a) return true;
  return a > 1;
}
function unknownType(a): any {
  return a;
}
function nested(): any { // Noncompliant
  const f = () => { return 'x'; };
  return 'y';
}
function empty(): any {
  return;
}
function ok(): string { return 'a'; }
"#,
        );
    }
}
