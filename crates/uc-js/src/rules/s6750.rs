//! The return value of "ReactDOM.render" should not be used
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(c) = node.kind() else {
            return;
        };
        let Expression::StaticMemberExpression(m) = &c.callee else {
            return;
        };
        if m.property.name != "render"
            || !matches!(&m.object, Expression::Identifier(o) if o.name == "ReactDOM")
        {
            return;
        }
        let mut parent = ctx.parent(node.id());
        while let AstKind::ParenthesizedExpression(_) = parent.kind() {
            parent = ctx.parent(parent.id());
        }
        let used = match parent.kind() {
            AstKind::VariableDeclarator(_)
            | AstKind::ObjectProperty(_)
            | AstKind::ReturnStatement(_)
            | AstKind::AssignmentExpression(_) => true,
            AstKind::ArrowFunctionExpression(a) => a.get_expression().is_some(),
            _ => false,
        };
        if used {
            ctx.report(
                m.span,
                "Do not depend on the return value from ReactDOM.render",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn render_return_value() {
        check(
            "S6750",
            "a.jsx",
            r#"
const inst = ReactDOM.render(<App />, root); // Noncompliant {{Do not depend on the return value from ReactDOM.render}}
ReactDOM.render(<App />, root);
const f = () => ReactDOM.render(<App />, root); // Noncompliant
function g() { return ReactDOM.render(<App />, root); } // Noncompliant
x = ReactDOM.render(<App />, root); // Noncompliant
const o = { a: ReactDOM.render(<App />, root) }; // Noncompliant
"#,
        );
    }
}
