//! Literals should not be used for promise rejection
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MESSAGE: &str = "Expected the Promise rejection reason to be an Error.";

/// True when the rejection reason is known not to be an `Error`.
fn is_known_non_error(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::StringLiteral(_)
        | Expression::TemplateLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::ObjectExpression(_)
        | Expression::ArrayExpression(_)
        | Expression::ArrowFunctionExpression(_)
        | Expression::FunctionExpression(_) => true,
        e if is_undefined(ctx, e) => true,
        e => {
            let t = type_of(ctx, e);
            t.is_primitive() || t.is_array()
        }
    }
}

/// True if `id` is the second parameter of a `new Promise((resolve, reject) => ...)` executor.
fn is_executor_reject(ctx: &Ctx<'_, '_>, id: &IdentifierReference<'_>) -> bool {
    let Some(sym) = ctx.symbol_of(id) else {
        return false;
    };
    let decl = ctx.symbol_decl(sym);
    let AstKind::FormalParameter(param) = decl.kind() else {
        return false;
    };
    let AstKind::FormalParameters(params) = ctx.parent_kind(decl.id()) else {
        return false;
    };
    if params.items.len() < 2 || params.items[1].span != param.span {
        return false;
    }
    let func = ctx.parent(ctx.parent(decl.id()).id());
    if !matches!(
        func.kind(),
        AstKind::Function(_) | AstKind::ArrowFunctionExpression(_)
    ) {
        return false;
    }
    let AstKind::NewExpression(new) = ctx.parent_kind(func.id()) else {
        return false;
    };
    matches!(&new.callee, Expression::Identifier(p) if ctx.is_global(p, "Promise"))
        && new
            .arguments
            .first()
            .is_some_and(|a| a.span() == func.kind().span())
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let is_reject = match &call.callee {
            Expression::StaticMemberExpression(m) => {
                m.property.name == "reject"
                    && matches!(&m.object, Expression::Identifier(p) if ctx.is_global(p, "Promise"))
            }
            Expression::Identifier(id) => is_executor_reject(ctx, id),
            _ => false,
        };
        if !is_reject {
            return;
        }
        match call.arguments.first() {
            None => ctx.report(call.span, MESSAGE),
            Some(arg) => {
                if let Some(e) = arg.as_expression()
                    && is_known_non_error(ctx, e)
                {
                    ctx.report(call.span, MESSAGE);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S6671",
            "a.ts",
            r#"
Promise.reject("error"); // Noncompliant {{Expected the Promise rejection reason to be an Error.}}
Promise.reject(42); // Noncompliant
Promise.reject(); // Noncompliant
Promise.reject({ message: "x" }); // Noncompliant
new Promise((resolve, reject) => {
  reject("error"); // Noncompliant
  setTimeout(() => reject(`late ${x}`)); // Noncompliant
  reject(new Error("error"));
});
function f(msg: string) {
  return Promise.reject(msg); // Noncompliant
}
Promise.reject(new Error("error"));
Promise.reject(err);
function g(e: any) { return Promise.reject(e); }
function h(reject) { reject("x"); }
new Promise((resolve) => resolve("ok"));
"#,
        );
    }
}
