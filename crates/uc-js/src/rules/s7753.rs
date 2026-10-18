//! "indexOf()" and "lastIndexOf()" should be used instead of "findIndex()" and "findLastIndex()" for simple equality searches
use crate::helpers::batch_unicorn::simple_array_search;
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
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        if let Some(span) = simple_array_search(ctx, call, "findIndex") {
            ctx.report(
                span,
                "Use `.indexOf()` instead of `.findIndex()` when looking for the index of an item.",
            );
        } else if let Some(span) = simple_array_search(ctx, call, "findLastIndex") {
            ctx.report(span, "Use `.lastIndexOf()` instead of `.findLastIndex() when looking for the index of an item.`");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn index_of() {
        check(
            "S7753",
            "a.js",
            r#"
const a = values.findIndex(x => x === 'foo'); // Noncompliant {{Use `.indexOf()` instead of `.findIndex()` when looking for the index of an item.}}
const b = values.findLastIndex(x => 'foo' === x); // Noncompliant {{Use `.lastIndexOf()` instead of `.findLastIndex() when looking for the index of an item.`}}
const c = values.findIndex(function (x) { return x === foo; }); // Noncompliant
const d = values.findIndex(x => { return x === foo; }); // Noncompliant
const e = values.findIndex(x => x.id === 'foo');
const f = values.findIndex(x => x === x);
const g = values.findIndex(x => x == 'foo');
const h = values.findIndex((x, i) => x === i);
const i = values.findIndex(function (x) { return x === this.foo; });
const j = values.findIndex(async x => x === 'foo');
"#,
        );
    }
}
