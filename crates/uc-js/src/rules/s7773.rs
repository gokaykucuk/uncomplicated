//! Number static methods and properties should be preferred over global equivalents
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
        // `Infinity` is not checked by default (`checkInfinity: false`).
        for name in ["parseInt", "parseFloat", "NaN", "isNaN", "isFinite"] {
            let Some(ids) = scoping.root_unresolved_references().get(name) else {
                continue;
            };
            for &rid in ids.iter() {
                let r = scoping.get_reference(rid);
                if !r.is_read() || r.is_write() {
                    continue;
                }
                let node = ctx.node(r.node_id());
                ctx.report(
                    node.kind().span(),
                    format!("Prefer `Number.{name}` over `{name}`."),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn number_properties() {
        check(
            "S7773",
            "a.js",
            r#"
const a = parseInt('10', 10); // Noncompliant {{Prefer `Number.parseInt` over `parseInt`.}}
const b = parseFloat('1.5'); // Noncompliant
const c = isNaN(x); // Noncompliant {{Prefer `Number.isNaN` over `isNaN`.}}
const d = x === NaN; // Noncompliant
const e = Infinity;
const f = Number.parseInt('10');
function g(parseInt) { return parseInt(x); }
"#,
        );
    }
}
