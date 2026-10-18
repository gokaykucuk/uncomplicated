//! Function argument names should be unique
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Function]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::Function(f) = node.kind() else {
            return;
        };
        if f.params.parameters_count() < 2 {
            return;
        }
        let mut names: Vec<&str> = Vec::new();
        for p in f.params.iter_bindings() {
            for id in p.get_binding_identifiers() {
                names.push(id.name.as_str());
            }
        }
        let mut reported: Vec<&str> = Vec::new();
        for (i, n) in names.iter().enumerate() {
            if !reported.contains(n) && names[i + 1..].contains(n) {
                reported.push(n);
                ctx.report(f.span, format!("Duplicate param '{n}'."));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn dupe_args() {
        check(
            "S1536",
            "a.js",
            r#"
function foo(a, b, a) {} // Noncompliant {{Duplicate param 'a'.}}
var bar = function (a, b, b, a, c) {}; // Noncompliant 2
function ok(a, b, c) {}
function ok2({ a }, [b], ...c) {}
"#,
        );
    }
}
