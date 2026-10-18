//! Vue component field names should not be duplicated
use crate::helpers::batch_bug2::{function_body, static_key, vue_component_object};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const GROUPS: [&str; 5] = ["props", "computed", "data", "methods", "setup"];

fn object_keys<'a>(o: &ObjectExpression<'a>, out: &mut Vec<(&'a str, Span)>) {
    for p in &o.properties {
        if let ObjectPropertyKind::ObjectProperty(p) = p
            && let Some(name) = static_key(p)
        {
            out.push((name, p.key.span()));
        }
    }
}

/// Keys of the object literal(s) returned by a function (`data() { return {...} }`, `() => ({})`).
fn returned_keys<'a>(e: &Expression<'a>, out: &mut Vec<(&'a str, Span)>) {
    if let Expression::ArrowFunctionExpression(a) = e
        && let Some(body) = a.body.as_expression()
    {
        if let Expression::ObjectExpression(o) = strip_ts(body) {
            object_keys(o, out);
        }
        return;
    }
    let Some(body) = function_body(e) else { return };
    for s in &body.statements {
        if let Statement::ReturnStatement(r) = s
            && let Some(Expression::ObjectExpression(o)) = r.argument.as_ref().map(strip_ts)
        {
            object_keys(o, out);
        }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression, AstType::NewExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let kind = node.kind();
        let Some(component) = vue_component_object(&kind) else {
            return;
        };
        let mut keys: Vec<(&str, Span)> = Vec::new();
        for p in &component.properties {
            let ObjectPropertyKind::ObjectProperty(p) = p else {
                continue;
            };
            let Some(group) = static_key(p) else { continue };
            if !GROUPS.contains(&group) {
                continue;
            }
            match strip_ts(&p.value) {
                Expression::ObjectExpression(o) if group != "setup" => object_keys(o, &mut keys),
                Expression::ArrayExpression(a) if group == "props" => {
                    for el in &a.elements {
                        if let Some(Expression::StringLiteral(s)) = el.as_expression() {
                            keys.push((s.value.as_str(), s.span));
                        }
                    }
                }
                v if group == "data" || group == "setup" => returned_keys(v, &mut keys),
                _ => {}
            }
        }
        let mut seen: FxHashSet<&str> = FxHashSet::default();
        for (name, span) in keys {
            if !seen.insert(name) {
                ctx.report(span, format!("Duplicate key '{name}'."));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn dupe_keys() {
        check(
            "S9128",
            "a.js",
            r#"
import { defineComponent } from 'vue';
export default defineComponent({
  props: ['name', 'age'],
  computed: {
    name() { return 1; }, // Noncompliant {{Duplicate key 'name'.}}
  },
  data() {
    return { name: 1, other: 2 }; // Noncompliant
  },
  methods: {
    other() {}, // Noncompliant
    fine() {},
  },
  setup() {
    return { age: 1 }; // Noncompliant
  },
});
const ok = defineComponent({
  props: { a: String },
  data: () => ({ b: 1 }),
  methods: { c() {} },
});
"#,
        );
    }
}
