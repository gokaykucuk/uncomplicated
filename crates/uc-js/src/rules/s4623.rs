//! "undefined" should not be passed as the value of optional parameters
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Parameters of the function a plain identifier callee refers to (single declaration only).
fn callee_params<'a>(
    ctx: &Ctx<'a, '_>,
    id: &IdentifierReference<'a>,
) -> Option<&'a FormalParameters<'a>> {
    let sym = ctx.symbol_of(id)?;
    if !ctx.scoping().symbol_redeclarations(sym).is_empty() {
        return None;
    }
    match ctx.symbol_decl(sym).kind() {
        AstKind::Function(f) => Some(&f.params),
        AstKind::VariableDeclarator(v) if !ctx.scoping().symbol_is_mutated(sym) => {
            match v.init.as_ref().map(strip_parens) {
                Some(Expression::ArrowFunctionExpression(a)) => Some(&a.params),
                Some(Expression::FunctionExpression(f)) => Some(&f.params),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Parameters of `this.name(...)` when `name` is a single, non-overloaded method of the enclosing class.
fn this_method_params<'a>(
    ctx: &Ctx<'a, '_>,
    node: NodeId,
    name: &str,
) -> Option<&'a FormalParameters<'a>> {
    let mut is_static = None;
    for a in ctx.ancestors(node) {
        match a.kind() {
            AstKind::ArrowFunctionExpression(_) => {}
            AstKind::Function(_) if is_static.is_none() => match ctx.parent_kind(a.id()) {
                AstKind::MethodDefinition(m) => is_static = Some(m.r#static),
                _ => return None,
            },
            AstKind::Class(c) => {
                let is_static = is_static?;
                let mut found = None;
                for el in &c.body.body {
                    if let ClassElement::MethodDefinition(m) = el
                        && property_key_name(&m.key) == Some(name)
                        && !m.computed
                    {
                        if found.is_some()
                            || m.r#static != is_static
                            || m.kind != MethodDefinitionKind::Method
                        {
                            return None;
                        }
                        found = Some(&m.value.params);
                    }
                }
                return found.map(|p| &**p);
            }
            _ => {}
        }
    }
    None
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        if crate::helpers::batch_ts::no_type_info(ctx) {
            return;
        }
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let Some(Argument::Identifier(last)) = call.arguments.last() else {
            return;
        };
        if last.name != "undefined" {
            return;
        }
        let params = match strip_parens(&call.callee) {
            Expression::Identifier(id) => callee_params(ctx, id),
            Expression::StaticMemberExpression(m)
                if matches!(strip_parens(&m.object), Expression::ThisExpression(_)) =>
            {
                this_method_params(ctx, node.id(), &m.property.name)
            }
            _ => None,
        };
        let Some(params) = params else { return };
        let Some(param) = params.items.get(call.arguments.len() - 1) else {
            return;
        };
        if param.optional || param.initializer.is_some() {
            ctx.report(last.span, "Remove this redundant \"undefined\".");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn undefined_argument() {
        check(
            "S4623",
            "a.ts",
            r#"
function foo(x: number, y: string = "default", z?: number) {}
foo(42, undefined); // Noncompliant {{Remove this redundant "undefined".}}
foo(42, "a", undefined); // Noncompliant
foo(undefined, "a");
foo(42);
const bar = (a?: number) => {};
bar(undefined); // Noncompliant
unknownFn(undefined);
function over(a?: number): void;
function over(a?: number) {}
over(undefined);
class C {
  m(a?: string) {}
  n() {
    this.m(undefined); // Noncompliant
  }
}
"#,
        );
    }
}
