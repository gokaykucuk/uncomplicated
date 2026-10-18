//! Side effects should not be performed inside Vue computed properties
use oxc_ast_visit::{Visit, walk};
use oxc_syntax::scope::ScopeFlags;

use crate::helpers::batch_bug2::{
    find_mutating, find_property, is_script_setup, is_vue_this, static_key, vue_component_object,
    vue_import_name,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MSG_PROPERTY: &str = "Remove this side effect from the computed property.";
const MSG_FUNCTION: &str = "Remove this side effect from the computed function.";

/// Visits a getter body without entering nested functions.
struct Getter<'c, 'a, 's> {
    ctx: &'c Ctx<'a, 's>,
    /// `Some(span)` of the setup scope for composition-API getters; `None` for options-API ones.
    setup: Option<Span>,
    getter: Span,
}

impl Getter<'_, '_, '_> {
    fn options_member(
        &self,
        object: &Expression<'_>,
        property: Option<&str>,
        prop_span: Span,
        id: NodeId,
    ) {
        let ctx = self.ctx;
        let this = is_vue_this(ctx, object);
        let is_vue = matches!(object, Expression::Identifier(o) if o.name == "Vue");
        let parent_is_call = matches!(ctx.parent_kind(id), AstKind::CallExpression(c) if c.callee.span() == ctx.node(id).span());
        if parent_is_call
            && ((this && property == Some("$set")) || (is_vue && property == Some("set")))
        {
            ctx.report(prop_span, MSG_PROPERTY);
            return;
        }
        if this && let Some(m) = find_mutating(ctx, id) {
            ctx.report(m.span, MSG_PROPERTY);
        }
    }

    fn composition_member(&self, object: &Expression<'_>, setup: Span) {
        let ctx = self.ctx;
        let Expression::Identifier(id) = object else {
            return;
        };
        let Some(sym) = ctx.symbol_of(id) else { return };
        if !ctx.scoping().symbol_redeclarations(sym).is_empty() {
            return;
        }
        let decl = ctx.symbol_decl(sym);
        if matches!(
            decl.kind(),
            AstKind::ImportSpecifier(_)
                | AstKind::ImportDefaultSpecifier(_)
                | AstKind::ImportNamespaceSpecifier(_)
        ) {
            return;
        }
        let d = decl.span();
        let inside = |outer: Span| outer.start <= d.start && d.end <= outer.end;
        if !inside(setup) || inside(self.getter) {
            return;
        }
        if let Some(m) = find_mutating(ctx, id.node_id.get()) {
            ctx.report(m.span, MSG_FUNCTION);
        }
    }

    fn member(&self, object: &Expression<'_>, property: Option<&str>, prop_span: Span, id: NodeId) {
        match self.setup {
            None => self.options_member(object, property, prop_span, id),
            Some(setup) => self.composition_member(object, setup),
        }
    }
}

impl<'a> Visit<'a> for Getter<'_, 'a, '_> {
    fn visit_static_member_expression(&mut self, it: &StaticMemberExpression<'a>) {
        self.member(
            &it.object,
            Some(it.property.name.as_str()),
            it.property.span,
            it.node_id.get(),
        );
        walk::walk_static_member_expression(self, it);
    }

    fn visit_computed_member_expression(&mut self, it: &ComputedMemberExpression<'a>) {
        let name = match &it.expression {
            Expression::StringLiteral(s) => Some(s.value.as_str()),
            _ => None,
        };
        self.member(&it.object, name, it.expression.span(), it.node_id.get());
        walk::walk_computed_member_expression(self, it);
    }

    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
}

fn visit_body<'a>(g: &mut Getter<'_, 'a, '_>, f: &Expression<'a>) {
    match f.without_parentheses() {
        Expression::FunctionExpression(func) => {
            if let Some(b) = &func.body {
                for s in &b.statements {
                    g.visit_statement(s);
                }
            }
        }
        Expression::ArrowFunctionExpression(a) => match a.body.as_function_body() {
            Some(b) => {
                for s in &b.statements {
                    g.visit_statement(s);
                }
            }
            None => {
                if let Some(e) = a.body.as_expression() {
                    g.visit_expression(e);
                }
            }
        },
        _ => {}
    }
}

/// The `setup` scope enclosing `id`: the whole `<script setup>`, or a component's `setup()` body.
fn setup_scope(ctx: &Ctx<'_, '_>, id: NodeId) -> Option<Span> {
    if is_script_setup(ctx) {
        return Some(ctx.nodes().program().span);
    }
    for anc in ctx.nodes().ancestor_ids(id) {
        let node = ctx.node(anc);
        let body_span = match node.kind() {
            AstKind::Function(f) => f.body.as_ref().map(|b| b.span),
            AstKind::ArrowFunctionExpression(a) => Some(a.body.span()),
            _ => continue,
        };
        let is_setup = match ctx.parent_kind(anc) {
            AstKind::ObjectProperty(p) => static_key(p) == Some("setup"),
            _ => false,
        };
        if is_setup {
            let obj = ctx.parent(ctx.parent(anc).id());
            if vue_component_object(ctx, &ctx.parent_kind(obj.id()))
                .is_some_and(|o| o.span == obj.span())
            {
                return body_span;
            }
        }
    }
    None
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
        // Composition API: `computed(getter)` / `computed({ get })`.
        if let AstKind::CallExpression(c) = &kind
            && let Expression::Identifier(callee) = &c.callee
            && (vue_import_name(ctx, callee) == Some("computed")
                || ctx.is_global(callee, "computed"))
        {
            let getter = match c.arguments.first().and_then(|a| a.as_expression()) {
                Some(
                    e
                    @ (Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_)),
                ) => Some(e),
                Some(Expression::ObjectExpression(o)) => {
                    find_property(o, "get").map(|p| &p.value).filter(|v| {
                        matches!(
                            v,
                            Expression::FunctionExpression(_)
                                | Expression::ArrowFunctionExpression(_)
                        )
                    })
                }
                _ => None,
            };
            if let Some(getter) = getter
                && let Some(setup) = setup_scope(ctx, node.id())
            {
                let mut g = Getter {
                    ctx,
                    setup: Some(setup),
                    getter: getter.span(),
                };
                visit_body(&mut g, getter);
            }
            return;
        }
        // Options API (not used together with `<script setup>`).
        if is_script_setup(ctx) {
            return;
        }
        let Some(component) = vue_component_object(ctx, &kind) else {
            return;
        };
        let Some(computed) = find_property(component, "computed") else {
            return;
        };
        let Expression::ObjectExpression(props) = &computed.value else {
            return;
        };
        for p in &props.properties {
            let ObjectPropertyKind::ObjectProperty(p) = p else {
                continue;
            };
            let value = strip_ts(&p.value);
            let getter = match value {
                Expression::FunctionExpression(_) => Some(value),
                Expression::ObjectExpression(o) => find_property(o, "get")
                    .map(|g| &g.value)
                    .filter(|v| matches!(v, Expression::FunctionExpression(_))),
                _ => None,
            };
            if let Some(getter) = getter {
                let mut g = Getter {
                    ctx,
                    setup: None,
                    getter: getter.span(),
                };
                visit_body(&mut g, getter);
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
            "S8988",
            "a.vue",
            r#"
<script>
export default {
  computed: {
    a() {
      this.count++; // Noncompliant {{Remove this side effect from the computed property.}}
      this.list.push(1); // Noncompliant
      this.$set(this.obj, 'x', 1); // Noncompliant
      return this.count;
    },
    b: {
      get() { this.x = 1; return 2; }, // Noncompliant
      set(v) { this.x = v; },
    },
    c() {
      const copy = [...this.list];
      copy.sort();
      return this.list.slice().sort((x, y) => { this.z = 1; return x - y; });
    },
  },
  methods: { m() { this.count++; } },
};
</script>
"#,
        );
    }

    #[test]
    fn composition_api() {
        check(
            "S8988",
            "a.vue",
            r#"
<script setup>
import { computed, ref } from 'vue';
const count = ref(0);
const list = ref([]);
const doubled = computed(() => {
  count.value++; // Noncompliant {{Remove this side effect from the computed function.}}
  list.value.push(1); // Noncompliant
  const local = [];
  local.push(1);
  return count.value * 2;
});
const fine = computed(() => list.value.slice().sort());
</script>
"#,
        );
    }
}
