//! Functions should be called consistently with or without "new"
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let mut used_in_new: FxHashMap<SymbolId, Span> = FxHashMap::default();
        let mut used_in_call: FxHashMap<SymbolId, Span> = FxHashMap::default();
        let mut has_issue: FxHashSet<SymbolId> = FxHashSet::default();
        for node in ctx.nodes().iter() {
            let (callee, is_new) = match node.kind() {
                AstKind::NewExpression(n) => (&n.callee, true),
                AstKind::CallExpression(c) => (&c.callee, false),
                _ => continue,
            };
            let Expression::Identifier(id) = callee else {
                continue;
            };
            let Some(sym) = ctx.symbol_of(id) else {
                continue;
            };
            let (this_map, other_map, tail) = if is_new {
                (&mut used_in_new, &mut used_in_call, "out")
            } else {
                (&mut used_in_call, &mut used_in_new, "")
            };
            match other_map.get(&sym) {
                Some(other) if !has_issue.contains(&sym) => {
                    let line = ctx.line(other.start);
                    ctx.report_with(
                        id.span,
                        format!("Correct the use of this function; on line {line} it was called with{tail} \"new\"."),
                        [(*other, None)],
                    );
                    has_issue.insert(sym);
                }
                _ => {
                    this_map.insert(sym, id.span);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn inconsistent_new() {
        check(
            "S3686",
            "a.js",
            r#"
function getNum() { return 5; }
function Thing() { this.a = 1; }
var my = new getNum();
var other = getNum();      // Noncompliant {{Correct the use of this function; on line 4 it was called with "new".}}
var x = getNum();
var t = Thing();
var u = new Thing();       // Noncompliant {{Correct the use of this function; on line 7 it was called without "new".}}
var v = new Thing();
var d = new Date();
var e = Date();
"#,
        );
    }
}
