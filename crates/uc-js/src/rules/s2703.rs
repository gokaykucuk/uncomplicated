//! Variables should be declared explicitly
use crate::helpers::batch_smell2::is_known_global;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let scoping = ctx.scoping();
        let mut found: Vec<(Span, &str)> = Vec::new();
        for (name, ids) in scoping.root_unresolved_references().iter() {
            let name: &str = name.as_str();
            if is_known_global(name) {
                continue;
            }
            let first_write = ids
                .iter()
                .map(|id| scoping.get_reference(*id))
                .filter(|r| r.is_write())
                .map(|r| ctx.node(r.node_id()).span())
                .min_by_key(|s| s.start);
            if let Some(span) = first_write {
                found.push((span, name));
            }
        }
        found.sort_by_key(|(s, _)| s.start);
        for (span, name) in found {
            ctx.report(
                span,
                format!("Add the \"let\", \"const\" or \"var\" keyword to this declaration of \"{name}\" to make it explicit."),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn implicit_global() {
        check(
            "S2703",
            "a.js",
            r#"
function f() {
  myVar = 42; // Noncompliant {{Add the "let", "const" or "var" keyword to this declaration of "myVar" to make it explicit.}}
  myVar = 43;
  other++; // Noncompliant
  for (key in obj) {} // Noncompliant
  var declared = 1;
  declared = 2;
  window = 1;
  onload = function () {};
  read = unknownRead; // Noncompliant
}
"#,
        );
    }

    #[test]
    fn globals_sorted() {
        let g = crate::helpers::batch_smell2::KNOWN_GLOBALS;
        assert!(g.windows(2).all(|w| w[0] < w[1]));
    }
}
