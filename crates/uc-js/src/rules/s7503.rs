//! Async functions should use async features
use oxc_ast_visit::{Visit, walk};
use oxc_syntax::scope::ScopeFlags;

use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// True if a value of this type can't be a thenable.
fn not_thenable(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    type_of(ctx, e).all(non_thenable_ty)
}

/// Looks for `await`, `for await`, `await using` and possibly-thenable returns, not entering
/// nested functions.
struct Scan<'c, 'a, 's> {
    ctx: &'c Ctx<'a, 's>,
    async_feature: bool,
}

impl<'a> Visit<'a> for Scan<'_, 'a, '_> {
    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}
    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}

    fn visit_await_expression(&mut self, _: &AwaitExpression<'a>) {
        self.async_feature = true;
    }

    fn visit_for_of_statement(&mut self, f: &ForOfStatement<'a>) {
        if f.r#await {
            self.async_feature = true;
        }
        walk::walk_for_of_statement(self, f);
    }

    fn visit_variable_declaration(&mut self, d: &VariableDeclaration<'a>) {
        if d.kind == VariableDeclarationKind::AwaitUsing {
            self.async_feature = true;
        }
        walk::walk_variable_declaration(self, d);
    }

    fn visit_return_statement(&mut self, r: &ReturnStatement<'a>) {
        if let Some(arg) = &r.argument
            && !not_thenable(self.ctx, arg)
        {
            // Unknown or thenable type: upstream may consider this an async feature.
            self.async_feature = true;
        }
        walk::walk_return_statement(self, r);
    }
}

fn skip_parens_parent<'s, 'a>(ctx: &Ctx<'a, 's>, id: NodeId) -> &'s AstNode<'a> {
    let mut p = ctx.parent(id);
    while let AstKind::ParenthesizedExpression(_) = p.kind() {
        p = ctx.parent(p.id());
    }
    p
}

fn static_key_name(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
        PropertyKey::StringLiteral(s) => Some(s.value.to_string()),
        PropertyKey::NumericLiteral(n) => Some(crate::helpers::batch_ts::fmt_number(n.value)),
        PropertyKey::TemplateLiteral(t) if t.expressions.is_empty() => t
            .quasis
            .first()
            .and_then(|q| q.value.cooked.as_ref())
            .map(|c| c.to_string()),
        _ => None,
    }
}

/// `getFunctionNameWithKind` from `@eslint-community/eslint-utils`, for async non-generator functions.
fn name_with_kind(
    ctx: &Ctx<'_, '_>,
    parent: &AstNode<'_>,
    is_arrow: bool,
    id: Option<&BindingIdentifier<'_>>,
) -> String {
    let mut tokens: Vec<String> = Vec::new();
    let (is_static, key) = match parent.kind() {
        AstKind::MethodDefinition(m) => (m.r#static, Some(&m.key)),
        AstKind::PropertyDefinition(p) => (p.r#static, Some(&p.key)),
        AstKind::ObjectProperty(p) => (false, Some(&p.key)),
        _ => (false, None),
    };
    if matches!(
        parent.kind(),
        AstKind::MethodDefinition(_) | AstKind::PropertyDefinition(_)
    ) {
        if is_static {
            tokens.push("static".into());
        }
        if matches!(key, Some(PropertyKey::PrivateIdentifier(_))) {
            tokens.push("private".into());
        }
    }
    tokens.push("async".into());
    match parent.kind() {
        AstKind::MethodDefinition(m) => tokens.push(
            match m.kind {
                MethodDefinitionKind::Get => "getter",
                MethodDefinitionKind::Set => "setter",
                _ => "method",
            }
            .into(),
        ),
        AstKind::ObjectProperty(p) => tokens.push(
            match p.kind {
                PropertyKind::Get => "getter",
                PropertyKind::Set => "setter",
                PropertyKind::Init => "method",
            }
            .into(),
        ),
        AstKind::PropertyDefinition(_) => tokens.push("method".into()),
        _ => {
            if is_arrow {
                tokens.push("arrow".into());
            }
            tokens.push("function".into());
        }
    }
    match (key, parent.kind()) {
        (Some(PropertyKey::PrivateIdentifier(p)), _) => tokens.push(format!("#{}", p.name)),
        (Some(k), _) => {
            if let Some(name) = static_key_name(k) {
                tokens.push(format!("'{name}'"));
            } else {
                let text = ctx.text(k.span());
                if !text.contains('\n') {
                    tokens.push(format!("[{text}]"));
                }
            }
        }
        (None, kind) => {
            if let Some(id) = id {
                tokens.push(format!("'{}'", id.name));
            } else {
                match kind {
                    AstKind::VariableDeclarator(v) => {
                        if let BindingPattern::BindingIdentifier(b) = &v.id {
                            tokens.push(format!("'{}'", b.name));
                        }
                    }
                    AstKind::AssignmentExpression(a) => {
                        if let AssignmentTarget::AssignmentTargetIdentifier(i) = &a.left {
                            tokens.push(format!("'{}'", i.name));
                        }
                    }
                    AstKind::AssignmentPattern(a) => {
                        if let BindingPattern::BindingIdentifier(b) = &a.left {
                            tokens.push(format!("'{}'", b.name));
                        }
                    }
                    AstKind::ExportDefaultDeclaration(_) => tokens.push("'default'".into()),
                    _ => {}
                }
            }
        }
    }
    let s = tokens.join(" ");
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => s,
    }
}

/// Start of the function head: the member (after decorators) or the function itself.
fn head_start(ctx: &Ctx<'_, '_>, parent: &AstNode<'_>) -> Option<u32> {
    let decorators = match parent.kind() {
        AstKind::MethodDefinition(m) => &m.decorators,
        AstKind::PropertyDefinition(p) => &p.decorators,
        AstKind::ObjectProperty(p) => return Some(p.span.start),
        _ => return None,
    };
    Some(match decorators.last() {
        Some(d) => token_after(ctx, d.span.end).map_or(parent.span().start, |t| t.start),
        None => parent.span().start,
    })
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Function, AstType::ArrowFunctionExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        if crate::helpers::batch_ts::no_type_info(ctx) {
            return;
        }
        let parent = skip_parens_parent(ctx, node.id());
        let mut scan = Scan {
            ctx,
            async_feature: false,
        };
        let (name, head) = match node.kind() {
            AstKind::Function(f) => {
                if !f.r#async || f.generator {
                    return;
                }
                let Some(body) = &f.body else { return };
                if body.statements.is_empty() {
                    return;
                }
                for s in &body.statements {
                    scan.visit_statement(s);
                }
                let start = head_start(ctx, parent).unwrap_or(f.span.start);
                (
                    name_with_kind(ctx, parent, false, f.id.as_ref()),
                    Span::new(start, f.params.span.start.max(start)),
                )
            }
            AstKind::ArrowFunctionExpression(a) => {
                if !a.r#async {
                    return;
                }
                // Upstream checks every direct child of an async arrow (notably its parameters)
                // for a thenable type: annotated parameters must not look like promises.
                let params_ok = a.params.items.iter().all(|p| {
                    p.type_annotation
                        .as_ref()
                        .is_none_or(|t| param_type_ok(ctx, &t.type_annotation))
                });
                if !params_ok {
                    return;
                }
                match a.body.as_function_body() {
                    Some(body) => {
                        if body.statements.is_empty() {
                            return;
                        }
                        for s in &body.statements {
                            scan.visit_statement(s);
                        }
                    }
                    None => match a.body.as_expression() {
                        Some(e) => {
                            if matches!(strip_parens(e), Expression::AwaitExpression(_))
                                || !not_thenable(ctx, e)
                            {
                                return;
                            }
                            scan.visit_expression(e);
                        }
                        None => return,
                    },
                }
                let head = match head_start(ctx, parent) {
                    Some(start) => Span::new(start, a.params.span.start.max(start)),
                    None => function_report_span(ctx, node),
                };
                (name_with_kind(ctx, parent, true, None), head)
            }
            _ => return,
        };
        if !scan.async_feature {
            ctx.report(head, format!("{name} has no 'await' expression."));
        }
    }
}

/// Parameter annotation that can't be a thenable: known non-thenable types, `any`, or a named
/// type whose name doesn't suggest a promise.
fn param_type_ok(ctx: &Ctx<'_, '_>, t: &TSType<'_>) -> bool {
    let ty = helpers::types::from_annotation(ctx, t, 0);
    if ty == Ty::Any {
        return true;
    }
    ty.all(|t| match t {
        Ty::Named(n) => ![
            "Promise",
            "Thenable",
            "Deferred",
            "Future",
            "Awaitable",
            "Task",
            "Observable",
        ]
        .iter()
        .any(|w| n.contains(w)),
        t => non_thenable_ty(t),
    })
}

fn non_thenable_ty(t: &Ty) -> bool {
    {
        matches!(
            t,
            Ty::String
                | Ty::Number
                | Ty::BigInt
                | Ty::Boolean
                | Ty::Null
                | Ty::Void
                | Ty::Undefined
                | Ty::Symbol
                | Ty::Array
                | Ty::Tuple
                | Ty::Function
                | Ty::RegExp
                | Ty::Date
                | Ty::Map
                | Ty::Set
                | Ty::Error
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn require_await() {
        check(
            "S7503",
            "a.ts",
            r#"
async function foo() { // Noncompliant {{Async function 'foo' has no 'await' expression.}}
  doSomething();
}
async function bar() {
  await doSomething();
}
async function empty() {}
async function returnsUnknown() {
  return doSomething();
}
async function returnsNumber() { // Noncompliant
  return 42;
}
const arrow = async () => { // Noncompliant {{Async arrow function 'arrow' has no 'await' expression.}}
  doSomething();
};
const exprArrow = async () => 1; // Noncompliant
const exprArrow2 = async () => fetchIt();
class C {
  async m() { // Noncompliant {{Async method 'm' has no 'await' expression.}}
    doSomething();
  }
  static async s() { // Noncompliant {{Static async method 's' has no 'await' expression.}}
    doSomething();
  }
  async #p() { // Noncompliant {{Private async method #p has no 'await' expression.}}
    doSomething();
  }
  prop = async () => { // Noncompliant {{Async method 'prop' has no 'await' expression.}}
    doSomething();
  };
}
const o = {
  async k() { // Noncompliant {{Async method 'k' has no 'await' expression.}}
    doSomething();
  },
};
async function nested() { // Noncompliant
  const f = async () => { await x; };
  f();
}
async function forAwait() {
  for await (const x of xs) {}
}
async function* gen() { yield 1; }
xs.map(async (p) => { doSomething(p); }); // Noncompliant
ps.map(async (p: Promise<number>) => { doSomething(p); });
const typed = async (n: number) => { doSomething(n); }; // Noncompliant
const ret = async (): Promise<void> => { doSomething(); }; // Noncompliant
"#,
        );
    }
}
