//! React components should not render non-boolean condition values
use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MSG: &str = "Convert the conditional to a boolean to avoid leaked value";

/// Upstream sets its react-native flag when the import is visited, so only imports placed
/// before the expression count.
fn uses_react_native(ctx: &Ctx<'_, '_>, pos: u32) -> bool {
    let program = ctx.nodes().program();
    program
        .body
        .iter()
        .take_while(|s| s.span().start < pos)
        .any(|s| match s {
            Statement::ImportDeclaration(d) => d.source.value == "react-native",
            Statement::VariableDeclaration(v) => v
                .declarations
                .iter()
                .any(|d| d.init.as_ref().and_then(require_module) == Some("react-native")),
            _ => false,
        })
}

fn check(ctx: &Ctx<'_, '_>, e: &Expression<'_>, rn: &mut Option<bool>) {
    let e = strip_parens(e);
    if let Expression::LogicalExpression(l) = e {
        check(ctx, &l.left, rn);
        check(ctx, &l.right, rn);
        return;
    }
    let t = type_of(ctx, e);
    let leaks = t.all(|t| matches!(t, Ty::Number | Ty::BigInt))
        || (t.all(|t| matches!(t, Ty::String))
            && *rn.get_or_insert_with(|| uses_react_native(ctx, e.span().start)));
    if leaks {
        ctx.report(e.span(), MSG);
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXExpressionContainer]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::JSXExpressionContainer(c) = node.kind() else {
            return;
        };
        let Some(Expression::LogicalExpression(l)) = c.expression.as_expression().map(strip_parens)
        else {
            return;
        };
        if l.operator != LogicalOperator::And {
            return;
        }
        check(ctx, &l.left, &mut None);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn leaked_render() {
        check(
            "S6439",
            "a.tsx",
            r#"
import React from "react";
const C1 = (count, collection) => {
  count = 1;
  return <div>{count && <List elements={collection} />}</div>;
};
const C2 = (count: boolean, collection) => <div>{count && <List />}</div>;
const C3 = (collection) => {
  let test = '';
  return <div>{test && <List />}</div>;
};
const N1 = (count: number) => <div>{count && <List />}</div>; // Noncompliant {{Convert the conditional to a boolean to avoid leaked value}}
const N2 = () => {
  const count = 0;
  return <div>{count && <List />}</div>; // Noncompliant
};
const N3 = (collection: Array<number>) => <div>{collection.length && <List />}</div>; // Noncompliant
const N4 = (test: number, count: number) => <div>{(test || (count)) && <List />}</div>; // Noncompliant 2
const N5 = (n: number) => <div>{n > 0 && <List />}</div>;
"#,
        );
    }

    #[test]
    fn react_native_strings() {
        check(
            "S6439",
            "a.tsx",
            r#"
import React from "react";
import { Text } from 'react-native';
const C = (s: string) => <Text>{s && <List />}</Text>; // Noncompliant
"#,
        );
    }
}
