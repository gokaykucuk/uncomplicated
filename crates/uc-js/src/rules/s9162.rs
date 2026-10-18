//! Cypress assertions should be retryable
use crate::helpers::batch_tests::{Func, chain_base, is_global_named, is_test_related_file};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const DOM_QUERIES: &[&str] = &[
    "children",
    "closest",
    "contains",
    "eq",
    "filter",
    "find",
    "first",
    "focused",
    "get",
    "last",
    "next",
    "nextAll",
    "nextUntil",
    "not",
    "parent",
    "parents",
    "parentsUntil",
    "prev",
    "prevAll",
    "prevUntil",
    "root",
    "shadow",
    "siblings",
];
const CHAI_LANGUAGE: &[&str] = &[
    "to", "be", "been", "is", "that", "which", "and", "has", "have", "with", "at", "of", "same",
    "but", "does", "still", "not", "deep", "nested", "own", "ordered", "any", "all", "itself",
    "also",
];
const CHAI_MATCHERS: &[&str] = &[
    "equal",
    "equals",
    "eq",
    "eql",
    "include",
    "includes",
    "contain",
    "contains",
    "match",
    "matches",
    "lengthOf",
    "length",
    "property",
    "above",
    "gt",
    "below",
    "lt",
    "least",
    "gte",
    "most",
    "lte",
    "within",
    "a",
    "an",
    "string",
    "members",
    "oneOf",
    "closeTo",
    "true",
    "false",
    "null",
    "undefined",
    "ok",
    "empty",
    "exist",
    "visible",
    "class",
    "attr",
    "text",
    "value",
    "html",
    "checked",
    "disabled",
    "enabled",
    "selected",
    "css",
    "data",
    "id",
];

#[derive(Clone, Copy, PartialEq)]
enum Safe {
    Value,
    Yielded,
    Str,
    Record,
}

fn safe_kind(e: &Expression<'_>, bindings: &FxHashMap<&str, Safe>) -> Option<Safe> {
    match strip_ts(e) {
        Expression::StringLiteral(_) => Some(Safe::Str),
        Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::RegExpLiteral(_) => Some(Safe::Value),
        Expression::Identifier(id) => bindings.get(id.name.as_str()).copied(),
        Expression::TemplateLiteral(t) => t
            .expressions
            .iter()
            .all(|e| safe_kind(e, bindings).is_some())
            .then_some(Safe::Str),
        Expression::UnaryExpression(u) => (u.operator != UnaryOperator::Delete
            && safe_kind(&u.argument, bindings).is_some())
        .then_some(Safe::Value),
        Expression::BinaryExpression(b) => (safe_kind(&b.left, bindings).is_some()
            && safe_kind(&b.right, bindings).is_some())
        .then_some(Safe::Value),
        Expression::LogicalExpression(b) => (safe_kind(&b.left, bindings).is_some()
            && safe_kind(&b.right, bindings).is_some())
        .then_some(Safe::Value),
        Expression::ConditionalExpression(c) => [&c.test, &c.consequent, &c.alternate]
            .iter()
            .all(|e| safe_kind(e, bindings).is_some())
            .then_some(Safe::Value),
        Expression::ArrayExpression(a) => a
            .elements
            .iter()
            .all(|el| match el {
                ArrayExpressionElement::Elision(_) => true,
                ArrayExpressionElement::SpreadElement(_) => false,
                el => el
                    .as_expression()
                    .is_some_and(|e| safe_kind(e, bindings).is_some()),
            })
            .then_some(Safe::Record),
        Expression::ObjectExpression(o) => o
            .properties
            .iter()
            .all(|p| match p {
                ObjectPropertyKind::ObjectProperty(p) => {
                    p.kind == PropertyKind::Init
                        && !p.computed
                        && !p.method
                        && property_key_name(&p.key) != Some("__proto__")
                        && safe_kind(&p.value, bindings).is_some()
                }
                ObjectPropertyKind::SpreadProperty(_) => false,
            })
            .then_some(Safe::Record),
        Expression::StaticMemberExpression(m) => {
            (safe_kind(&m.object, bindings) == Some(Safe::Record)).then_some(Safe::Value)
        }
        Expression::CallExpression(c) => {
            let Expression::StaticMemberExpression(m) = &c.callee else {
                return None;
            };
            if !c.arguments.is_empty() {
                return None;
            }
            let receiver = safe_kind(&m.object, bindings);
            match m.property.name.as_str() {
                "text" if receiver == Some(Safe::Yielded) => Some(Safe::Str),
                "trim" | "toLowerCase" | "toUpperCase" if receiver == Some(Safe::Str) => {
                    Some(Safe::Str)
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn uses_yielded(ctx: &Ctx<'_, '_>, e: &Expression<'_>, yielded: &FxHashSet<&str>) -> bool {
    ctx.tokens_in(e.span())
        .iter()
        .any(|t| t.kind() == Kind::Ident && yielded.contains(ctx.text(t.span())))
}

fn is_selector(a: &Argument<'_>) -> bool {
    match a {
        Argument::StringLiteral(s) => !s.value.starts_with('@'),
        Argument::TemplateLiteral(t) => t
            .quasis
            .first()
            .and_then(|q| q.value.cooked.as_ref())
            .is_some_and(|c| !c.is_empty() && !c.starts_with('@')),
        _ => false,
    }
}

fn is_retryable_query_chain<'a>(ctx: &Ctx<'a, '_>, e: &Expression<'a>) -> bool {
    let empty = FxHashMap::default();
    let mut cur = e;
    let mut saw = false;
    loop {
        match strip_ts(cur) {
            Expression::CallExpression(c) if !c.optional => {
                let Expression::StaticMemberExpression(m) = &c.callee else {
                    return false;
                };
                let name = m.property.name.as_str();
                if m.optional || !DOM_QUERIES.contains(&name) {
                    return false;
                }
                let args_ok = c.arguments.iter().enumerate().all(|(i, a)| {
                    if name == "get" && i == 0 {
                        is_selector(a)
                    } else {
                        a.as_expression()
                            .is_some_and(|e| safe_kind(e, &empty).is_some())
                    }
                });
                if !args_ok {
                    return false;
                }
                saw = true;
                cur = &m.object;
            }
            other => return saw && is_global_named(ctx, other, "cy"),
        }
    }
}

/// `expect(x).to.equal(y)` / `assert.equal(x, y)` with retry-safe arguments; returns whether the
/// assertion uses a yielded binding.
fn chai_assertion<'a>(
    ctx: &Ctx<'a, '_>,
    e: &Expression<'a>,
    bindings: &FxHashMap<&str, Safe>,
    yielded: &FxHashSet<&str>,
) -> Option<bool> {
    let base = chain_base(e);
    let Expression::Identifier(root) = base else {
        return None;
    };
    if ctx.symbol_of(root).is_some() || !matches!(root.name.as_str(), "expect" | "assert") {
        return None;
    }
    // walk the chain checking shapes and argument safety
    let mut cur = e;
    let mut segments: Vec<&str> = Vec::new();
    let mut uses = false;
    loop {
        match cur {
            Expression::CallExpression(c) => {
                for a in &c.arguments {
                    let ex = a.as_expression()?;
                    safe_kind(ex, bindings)?;
                    uses |= uses_yielded(ctx, ex, yielded);
                }
                cur = &c.callee;
            }
            Expression::StaticMemberExpression(m) => {
                segments.push(m.property.name.as_str());
                cur = &m.object;
            }
            Expression::Identifier(_) => break,
            _ => return None,
        }
    }
    let (matcher, rest) = segments.split_first()?;
    let shape_ok = if root.name == "assert" {
        rest.is_empty()
    } else {
        CHAI_MATCHERS.contains(matcher) && rest.iter().all(|s| CHAI_LANGUAGE.contains(s))
    };
    shape_ok.then_some(uses)
}

fn is_assertion_only_callback<'a>(ctx: &Ctx<'a, '_>, f: Func<'_, 'a>) -> bool {
    if f.is_async() || f.is_generator() || f.param_count() != 1 {
        return false;
    }
    let Some(param) = f.first_ident_param() else {
        return false;
    };
    let Some(body) = f.block() else { return false };
    if body.statements.is_empty() {
        return false;
    }
    let mut bindings: FxHashMap<&str, Safe> = FxHashMap::default();
    let mut yielded: FxHashSet<&str> = FxHashSet::default();
    bindings.insert(param.name.as_str(), Safe::Yielded);
    yielded.insert(param.name.as_str());
    let mut count = 0;
    let mut has_yielded = false;
    for s in &body.statements {
        match s {
            Statement::VariableDeclaration(v) => {
                if v.kind != VariableDeclarationKind::Const {
                    return false;
                }
                for d in &v.declarations {
                    let (BindingPattern::BindingIdentifier(id), Some(init)) = (&d.id, &d.init)
                    else {
                        return false;
                    };
                    let Some(kind) = safe_kind(init, &bindings) else {
                        return false;
                    };
                    bindings.insert(id.name.as_str(), kind);
                    if uses_yielded(ctx, init, &yielded) {
                        yielded.insert(id.name.as_str());
                    }
                }
            }
            Statement::ExpressionStatement(e) => {
                let Some(uses) = chai_assertion(ctx, &e.expression, &bindings, &yielded) else {
                    return false;
                };
                has_yielded |= uses;
                count += 1;
            }
            _ => return false,
        }
    }
    count > 0 && has_yielded
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let Expression::StaticMemberExpression(m) = &call.callee else {
            return;
        };
        if m.property.name != "then"
            || m.optional
            || call.optional
            || call.type_arguments.is_some()
            || call.arguments.len() != 1
        {
            return;
        }
        let Some(cb) = Func::of_arg(&call.arguments[0]) else {
            return;
        };
        if is_test_related_file(ctx.file.path)
            && is_retryable_query_chain(ctx, &m.object)
            && is_assertion_only_callback(ctx, cb)
        {
            ctx.report(
                m.property.span,
                "Assertions in `.then()` run only once; use `.should()` to retry them and avoid flaky tests.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn retryable() {
        check(
            "S9162",
            "cypress/e2e/status.cy.js",
            r#"
cy.get('[data-cy=status]').then($status => { // Noncompliant {{Assertions in `.then()` run only once; use `.should()` to retry them and avoid flaky tests.}}
  expect($status.text()).to.equal('Ready');
});
cy.get('input').then($input => { // Noncompliant
  const text = $input.text(), expected = 'Ready';
  expect(text).to.equal(expected);
});
cy.get('input').then($input => {
  initializePlugin($input[0]);
});
cy.get('input').then($input => {
  let text = $input.text();
  expect(text).to.equal('Ready');
});
cy.get('input').then(({ value }) => {
  expect(value).to.equal('Ready');
});
cy.get('input').then($input => {
  const text = source.text;
  expect($input.text()).to.equal(text);
});
cy.get('@alias').then($input => {
  expect($input.text()).to.equal('Ready');
});
cy.request('/api').then(res => {
  expect(res.status).to.equal(200);
});
"#,
        );
        check(
            "S9162",
            "src/status.js",
            "cy.get('x').then($s => { expect($s.text()).to.equal('R'); });",
        );
    }
}
