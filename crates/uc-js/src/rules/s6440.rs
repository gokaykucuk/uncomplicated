//! React Hooks should be properly called
//!
//! Follows `react-hooks/rules-of-hooks`. The code path analysis of the original rule is
//! approximated syntactically: a hook is conditional when it sits in a branch (if / ternary /
//! logical right operand / switch case / catch / default value) or after a `return` of its
//! function, and cyclic when it sits in a loop of its function.
use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const ORDER: &str = "React Hooks must be called in the exact same order in every component render.";
const WHERE: &str =
    "React Hooks must be called in a React function component or a custom React Hook function.";

/// `isHook` on a callee.
fn is_hook_callee(e: &Expression<'_>) -> bool {
    match e {
        Expression::Identifier(id) => is_hook_name(&id.name),
        Expression::StaticMemberExpression(m) => {
            is_hook_name(&m.property.name)
                && matches!(&m.object, Expression::Identifier(o) if is_component_name(&o.name)
                    // `Vue.use(...)`, `Router.use(...)` are not React hooks.
                    && (m.property.name != "use" || o.name == "React"))
        }
        _ => false,
    }
}

fn is_use_identifier(e: &Expression<'_>) -> bool {
    match e {
        Expression::Identifier(id) => id.name == "use",
        Expression::StaticMemberExpression(m) => {
            m.property.name == "use"
                && matches!(&m.object, Expression::Identifier(o) if o.name == "React")
        }
        _ => false,
    }
}

/// Name node of a function per `getFunctionName`, as (text, is identifier, identifier name).
enum FnName<'a> {
    Ident(&'a str),
    /// Member expression `a.b`: (source text, object, property)
    Member(&'a str, &'a str, &'a str),
    Other(&'a str),
}

impl FnName<'_> {
    fn text(&self) -> &str {
        match self {
            FnName::Ident(n) => n,
            FnName::Member(t, _, _) => t,
            FnName::Other(t) => t,
        }
    }

    fn is_component_or_hook(&self) -> bool {
        match self {
            FnName::Ident(n) => is_component_name(n) || is_hook_name(n),
            FnName::Member(_, o, p) => is_hook_name(p) && is_component_name(o),
            FnName::Other(_) => false,
        }
    }
}

fn function_name<'a>(ctx: &Ctx<'a, '_>, node: &AstNode<'a>) -> Option<FnName<'a>> {
    match node.kind() {
        AstKind::Function(f) if f.is_declaration() || f.id.is_some() => {
            f.id.as_ref().map(|id| FnName::Ident(id.name.as_str()))
        }
        AstKind::Function(_) | AstKind::ArrowFunctionExpression(_) => {
            let parent = estree_parent(ctx, node.id());
            match parent.kind() {
                AstKind::VariableDeclarator(v) => match &v.id {
                    BindingPattern::BindingIdentifier(id) => Some(FnName::Ident(id.name.as_str())),
                    p => Some(FnName::Other(ctx.text(p.span()))),
                },
                AstKind::AssignmentExpression(a) if a.operator == AssignmentOperator::Assign => {
                    match &a.left {
                        AssignmentTarget::AssignmentTargetIdentifier(id) => {
                            Some(FnName::Ident(id.name.as_str()))
                        }
                        AssignmentTarget::StaticMemberExpression(m) => match &m.object {
                            Expression::Identifier(o) => Some(FnName::Member(
                                ctx.text(m.span),
                                o.name.as_str(),
                                m.property.name.as_str(),
                            )),
                            _ => Some(FnName::Other(ctx.text(m.span))),
                        },
                        t => Some(FnName::Other(ctx.text(t.span()))),
                    }
                }
                AstKind::ObjectProperty(p) if !p.computed => match &p.key {
                    PropertyKey::StaticIdentifier(id) => Some(FnName::Ident(id.name.as_str())),
                    k => Some(FnName::Other(ctx.text(k.span()))),
                },
                AstKind::AssignmentPattern(p) => match &p.left {
                    BindingPattern::BindingIdentifier(id) => Some(FnName::Ident(id.name.as_str())),
                    l => Some(FnName::Other(ctx.text(l.span()))),
                },
                _ => None,
            }
        }
        _ => None,
    }
}

fn is_react_fn(e: &Expression<'_>, name: &str) -> bool {
    match e {
        Expression::Identifier(id) => id.name == name,
        Expression::StaticMemberExpression(m) => {
            m.property.name == name
                && matches!(&m.object, Expression::Identifier(o) if o.name == "React")
        }
        _ => false,
    }
}

/// `isForwardRefCallback || isMemoCallback`.
fn is_wrapper_callback<'a>(ctx: &Ctx<'a, '_>, node: &AstNode<'a>) -> bool {
    match estree_parent(ctx, node.id()).kind() {
        AstKind::CallExpression(c) => {
            is_react_fn(&c.callee, "forwardRef") || is_react_fn(&c.callee, "memo")
        }
        AstKind::NewExpression(c) => {
            is_react_fn(&c.callee, "forwardRef") || is_react_fn(&c.callee, "memo")
        }
        _ => false,
    }
}

fn is_inside_component_or_hook<'a>(ctx: &Ctx<'a, '_>, node: &AstNode<'a>) -> bool {
    std::iter::once(node)
        .chain(ancestors(ctx, node.id()))
        .any(|n| {
            function_name(ctx, n).is_some_and(|f| f.is_component_or_hook())
                || (is_function(&n.kind()) && is_wrapper_callback(ctx, n))
        })
}

fn contains(outer: Span, inner: Span) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

fn always_throws(s: &Statement<'_>) -> bool {
    match s {
        Statement::ThrowStatement(_) => true,
        Statement::BlockStatement(b) => b.body.last().is_some_and(always_throws),
        _ => false,
    }
}

#[derive(Default)]
struct Flow {
    conditional: bool,
    cycled: bool,
}

/// Control-flow position of `call` within its function `func` (exclusive).
fn flow<'a>(ctx: &Ctx<'a, '_>, call: &AstNode<'a>, func: Option<NodeId>) -> Flow {
    let span = call.kind().span();
    let mut f = Flow::default();
    for a in ancestors(ctx, call.id()) {
        if Some(a.id()) == func {
            break;
        }
        match a.kind() {
            AstKind::IfStatement(i) => {
                if contains(i.consequent.span(), span) {
                    f.conditional |= !i.alternate.as_ref().is_some_and(always_throws);
                } else if i
                    .alternate
                    .as_ref()
                    .is_some_and(|alt| contains(alt.span(), span))
                {
                    f.conditional |= !always_throws(&i.consequent);
                }
            }
            AstKind::ConditionalExpression(c) => f.conditional |= !contains(c.test.span(), span),
            AstKind::LogicalExpression(l) => f.conditional |= contains(l.right.span(), span),
            AstKind::SwitchStatement(s) => {
                if !contains(s.discriminant.span(), span) {
                    let first_test = s
                        .cases
                        .first()
                        .and_then(|c| c.test.as_ref())
                        .map(GetSpan::span);
                    f.conditional |= !first_test.is_some_and(|t| contains(t, span));
                }
            }
            AstKind::ForStatement(x) => {
                f.cycled |= !x.init.as_ref().is_some_and(|i| contains(i.span(), span))
            }
            AstKind::ForInStatement(x) => f.cycled |= !contains(x.right.span(), span),
            AstKind::ForOfStatement(x) => f.cycled |= !contains(x.right.span(), span),
            AstKind::WhileStatement(_) | AstKind::DoWhileStatement(_) => f.cycled = true,
            AstKind::AssignmentPattern(p) => f.conditional |= contains(p.right.span(), span),
            AstKind::AssignmentTargetWithDefault(p) => {
                f.conditional |= contains(p.init.span(), span)
            }
            AstKind::FormalParameter(p) => {
                f.conditional |= p
                    .initializer
                    .as_ref()
                    .is_some_and(|i| contains(i.span(), span));
            }
            AstKind::CatchClause(c) => f.conditional |= contains(c.body.span, span),
            AstKind::ChainExpression(_) => {}
            _ => {}
        }
    }
    f
}

/// Is there a `return` (or, for the message only, a `throw`) before `pos` in the function?
fn early_exit_before(stmts: &[Statement<'_>], pos: u32, include_throw: bool) -> bool {
    fn walk(s: &Statement<'_>, pos: u32, include_throw: bool) -> bool {
        if s.span().start >= pos {
            return false;
        }
        match s {
            Statement::ReturnStatement(r) => r.span.end <= pos,
            Statement::ThrowStatement(t) => include_throw && t.span.end <= pos,
            Statement::BlockStatement(b) => b.body.iter().any(|s| walk(s, pos, include_throw)),
            Statement::IfStatement(i) => {
                walk(&i.consequent, pos, include_throw)
                    || i.alternate
                        .as_ref()
                        .is_some_and(|a| walk(a, pos, include_throw))
            }
            Statement::ForStatement(x) => walk(&x.body, pos, include_throw),
            Statement::ForInStatement(x) => walk(&x.body, pos, include_throw),
            Statement::ForOfStatement(x) => walk(&x.body, pos, include_throw),
            Statement::WhileStatement(x) => walk(&x.body, pos, include_throw),
            Statement::DoWhileStatement(x) => walk(&x.body, pos, include_throw),
            Statement::LabeledStatement(x) => walk(&x.body, pos, include_throw),
            Statement::SwitchStatement(x) => x
                .cases
                .iter()
                .any(|c| c.consequent.iter().any(|s| walk(s, pos, include_throw))),
            Statement::TryStatement(t) => {
                t.block.body.iter().any(|s| walk(s, pos, include_throw))
                    || t.handler
                        .as_ref()
                        .is_some_and(|h| h.body.body.iter().any(|s| walk(s, pos, include_throw)))
                    || t.finalizer
                        .as_ref()
                        .is_some_and(|b| b.body.iter().any(|s| walk(s, pos, include_throw)))
            }
            _ => false,
        }
    }
    // A `return` directly in the body makes the hook unreachable: not reported upstream.
    stmts
        .iter()
        .any(|s| !matches!(s, Statement::ReturnStatement(_)) && walk(s, pos, include_throw))
}

fn unreachable(stmts: &[Statement<'_>], pos: u32) -> bool {
    stmts
        .iter()
        .any(|s| matches!(s, Statement::ReturnStatement(r) if r.span.end <= pos))
}

/// Sonar exemption: the function is assigned to a variable typed `FC` / `FunctionComponent`.
fn is_typed_as_fc<'a>(ctx: &Ctx<'a, '_>, func: &AstNode<'a>) -> bool {
    let AstKind::VariableDeclarator(v) = ctx.parent_kind(func.id()) else {
        return false;
    };
    let Some(ann) = &v.type_annotation else {
        return false;
    };
    let TSType::TSTypeReference(r) = &ann.type_annotation else {
        return false;
    };
    let fc = |n: &str| n == "FC" || n == "FunctionComponent";
    match &r.type_name {
        TSTypeName::IdentifierReference(id) => fc(&id.name),
        TSTypeName::QualifiedName(q) => {
            matches!(&q.left, TSTypeName::IdentifierReference(l) if l.name == "React")
                && fc(&q.right.name)
        }
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        if !is_hook_callee(&call.callee) || !file_uses_react(ctx) {
            return;
        }
        let hook = &call.callee;
        let name = ctx.text(hook.span());
        let span = hook.span();
        let is_use = is_use_identifier(hook);

        // The code path owning the call: nearest function, or a class field initializer.
        let mut func = None;
        let mut class_field = false;
        for a in ancestors(ctx, node.id()) {
            match a.kind() {
                AstKind::Function(_) | AstKind::ArrowFunctionExpression(_) => {
                    func = Some(a);
                    break;
                }
                AstKind::PropertyDefinition(p)
                    if p.value.as_ref().is_some_and(|v| contains(v.span(), span)) =>
                {
                    class_field = true;
                    break;
                }
                _ => {}
            }
        }

        if is_use
            && ancestors(ctx, node.id())
                .any(|a| matches!(a.kind(), AstKind::TryStatement(_) | AstKind::CatchClause(_)))
        {
            ctx.report(
                span,
                format!("React Hook \"{name}\" cannot be called in a try/catch block."),
            );
        }
        let fl = flow(ctx, node, func.map(|f| f.id()));
        let in_do_while =
            ancestors(ctx, node.id()).any(|a| matches!(a.kind(), AstKind::DoWhileStatement(_)));
        if (fl.cycled || in_do_while) && !is_use {
            ctx.report(
                span,
                format!("React Hook \"{name}\" may be executed more than once. Possibly because it is called in a loop. {ORDER}"),
            );
        }

        if class_field {
            ctx.report(
                span,
                format!("React Hook \"{name}\" cannot be called in a class component. {WHERE}"),
            );
            return;
        }
        let Some(func) = func else {
            ctx.report(
                span,
                format!("React Hook \"{name}\" cannot be called at the top level. {WHERE}"),
            );
            return;
        };
        let fname = function_name(ctx, func);
        let direct = match &fname {
            Some(n) => n.is_component_or_hook(),
            None => is_wrapper_callback(ctx, func),
        };
        if direct {
            let is_async = match func.kind() {
                AstKind::Function(f) => f.r#async,
                AstKind::ArrowFunctionExpression(a) => a.r#async,
                _ => false,
            };
            if is_async {
                ctx.report(
                    span,
                    format!("React Hook \"{name}\" cannot be called in an async function."),
                );
            }
            let stmts = function_statements(&func.kind()).unwrap_or(&[]);
            if unreachable(stmts, span.start) {
                return;
            }
            let early_return = early_exit_before(stmts, span.start, false);
            if !fl.cycled && (fl.conditional || early_return) && !is_use && !in_do_while {
                let suffix = if early_return || early_exit_before(stmts, span.start, true) {
                    " Did you accidentally call a React Hook after an early return?"
                } else {
                    ""
                };
                ctx.report(
                    span,
                    format!("React Hook \"{name}\" is called conditionally. {ORDER}{suffix}"),
                );
            }
            return;
        }
        let parent = ctx.parent(func.id());
        let is_class_member = match parent.kind() {
            AstKind::MethodDefinition(_) => true,
            AstKind::PropertyDefinition(p) => p
                .value
                .as_ref()
                .is_some_and(|v| strip_parens(v).span() == func.kind().span()),
            _ => false,
        };
        if is_class_member {
            ctx.report(
                span,
                format!("React Hook \"{name}\" cannot be called in a class component. {WHERE}"),
            );
        } else if let Some(n) = fname {
            if is_typed_as_fc(ctx, func) {
                return;
            }
            ctx.report(
                span,
                format!(
                    "React Hook \"{name}\" is called in function \"{}\" that is neither a React function component nor a custom React Hook function. React component names must start with an uppercase letter. React Hook names must start with the word \"use\".",
                    n.text()
                ),
            );
        } else if !is_use && is_inside_component_or_hook(ctx, func) {
            ctx.report(
                span,
                format!("React Hook \"{name}\" cannot be called inside a callback. {WHERE}"),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn conditional() {
        check(
            "S6440",
            "a.js",
            r#"
import { useEffect, useState } from 'react';

function Form() {
  const [name, setName] = useState('Mary');
  if (name !== '') {
    useEffect(function persistForm() { // Noncompliant {{React Hook "useEffect" is called conditionally. React Hooks must be called in the exact same order in every component render.}}
      console.log('persistForm');
    });
  }
  const x = cond ? useState(1) : 2; // Noncompliant
  cond && useFoo(); // Noncompliant
  useBar() || other;
  return <button onClick={() => setName("Poppins")}>Supercali...</button>;
}

function Early() {
  if (a) return null;
  useState(); // Noncompliant {{React Hook "useState" is called conditionally. React Hooks must be called in the exact same order in every component render. Did you accidentally call a React Hook after an early return?}}
}

function Throws() {
  if (!a) throw new Error();
  useState();
  if (b) { useState(); } else { throw new Error(); }
}

function useLoop() {
  for (const x of xs) {
    useState(); // Noncompliant {{React Hook "useState" may be executed more than once. Possibly because it is called in a loop. React Hooks must be called in the exact same order in every component render.}}
  }
  while (c) { if (d) useMemo(); } // Noncompliant
}

function UseApi() {
  if (a) { use(ctx); }
  React.useState();
}
"#,
        );
    }

    #[test]
    fn placement() {
        check(
            "S6440",
            "a.tsx",
            r#"
import React, { useState, type FC } from 'react';
const top = useState(); // Noncompliant {{React Hook "useState" cannot be called at the top level. React Hooks must be called in a React function component or a custom React Hook function.}}
function getState() {
  const [val] = useState(0); // Noncompliant {{React Hook "useState" is called in function "getState" that is neither a React function component nor a custom React Hook function. React component names must start with an uppercase letter. React Hook names must start with the word "use".}}
  return val;
}
const _Message: FC<{}> = () => {
  const [v] = useState(0);
  return <div />;
};
const _helper = () => useState(0); // Noncompliant
class C extends React.Component {
  render() {
    useState(); // Noncompliant {{React Hook "useState" cannot be called in a class component. React Hooks must be called in a React function component or a custom React Hook function.}}
    return null;
  }
}
function Comp() {
  useEffect(() => {
    useState(); // Noncompliant {{React Hook "useState" cannot be called inside a callback. React Hooks must be called in a React function component or a custom React Hook function.}}
  });
  return null;
}
const F = React.forwardRef((props, ref) => {
  const [a] = useState();
  return <div ref={ref} />;
});
const M = memo(function () { useState(); return null; });
async function useAsync() {
  useState(); // Noncompliant {{React Hook "useState" cannot be called in an async function.}}
}
it('works', () => {
  renderHook(() => useCounter());
});
Vue.use(Plugin);
"#,
        );
    }
}
