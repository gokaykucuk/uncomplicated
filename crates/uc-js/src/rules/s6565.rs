//! Prefer the return type "this" in fluent interfaces
use oxc_ast_visit::{Visit, walk};
use oxc_syntax::scope::ScopeFlags;

use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Counts `return this` and other returns in a function body (nested functions excluded).
#[derive(Default)]
struct Returns {
    this: usize,
    other: usize,
}

impl<'a> Visit<'a> for Returns {
    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}
    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
    fn visit_class(&mut self, _: &Class<'a>) {}

    fn visit_return_statement(&mut self, r: &ReturnStatement<'a>) {
        match r.argument.as_ref().map(strip_parens) {
            Some(Expression::ThisExpression(_)) => self.this += 1,
            _ => self.other += 1,
        }
        walk::walk_return_statement(self, r);
    }
}

fn check<'a>(
    ctx: &Ctx<'a, '_>,
    class_name: &str,
    ret: Option<&TSTypeAnnotation<'a>>,
    body: Option<&FunctionBody<'a>>,
    expr_body: Option<&Expression<'a>>,
) {
    let Some(ret) = ret else { return };
    let TSType::TSTypeReference(r) = &ret.type_annotation else {
        return;
    };
    if !matches!(&r.type_name, TSTypeName::IdentifierReference(id) if id.name == class_name) {
        return;
    }
    let mut v = Returns::default();
    if let Some(e) = expr_body {
        match strip_parens(e) {
            Expression::ThisExpression(_) => v.this += 1,
            _ => v.other += 1,
        }
    } else {
        let Some(body) = body else { return };
        for s in &body.statements {
            v.visit_statement(s);
        }
    }
    if v.this > 0 && v.other == 0 {
        ctx.report(r.span, "Use `this` type instead.");
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Class]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        if crate::helpers::batch_ts::no_type_info(ctx) {
            return;
        }
        let AstKind::Class(class) = node.kind() else {
            return;
        };
        let Some(id) = &class.id else { return };
        for el in &class.body.body {
            match el {
                ClassElement::MethodDefinition(m)
                    if m.kind == MethodDefinitionKind::Method && !m.r#static =>
                {
                    check(
                        ctx,
                        &id.name,
                        m.value.return_type.as_deref(),
                        m.value.body.as_deref(),
                        None,
                    );
                }
                ClassElement::PropertyDefinition(p) if !p.r#static => {
                    match p.value.as_ref().map(strip_parens) {
                        Some(Expression::ArrowFunctionExpression(a)) => {
                            let ret = a.return_type.as_deref();
                            match a.body.as_function_body() {
                                Some(b) => check(ctx, &id.name, ret, Some(b), None),
                                None => check(ctx, &id.name, ret, None, a.body.as_expression()),
                            }
                        }
                        Some(Expression::FunctionExpression(f)) => {
                            check(
                                ctx,
                                &id.name,
                                f.return_type.as_deref(),
                                f.body.as_deref(),
                                None,
                            );
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn return_this() {
        check(
            "S6565",
            "a.ts",
            r#"
class Foo {
  f1(): Foo { // Noncompliant {{Use `this` type instead.}}
    return this;
  }
  f2 = (): Foo => this; // Noncompliant
  f3(): Foo | undefined {
    return Math.random() > 0.5 ? this : undefined;
  }
  f4(): this { return this; }
  f5(): Foo {
    if (x) return new Foo();
    return this;
  }
  f6(): Foo { // Noncompliant
    const g = () => { return other; };
    return this;
  }
  static s(): Foo { return this as any; }
}
"#,
        );
    }
}
