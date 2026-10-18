//! Testing Library queries should match presence assertions
use crate::helpers::batch_tests::{
    Func, ancestors, is_direct_tl_binding, query_kind, testing_library_path,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// `expect(actual)[.not].matcher()` -> (actual, presence)
fn presence_assertion<'b, 'a>(call: &'b CallExpression<'a>) -> Option<(&'b Expression<'a>, bool)> {
    let Expression::StaticMemberExpression(m) = &call.callee else {
        return None;
    };
    if !call.arguments.is_empty() {
        return None;
    }
    let mut obj = &m.object;
    let mut negated = false;
    if let Expression::StaticMemberExpression(n) = obj
        && n.property.name == "not"
    {
        negated = true;
        obj = &n.object;
    }
    let Expression::CallExpression(e) = obj else {
        return None;
    };
    if e.arguments.len() != 1
        || !matches!(&e.callee, Expression::Identifier(id) if id.name == "expect")
    {
        return None;
    }
    let actual = e.arguments[0].as_expression()?;
    match m.property.name.as_str() {
        "toBeInTheDocument" | "toBeTruthy" | "toBeDefined" => Some((actual, !negated)),
        "toBeNull" | "toBeFalsy" => Some((actual, negated)),
        _ => None,
    }
}

fn get_or_query(name: &str) -> bool {
    matches!(query_kind(name), Some("get" | "query"))
}

/// The query call of `screen.getByX(...)`, `screen.getAllByX(...)[0]`, `screen.queryAllByX(...).length`.
fn query_call<'b, 'a>(actual: &'b Expression<'a>) -> Option<&'b CallExpression<'a>> {
    match strip_ts(actual) {
        Expression::CallExpression(c) => Some(c),
        Expression::ComputedMemberExpression(m) => {
            let Expression::CallExpression(c) = &m.object else {
                return None;
            };
            let all = matches!(&c.callee, Expression::StaticMemberExpression(p) if p.property.name.contains("AllBy"));
            let index = matches!(&m.expression, Expression::Identifier(_))
                || matches!(&m.expression, Expression::NumericLiteral(n) if n.value >= 0.0 && n.value.fract() == 0.0);
            (all && index).then_some(c.as_ref())
        }
        Expression::StaticMemberExpression(m) if m.property.name == "length" => {
            let Expression::CallExpression(c) = &m.object else {
                return None;
            };
            matches!(&c.callee, Expression::StaticMemberExpression(p) if p.property.name.contains("AllBy")).then_some(c.as_ref())
        }
        _ => None,
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
        let Some((actual, presence)) = presence_assertion(call) else {
            return;
        };
        let Some(query) = query_call(actual) else {
            return;
        };
        let Expression::StaticMemberExpression(qm) = &query.callee else {
            return;
        };
        let name = qm.property.name.as_str();
        if !get_or_query(name) || !is_direct_tl_binding(ctx, &qm.object, "screen", false, false) {
            return;
        }
        let is_get = name.starts_with("get");
        if presence == is_get || in_wait_for(ctx, node.id()) {
            return;
        }
        ctx.report(
            qm.property.span,
            if presence {
                "A presence assertion should use a `getBy*` query so a missing element produces Testing Library diagnostics."
            } else {
                "An absence assertion should use a `queryBy*` so it can observe a missing element."
            },
        );
    }
}

fn in_wait_for(ctx: &Ctx<'_, '_>, id: NodeId) -> bool {
    let mut prev: Option<Span> = None;
    for n in ancestors(ctx, id) {
        if let (AstKind::CallExpression(c), Some(p)) = (n.kind(), prev)
            && c.arguments
                .iter()
                .any(|a| a.span() == p && Func::of_arg(a).is_some())
            && testing_library_path(ctx, &c.callee).as_deref() == Some("waitFor")
        {
            return true;
        }
        prev = Some(n.kind().span());
    }
    false
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn presence() {
        check(
            "S9027",
            "a.test.jsx",
            r#"
import { screen, waitFor } from '@testing-library/react';
test('x', async () => {
  expect(screen.queryByText('Saved')).toBeInTheDocument(); // Noncompliant {{A presence assertion should use a `getBy*` query so a missing element produces Testing Library diagnostics.}}
  expect(screen.getByText('Error')).not.toBeInTheDocument(); // Noncompliant {{An absence assertion should use a `queryBy*` so it can observe a missing element.}}
  expect(screen.getByText('Error')).toBeNull(); // Noncompliant
  expect(screen.getAllByRole('row')[0]).not.toBeInTheDocument(); // Noncompliant
  expect(screen.getByText('Saved')).toBeInTheDocument();
  expect(screen.queryByText('Error')).not.toBeInTheDocument();
  await waitFor(() => expect(screen.queryByText('Saved')).toBeInTheDocument());
  expect(other.queryByText('Saved')).toBeInTheDocument();
});
"#,
        );
    }
}
