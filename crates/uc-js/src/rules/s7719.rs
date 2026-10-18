//! Date objects should be cloned directly without calling "getTime()"
use crate::helpers::batch_unicorn::{arg, is_method_call, no_spread};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::NewExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::NewExpression(new) = node.kind() else {
            return;
        };
        if !matches!(&new.callee, Expression::Identifier(id) if id.name == "Date")
            || new.arguments.len() != 1
            || !no_spread(&new.arguments)
        {
            return;
        }
        let Some(Expression::CallExpression(call)) = arg(&new.arguments, 0) else {
            return;
        };
        let Some(member) = is_method_call(call, &["getTime"], Some(0), false) else {
            return;
        };
        ctx.report(
            Span::new(member.property.span.start, call.span.end),
            "Unnecessary `.getTime()` call.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn date_clone() {
        check(
            "S7719",
            "a.js",
            r#"
new Date(date.getTime()); // Noncompliant {{Unnecessary `.getTime()` call.}}
new Date((foo.bar).getTime());   // Noncompliant
new Date(date);
new Date(date.getTime(1));
new Date(date?.getTime());
new Date(date.getTime?.());
new Date(date.getTime(), 1);
new Date(date['getTime']());
new Foo(date.getTime());
"#,
        );
    }
}
