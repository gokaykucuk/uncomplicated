//! Constructors should not contain asynchronous operations
use crate::helpers::types::from_annotation;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const PROMISE_STATICS: &[&str] = &[
    "resolve",
    "reject",
    "all",
    "race",
    "allSettled",
    "any",
    "try",
];

fn returns_promise(
    ctx: &Ctx<'_, '_>,
    is_async: bool,
    generator: bool,
    ret: Option<&TSTypeAnnotation<'_>>,
) -> bool {
    if generator {
        return false;
    }
    is_async || ret.is_some_and(|r| from_annotation(ctx, &r.type_annotation, 0).is_promise())
}

/// True if the call is known to return a promise.
fn is_thenable_call(ctx: &Ctx<'_, '_>, call: &CallExpression<'_>) -> bool {
    match &call.callee {
        Expression::StaticMemberExpression(m) => {
            let name = m.property.name.as_str();
            if let Expression::Identifier(obj) = &m.object
                && ctx.is_global(obj, "Promise")
            {
                return PROMISE_STATICS.contains(&name);
            }
            matches!(name, "then" | "catch" | "finally") && type_of(ctx, &m.object).is_promise()
        }
        Expression::Identifier(id) => {
            let Some(sym) = ctx.symbol_of(id) else {
                return id.name == "fetch";
            };
            match ctx.symbol_decl(sym).kind() {
                AstKind::Function(f) => {
                    returns_promise(ctx, f.r#async, f.generator, f.return_type.as_deref())
                }
                AstKind::VariableDeclarator(v) if !ctx.scoping().symbol_is_mutated(sym) => {
                    match v.init.as_ref().map(strip_ts) {
                        Some(Expression::ArrowFunctionExpression(a)) => {
                            returns_promise(ctx, a.r#async, false, a.return_type.as_deref())
                        }
                        Some(Expression::FunctionExpression(f)) => {
                            returns_promise(ctx, f.r#async, f.generator, f.return_type.as_deref())
                        }
                        _ => false,
                    }
                }
                _ => false,
            }
        }
        _ => false,
    }
}

fn is_statement(kind: &AstKind<'_>) -> bool {
    matches!(
        kind,
        AstKind::ExpressionStatement(_)
            | AstKind::BlockStatement(_)
            | AstKind::FunctionBody(_)
            | AstKind::IfStatement(_)
            | AstKind::ReturnStatement(_)
            | AstKind::ForStatement(_)
            | AstKind::ForInStatement(_)
            | AstKind::ForOfStatement(_)
            | AstKind::WhileStatement(_)
            | AstKind::DoWhileStatement(_)
            | AstKind::SwitchStatement(_)
            | AstKind::ThrowStatement(_)
            | AstKind::TryStatement(_)
            | AstKind::LabeledStatement(_)
            | AstKind::WithStatement(_)
    )
}

/// `this.x = Promise.resolve()`: an already-resolved promise used as a queue tail.
fn is_resolved_promise_sentinel(
    ctx: &Ctx<'_, '_>,
    call: &CallExpression<'_>,
    stmt: &AstKind<'_>,
) -> bool {
    let AstKind::ExpressionStatement(s) = stmt else {
        return false;
    };
    let Expression::AssignmentExpression(a) = &s.expression else {
        return false;
    };
    if a.operator != AssignmentOperator::Assign
        || strip_ts(&a.right).span() != call.span
        || !call.arguments.is_empty()
    {
        return false;
    }
    let target_ok = match &a.left {
        AssignmentTarget::StaticMemberExpression(m) => {
            matches!(m.object, Expression::ThisExpression(_))
        }
        AssignmentTarget::PrivateFieldExpression(m) => {
            matches!(m.object, Expression::ThisExpression(_))
        }
        AssignmentTarget::ComputedMemberExpression(m) => {
            matches!(m.object, Expression::ThisExpression(_))
                && matches!(
                    m.expression,
                    Expression::StringLiteral(_) | Expression::NumericLiteral(_)
                )
        }
        _ => false,
    };
    target_ok
        && matches!(&call.callee, Expression::StaticMemberExpression(m)
            if m.property.name == "resolve" && matches!(&m.object, Expression::Identifier(p) if ctx.is_global(p, "Promise")))
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        if !is_thenable_call(ctx, call) {
            return;
        }
        let mut statement: Option<AstKind<'a>> = None;
        let mut in_constructor = false;
        for anc in ctx.ancestors(node.id()) {
            let kind = anc.kind();
            if is_function(&kind) {
                in_constructor = matches!(ctx.parent_kind(anc.id()),
                    AstKind::MethodDefinition(m) if m.kind == MethodDefinitionKind::Constructor);
                break;
            }
            if statement.is_none() && is_statement(&kind) {
                statement = Some(kind);
            }
        }
        let Some(stmt) = statement else { return };
        if in_constructor && !is_resolved_promise_sentinel(ctx, call, &stmt) {
            ctx.report(
                stmt.span(),
                "Refactor this asynchronous operation outside of the constructor.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S7059",
            "a.ts",
            r#"
async function fetchData() { return 1; }
class A {
  constructor() {
    this.init = () => fetchData();
    this.data = null;
    this.tail = Promise.resolve();
    this['tail'] = Promise.resolve();
    this.#t = Promise.resolve();
    Promise.resolve().then(() => this.data = fetchData()); // Noncompliant {{Refactor this asynchronous operation outside of the constructor.}}
    this.tail = Promise.resolve(1); // Noncompliant
    this.tail = fetchData(); // Noncompliant
    other.tail = Promise.resolve(); // Noncompliant
    if (ready) {
      fetchData(); // Noncompliant
    }
  }
  #t: Promise<void>;
  async initialize() {
    this.data = await fetchData();
  }
  method() {
    fetchData();
  }
}
"#,
        );
    }
}
