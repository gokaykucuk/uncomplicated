//! Unnecessary constructors should be removed
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_simple(p: &FormalParameter<'_>) -> bool {
    p.initializer.is_none() && matches!(p.pattern, BindingPattern::BindingIdentifier(_))
}

fn param_name<'a>(p: &'a FormalParameter<'a>) -> Option<&'a str> {
    match &p.pattern {
        BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
        _ => None,
    }
}

fn rest_name<'a>(params: &'a FormalParameters<'a>) -> Option<Option<&'a str>> {
    params.rest.as_ref().map(|r| match &r.rest.argument {
        BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
        _ => None,
    })
}

/// `super(...arguments)` or `super(a, b, ...c)` passing the parameters through unchanged.
fn is_redundant_super_call(params: &FormalParameters<'_>, body: &FunctionBody<'_>) -> bool {
    if !body.directives.is_empty() || body.statements.len() != 1 {
        return false;
    }
    let Statement::ExpressionStatement(stmt) = &body.statements[0] else {
        return false;
    };
    let Expression::CallExpression(call) = &stmt.expression else {
        return false;
    };
    if !matches!(call.callee, Expression::Super(_)) {
        return false;
    }
    if !params.items.iter().all(is_simple) || rest_name(params).is_some_and(|n| n.is_none()) {
        return false;
    }
    let args = &call.arguments;
    if args.len() == 1
        && let Argument::SpreadElement(s) = &args[0]
        && matches!(&s.argument, Expression::Identifier(id) if id.name == "arguments")
    {
        return true;
    }
    let rest = rest_name(params).flatten();
    if params.items.len() + usize::from(rest.is_some()) != args.len() {
        return false;
    }
    for (i, arg) in args.iter().enumerate() {
        let ok = if i < params.items.len() {
            matches!(arg, Argument::Identifier(id) if Some(id.name.as_str()) == param_name(&params.items[i]))
        } else {
            matches!(arg, Argument::SpreadElement(s)
                if matches!(&s.argument, Expression::Identifier(id) if Some(id.name.as_str()) == rest))
        };
        if !ok {
            return false;
        }
    }
    true
}

/// The superclass may have a constructor this one changes the visibility of.
fn inherits_restricted_constructor(ctx: &Ctx<'_, '_>, heritage: &Expression<'_>) -> bool {
    let Expression::Identifier(id) = heritage else {
        return false;
    };
    let Some(sym) = ctx.symbol_of(id) else {
        return false;
    };
    match ctx.symbol_decl(sym).kind() {
        AstKind::Class(c) => c.body.body.iter().any(|m| {
            matches!(m, ClassElement::MethodDefinition(m)
                if m.kind == MethodDefinitionKind::Constructor && m.accessibility == Some(TSAccessibility::Protected))
        }),
        AstKind::ImportSpecifier(_) | AstKind::ImportDefaultSpecifier(_) | AstKind::ImportNamespaceSpecifier(_) => true,
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::MethodDefinition]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::MethodDefinition(m) = node.kind() else {
            return;
        };
        if m.kind != MethodDefinitionKind::Constructor
            || m.r#type != MethodDefinitionType::MethodDefinition
        {
            return;
        }
        let Some(body) = &m.value.body else { return };
        let class_node = ctx.parent(ctx.parent(node.id()).id());
        let AstKind::Class(class) = class_node.kind() else {
            return;
        };
        let heritage = class.heritage.as_ref().map(|h| &h.expression);
        let extends = class.heritage.is_some();
        match m.accessibility {
            Some(TSAccessibility::Protected | TSAccessibility::Private) => return,
            Some(TSAccessibility::Public) if extends => return,
            _ => {}
        }
        let params = &m.value.params;
        if params.items.iter().any(|p| {
            p.accessibility.is_some() || p.readonly || p.r#override || !p.decorators.is_empty()
        }) {
            return;
        }
        if class.is_declaration() && !class.decorators.is_empty() {
            return;
        }
        if heritage.is_some_and(|h| inherits_restricted_constructor(ctx, h)) {
            return;
        }
        let useless = if extends {
            is_redundant_super_call(params, body)
        } else {
            body.statements.is_empty() && body.directives.is_empty()
        };
        if useless {
            ctx.report(m.span, "Useless constructor.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn useless_constructor() {
        check(
            "S6647",
            "a.ts",
            r#"
class A {
  constructor() {}                    // Noncompliant {{Useless constructor.}}
}
class Base {}
class B extends Base {
  constructor(a, b) {                 // Noncompliant
    super(a, b);
  }
}
class C extends Base {
  constructor(...args) {              // Noncompliant
    super(...args);
  }
}
class D extends Base {
  constructor(a) {
    super(a, 1);
  }
}
class E {
  constructor(private x: number) {}
}
class F {
  private constructor() {}
}
class G extends Base {
  public constructor() {
    super();
  }
}
class H {
  constructor() {
    init();
  }
}
"#,
        );
    }
}
