//! Vue component props should not be mutated directly
use oxc_ast_visit::{Visit, walk};

use crate::helpers::batch_bug2::{
    component_props, find_mutating, find_property, is_vue_this, member_name, vue_component_object,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn report(ctx: &Ctx<'_, '_>, span: Span, key: &str) {
    ctx.report(
        span,
        format!("Emit an event and register a listener for it in the parent, instead of mutating the \"{key}\" prop directly."),
    );
}

/// Bindings of a props pattern with their property path (`props` → [], `{ a: { b } }` → [a, b]).
fn pattern_bindings<'a>(
    p: &BindingPattern<'a>,
    path: Vec<String>,
    out: &mut Vec<(SymbolId, Vec<String>)>,
) {
    match p {
        BindingPattern::BindingIdentifier(id) => {
            if let Some(sym) = id.symbol_id.get() {
                out.push((sym, path));
            }
        }
        BindingPattern::AssignmentPattern(a) => pattern_bindings(&a.left, path, out),
        BindingPattern::ObjectPattern(o) => {
            for prop in &o.properties {
                let name = match property_key_name(&prop.key) {
                    Some(n) if !prop.computed => n.to_string(),
                    _ => "?unknown?".to_string(),
                };
                let mut next = path.clone();
                next.push(name);
                pattern_bindings(&prop.value, next, out);
            }
            if let Some(rest) = &o.rest {
                pattern_bindings(&rest.argument, path, out);
            }
        }
        BindingPattern::ArrayPattern(a) => {
            for (i, el) in a.elements.iter().enumerate() {
                if let Some(el) = el {
                    let mut next = path.clone();
                    next.push(i.to_string());
                    pattern_bindings(el, next, out);
                }
            }
        }
    }
}

/// Reports mutations through the references of a props binding.
fn verify_prop_variable(ctx: &Ctx<'_, '_>, sym: SymbolId, path: &[String]) {
    for r in ctx.scoping().get_resolved_references(sym) {
        if !r.is_read() {
            continue;
        }
        let Some(m) = find_mutating(ctx, r.node_id()) else {
            continue;
        };
        let name = if path.is_empty() {
            let Some(&first) = m.path.first() else {
                continue;
            };
            let Some(n) = member_name(ctx, first) else {
                continue;
            };
            n
        } else {
            if m.path.is_empty() && !m.is_call {
                continue;
            }
            path[0].clone()
        };
        report(ctx, m.span, &name);
    }
}

fn verify_pattern(ctx: &Ctx<'_, '_>, pattern: &BindingPattern<'_>) {
    let mut bindings = Vec::new();
    pattern_bindings(pattern, Vec::new(), &mut bindings);
    for (sym, path) in bindings {
        verify_prop_variable(ctx, sym, &path);
    }
}

/// `this.<prop>` mutations inside an options-API component.
struct ThisMutations<'c, 'a, 's> {
    ctx: &'c Ctx<'a, 's>,
    props: Vec<&'c str>,
}

impl ThisMutations<'_, '_, '_> {
    fn check(&self, object: &Expression<'_>, id: NodeId) {
        if !is_vue_this(self.ctx, object) {
            return;
        }
        let Some(name) = member_name(self.ctx, id) else {
            return;
        };
        if !self.props.contains(&name.as_str()) {
            return;
        }
        if let Some(m) = find_mutating(self.ctx, id) {
            report(self.ctx, m.span, &name);
        }
    }
}

impl<'a> Visit<'a> for ThisMutations<'_, 'a, '_> {
    fn visit_static_member_expression(&mut self, it: &StaticMemberExpression<'a>) {
        self.check(&it.object, it.node_id.get());
        walk::walk_static_member_expression(self, it);
    }

    fn visit_computed_member_expression(&mut self, it: &ComputedMemberExpression<'a>) {
        if member_name(self.ctx, it.node_id.get()).is_some_and(|n| !n.starts_with('[')) {
            self.check(&it.object, it.node_id.get());
        }
        walk::walk_computed_member_expression(self, it);
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::CallExpression,
            AstType::NewExpression,
            AstType::ExportDefaultDeclaration,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let kind = node.kind();
        // `<script setup>`: `const props = defineProps(...)`.
        if let AstKind::CallExpression(c) = &kind
            && ctx.file.path.ends_with(".vue")
            && matches!(&c.callee, Expression::Identifier(id) if ctx.is_global(id, "defineProps"))
        {
            let mut target = node;
            let parent = ctx.parent(node.id());
            if let AstKind::CallExpression(w) = parent.kind()
                && matches!(&w.callee, Expression::Identifier(id) if id.name == "withDefaults")
                && w.arguments.first().is_some_and(|a| a.span() == c.span)
            {
                target = parent;
            }
            if let AstKind::VariableDeclarator(v) = ctx.parent_kind(target.id())
                && v.init.as_ref().is_some_and(|i| i.span() == target.span())
            {
                verify_pattern(ctx, &v.id);
            }
            return;
        }
        let Some(component) = vue_component_object(ctx, &kind) else {
            return;
        };
        let props = component_props(component);
        if !props.is_empty() {
            let mut v = ThisMutations { ctx, props };
            v.visit_object_expression(component);
        }
        // `setup(props) { ... }`
        if let Some(setup) = find_property(component, "setup") {
            let params = match strip_ts(&setup.value) {
                Expression::FunctionExpression(f) => Some(&f.params),
                Expression::ArrowFunctionExpression(a) => Some(&a.params),
                _ => None,
            };
            if let Some(first) = params.and_then(|p| p.items.first())
                && !matches!(first.pattern, BindingPattern::ArrayPattern(_))
            {
                verify_pattern(ctx, &first.pattern);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn options_api() {
        check(
            "S8951",
            "a.vue",
            r#"
<script>
export default {
  props: ['value', 'items'],
  methods: {
    m() {
      this.value = 1; // Noncompliant {{Emit an event and register a listener for it in the parent, instead of mutating the "value" prop directly.}}
      this.items.push(2); // Noncompliant {{Emit an event and register a listener for it in the parent, instead of mutating the "items" prop directly.}}
      this.value.x = 3; // Noncompliant
      this.value++; // Noncompliant
      const vm = this;
      vm.value = 4; // Noncompliant
      this.other = 5;
      const copy = this.items.slice();
      copy.push(1);
    },
  },
  setup(props) {
    props.value = 1; // Noncompliant {{Emit an event and register a listener for it in the parent, instead of mutating the "value" prop directly.}}
    const x = props.value;
  },
};
</script>
"#,
        );
    }

    #[test]
    fn script_setup() {
        check(
            "S8951",
            "a.vue",
            r#"
<script setup>
const props = defineProps({ list: Array, obj: Object });
props.list.push(1); // Noncompliant {{Emit an event and register a listener for it in the parent, instead of mutating the "list" prop directly.}}
props.obj.a = 2; // Noncompliant {{Emit an event and register a listener for it in the parent, instead of mutating the "obj" prop directly.}}
const n = props.list.length;
</script>
"#,
        );
        check(
            "S8951",
            "b.vue",
            r#"
<script setup>
const { list } = withDefaults(defineProps({ list: Array }), {});
list.push(1); // Noncompliant
const m = list.map(x => x);
</script>
"#,
        );
    }
}
