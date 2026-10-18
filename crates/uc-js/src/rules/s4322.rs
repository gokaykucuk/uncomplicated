//! Type predicates should be used
use crate::helpers::batch_ts::ancestors;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_undefined_ident(e: &Expression<'_>) -> bool {
    matches!(strip_parens(e), Expression::Identifier(id) if id.name == "undefined")
}

/// For `(x as T).prop`, returns `(x, T)`.
fn cast_tuple<'b, 'a>(e: &'b Expression<'a>) -> Option<(&'b Expression<'a>, &'b TSType<'a>)> {
    let object = match strip_parens(e) {
        Expression::StaticMemberExpression(m) => &m.object,
        Expression::ComputedMemberExpression(m) => &m.object,
        Expression::PrivateFieldExpression(m) => &m.object,
        _ => return None,
    };
    match strip_parens(object) {
        Expression::TSAsExpression(a) => Some((&a.expression, &a.type_annotation)),
        Expression::TSTypeAssertion(a) => Some((&a.expression, &a.type_annotation)),
        _ => None,
    }
}

fn checked_expression<'b, 'a>(e: &'b Expression<'a>) -> Option<&'b Expression<'a>> {
    match strip_parens(e) {
        Expression::BinaryExpression(b)
            if matches!(
                b.operator,
                BinaryOperator::StrictInequality | BinaryOperator::Inequality
            ) =>
        {
            if is_undefined_ident(&b.right) {
                Some(&b.left)
            } else if is_undefined_ident(&b.left) {
                Some(&b.right)
            } else {
                None
            }
        }
        Expression::CallExpression(c)
            if c.arguments.len() == 1
                && matches!(&c.callee, Expression::Identifier(id) if id.name == "Boolean") =>
        {
            c.arguments[0].as_expression()
        }
        Expression::UnaryExpression(u) if u.operator == UnaryOperator::LogicalNot => {
            match strip_parens(&u.argument) {
                Expression::UnaryExpression(u2) if u2.operator == UnaryOperator::LogicalNot => {
                    Some(&u2.argument)
                }
                _ => None,
            }
        }
        _ => None,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Function]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::Function(f) = node.kind() else {
            return;
        };
        let applies = match f.r#type {
            FunctionType::FunctionDeclaration => true,
            FunctionType::FunctionExpression => ancestors(ctx, node.id())
                .any(|a| matches!(a.kind(), AstKind::MethodDefinition(m) if m.kind == MethodDefinitionKind::Method)),
            _ => false,
        };
        if !applies {
            return;
        }
        if matches!(
            f.return_type.as_deref().map(|r| &r.type_annotation),
            Some(TSType::TSTypePredicate(_))
        ) {
            return;
        }
        let Some(body) = &f.body else { return };
        let [Statement::ReturnStatement(ret)] = body.statements.as_slice() else {
            return;
        };
        let Some(arg) = &ret.argument else { return };
        let Some(checked) = checked_expression(arg) else {
            return;
        };
        let Some((casted, ty)) = cast_tuple(checked) else {
            return;
        };
        if matches!(ty, TSType::TSAnyKeyword(_)) {
            return;
        }
        let n_params = f.params.items.len()
            + usize::from(f.params.rest.is_some())
            + usize::from(f.this_param.is_some());
        let is_this = matches!(strip_parens(casted), Expression::ThisExpression(_));
        if n_params == 1 || (n_params == 0 && is_this) {
            ctx.report(
                function_report_span(ctx, node),
                format!(
                    "Declare this function return type using type predicate \"{} is {}\".",
                    ctx.text_of(casted),
                    ctx.text_of(ty)
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn type_guard() {
        check(
            "S4322",
            "a.ts",
            r#"
function isSomething(x: BaseType): boolean { // Noncompliant {{Declare this function return type using type predicate "x is Something".}}
  return (<Something>x).foo !== undefined;
}
function isCat(animal: Animal) { // Noncompliant {{Declare this function return type using type predicate "animal is Cat".}}
  return (animal as Cat).meow !== undefined;
}
function isB(x: A) { return !!(x as B).b; } // Noncompliant
function isC(x: A) { return Boolean((x as C).c); } // Noncompliant
function ok1(x: A): x is B { return (x as B).b !== undefined; }
function ok2(x: A, y: A) { return (x as B).b !== undefined; }
function ok3(x: A) { return (x as any).b !== undefined; }
class K {
  isFish() { // Noncompliant
    return (this as Fish).swim !== undefined;
  }
  isOk(a: A) {
    foo();
    return (a as B).b !== undefined;
  }
}
const arrow = (x: A) => (x as B).b !== undefined;
"#,
        );
    }
}
