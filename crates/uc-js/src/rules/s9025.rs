//! Asynchronous actions should not be used in Vue computed properties
use oxc_ast_visit::{Visit, walk};
use oxc_syntax::scope::ScopeFlags;

use crate::helpers::batch_bug2::{
    find_property, static_key, visit_function_body, vue_callee_name, vue_component_object,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

struct Finder<'c, 'a, 's> {
    ctx: &'c Ctx<'a, 's>,
    /// `Some(name)` for an options-API computed property, `None` for `computed()`.
    property: Option<&'c str>,
}

impl Finder<'_, '_, '_> {
    fn report(&self, span: Span, what: &str) {
        let msg = match self.property {
            Some(p) => format!("Move this {what} out of the computed property \"{p}\"."),
            None => format!("Move this {what} out of the computed function."),
        };
        self.ctx.report(span, msg);
    }
}

fn is_timed(callee: &Expression<'_>) -> bool {
    const TIMED: [&str; 4] = [
        "setTimeout",
        "setInterval",
        "setImmediate",
        "requestAnimationFrame",
    ];
    match callee.without_parentheses() {
        Expression::Identifier(id) => TIMED.contains(&id.name.as_str()),
        Expression::StaticMemberExpression(m) => {
            matches!(&m.object, Expression::Identifier(o) if o.name == "window")
                && TIMED.contains(&m.property.name.as_str())
        }
        _ => false,
    }
}

impl<'a> Visit<'a> for Finder<'_, 'a, '_> {
    fn visit_await_expression(&mut self, it: &AwaitExpression<'a>) {
        self.report(it.span, "await operator");
        walk::walk_await_expression(self, it);
    }

    fn visit_new_expression(&mut self, it: &NewExpression<'a>) {
        if matches!(&it.callee, Expression::Identifier(id) if id.name == "Promise") {
            self.report(it.span, "Promise object");
        }
        walk::walk_new_expression(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        let promise = match it.callee.without_parentheses() {
            Expression::StaticMemberExpression(m) => {
                matches!(m.property.name.as_str(), "then" | "catch" | "finally")
                    || (matches!(&m.object, Expression::Identifier(o) if o.name == "Promise")
                        && matches!(
                            m.property.name.as_str(),
                            "all" | "race" | "reject" | "resolve"
                        ))
            }
            _ => false,
        };
        let next_tick = match it.callee.without_parentheses() {
            Expression::StaticMemberExpression(m) => {
                (m.property.name == "$nextTick"
                    && matches!(m.object, Expression::ThisExpression(_)))
                    || (m.property.name == "nextTick"
                        && matches!(&m.object, Expression::Identifier(o) if o.name == "Vue"))
            }
            Expression::Identifier(_) => vue_callee_name(self.ctx, &it.callee) == Some("nextTick"),
            _ => false,
        };
        if promise || next_tick {
            self.report(it.span, "asynchronous action");
        } else if is_timed(&it.callee) {
            self.report(it.span, "timed function");
        }
        walk::walk_call_expression(self, it);
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        if it.r#async {
            self.report(it.span, "async function declaration");
        }
        walk::walk_function(self, it, flags);
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        if it.r#async {
            self.report(it.span, "async function declaration");
        }
        walk::walk_arrow_function_expression(self, it);
    }
}

/// Checks a computed getter given as a function expression.
fn check_getter<'a>(ctx: &Ctx<'a, '_>, getter: &Expression<'a>, property: Option<&str>) {
    let mut f = Finder { ctx, property };
    let is_async = match getter.without_parentheses() {
        Expression::FunctionExpression(x) => Some((x.r#async, x.span)),
        Expression::ArrowFunctionExpression(x) => Some((x.r#async, x.span)),
        _ => None,
    };
    if let Some((true, span)) = is_async {
        f.report(span, "async function declaration");
    }
    visit_function_body(&mut f, getter);
}

/// A computed definition: a getter function, or `{ get() {}, set() {} }`.
fn check_definition<'a>(ctx: &Ctx<'a, '_>, def: &Expression<'a>, property: Option<&str>) {
    match def.without_parentheses() {
        Expression::ObjectExpression(o) => {
            if let Some(get) = find_property(o, "get") {
                check_getter(ctx, &get.value, property);
            }
        }
        e => check_getter(ctx, e, property),
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression, AstType::NewExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        if let AstKind::CallExpression(c) = node.kind()
            && vue_callee_name(ctx, &c.callee) == Some("computed")
        {
            if let Some(arg) = c.arguments.first().and_then(|a| a.as_expression()) {
                check_definition(ctx, arg, None);
            }
            return;
        }
        let kind = node.kind();
        let Some(component) = vue_component_object(&kind) else {
            return;
        };
        let Some(computed) = find_property(component, "computed") else {
            return;
        };
        let Expression::ObjectExpression(props) = computed.value.without_parentheses() else {
            return;
        };
        for p in &props.properties {
            if let ObjectPropertyKind::ObjectProperty(p) = p
                && let Some(name) = static_key(p)
            {
                check_definition(ctx, &p.value, Some(name));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn async_in_computed() {
        check(
            "S9025",
            "a.ts",
            r#"
import { computed, ref, defineComponent, nextTick } from 'vue';
const value = ref(0);
const a = computed(() => {
  somePromise.then(v => v); // Noncompliant {{Move this asynchronous action out of the computed function.}}
  return value.value;
});
const b = computed(async () => { // Noncompliant {{Move this async function declaration out of the computed function.}}
  await fetchData(); // Noncompliant {{Move this await operator out of the computed function.}}
  return value.value;
});
const c = computed(() => {
  new Promise((resolve) => resolve(value.value)); // Noncompliant {{Move this Promise object out of the computed function.}}
  setInterval(() => {}, 100); // Noncompliant {{Move this timed function out of the computed function.}}
  nextTick(() => {}); // Noncompliant
  return value.value;
});
const ok = computed(() => [...value.value].sort((x, y) => x - y));
export default defineComponent({
  computed: {
    data() {
      setTimeout(() => {}, 100); // Noncompliant {{Move this timed function out of the computed property "data".}}
      this.$nextTick(() => {}); // Noncompliant {{Move this asynchronous action out of the computed property "data".}}
      return this.value;
    },
    other: {
      get() { return fetch("x").then(r => r); }, // Noncompliant
      set(v) { setTimeout(() => {}); },
    },
    fine() { return this.value; },
  },
  methods: {
    m() { setTimeout(() => {}); },
  },
});
function unrelated() { setTimeout(() => {}); }
"#,
        );
    }
}
