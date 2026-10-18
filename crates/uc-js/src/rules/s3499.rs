//! Shorthand object properties should be grouped at the beginning or end of an object declaration
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ObjectExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::ObjectExpression(o) = node.kind() else {
            return;
        };
        let mut props = Vec::with_capacity(o.properties.len());
        for p in &o.properties {
            match p {
                ObjectPropertyKind::ObjectProperty(p) => props.push(p),
                ObjectPropertyKind::SpreadProperty(_) => return,
            }
        }
        let total_shorthand = props.iter().filter(|p| p.shorthand).count();
        if total_shorthand == 0 {
            return;
        }
        let at_begin = props.iter().take_while(|p| p.shorthand).count();
        let at_end = props.iter().rev().take_while(|p| p.shorthand).count();
        let all_begin = at_begin == total_shorthand;
        let all_end = at_end == total_shorthand;
        let n = props.len();
        let (begin, end, position) = if !all_begin && at_begin > at_end {
            (at_begin, n, "the beginning")
        } else if !all_end && at_end > at_begin {
            (0, n - at_end, "the end")
        } else if !all_begin && !all_end {
            (0, n, "either the beginning or end")
        } else {
            return;
        };
        let secondary: Vec<(Span, Option<String>)> = props[begin..end]
            .iter()
            .filter(|p| p.shorthand)
            .map(|p| (p.span, Some(format!("Move to {position}"))))
            .collect();
        ctx.report_with(
            Span::new(o.span.start, o.span.start + 1),
            format!("Group all shorthand properties at {position} of this object declaration."),
            secondary,
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn grouping() {
        check(
            "S3499",
            "a.js",
            r#"
var o1 = { a, b: 1, c, d: 2, e }; // Noncompliant {{Group all shorthand properties at either the beginning or end of this object declaration.}}
var o2 = { a, b, c: 1, d }; // Noncompliant {{Group all shorthand properties at the beginning of this object declaration.}}
var o3 = { a, b: 1, c, d }; // Noncompliant {{Group all shorthand properties at the end of this object declaration.}}
var o4 = { a, b, c: 1 };
var o5 = { c: 1, a, b };
var o6 = { a: 1, b: 2 };
var o7 = { a, b: 1, ...c, d };
var o8 = { a, b() {}, c }; // Noncompliant
"#,
        );
    }
}
