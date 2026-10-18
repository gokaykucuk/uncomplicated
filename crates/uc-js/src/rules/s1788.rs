//! Function parameters with default values should be last
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_plain(p: &FormalParameter<'_>) -> bool {
    p.initializer.is_none()
        && !p.optional
        && !matches!(p.pattern, BindingPattern::AssignmentPattern(_))
}

/// `function reducer(state = initial, action)` / `(state = x, { type })`: Redux reducers.
fn is_redux_reducer(params: &FormalParameters<'_>) -> bool {
    if params.items.len() != 2 || params.rest.is_some() {
        return false;
    }
    let (first, second) = (&params.items[0], &params.items[1]);
    let first_is_state = first.initializer.is_some()
        && matches!(&first.pattern, BindingPattern::BindingIdentifier(id) if id.name == "state");
    if !first_is_state {
        return false;
    }
    match &second.pattern {
        BindingPattern::BindingIdentifier(id) => id.name == "action",
        BindingPattern::ObjectPattern(o) => o
            .properties
            .iter()
            .any(|p| !p.computed && property_key_name(&p.key) == Some("type")),
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::FormalParameters]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::FormalParameters(params) = node.kind() else {
            return;
        };
        if params.items.len() < 2 || is_redux_reducer(params) {
            return;
        }
        let mut seen_plain = false;
        for p in params.items.iter().rev() {
            if is_plain(p) {
                seen_plain = true;
                continue;
            }
            // Only `name = value` parameters are reported (not TS parameter properties,
            // optional parameters or destructuring patterns with defaults).
            if seen_plain
                && p.initializer.is_some()
                && p.accessibility.is_none()
                && !p.readonly
                && !p.r#override
                && matches!(p.pattern, BindingPattern::BindingIdentifier(_))
            {
                ctx.report(
                    p.span,
                    "Move this default parameter last, so callers can omit it instead of passing \"undefined\".",
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn default_param_last() {
        check(
            "S1788",
            "a.ts",
            r#"
function multiplyDefaultLast(b, a = 1) { return b * a; }
function multiplyBothDefault(b = 2, a = 1) { return b * a; }
function appReducerNamed(state = initialState, action) { return state; }
function appReducerWithType(state = initialState, { type }) { return state; }
const dataReducer = (state = null, { type, payload }) => state;
function multiply(a = 1, b) { return a * b; } // Noncompliant {{Move this default parameter last, so callers can omit it instead of passing "undefined".}}
function appReducer(state = initialState, action, param) { return state; } // Noncompliant
function wrong(status = initialState, action) { return status; } // Noncompliant
function notReducer(state = initialState, { payload }) { return state; } // Noncompliant
function opt(a?: number, b) {}
function destr({ a } = {}, b) {}
class C { constructor(private a = 1, b) {} }
function rest(a = 1, ...b) {}
"#,
        );
    }
}
