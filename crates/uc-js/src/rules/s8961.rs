//! Vue component events should be explicitly declared
use crate::helpers::batch_smell4::{
    find_property, is_vue3_project, static_key_name, vue_component_object,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Declared event names; `None` when the declaration cannot be analyzed (anything may be emitted).
fn declared_names(e: Option<&Expression<'_>>) -> Option<Vec<String>> {
    let Some(e) = e else { return Some(Vec::new()) };
    match strip_ts(e) {
        Expression::ArrayExpression(a) => a
            .elements
            .iter()
            .map(|el| {
                el.as_expression()
                    .and_then(string_value)
                    .map(str::to_string)
            })
            .collect(),
        Expression::ObjectExpression(o) => o
            .properties
            .iter()
            .map(|p| match p {
                ObjectPropertyKind::ObjectProperty(p) => {
                    static_key_name(&p.key, p.computed).map(|n| n.into_owned())
                }
                ObjectPropertyKind::SpreadProperty(_) => None,
            })
            .collect(),
        _ => None,
    }
}

/// The innermost Vue component options object enclosing `id`.
fn enclosing_component<'a>(ctx: &Ctx<'a, '_>, id: NodeId) -> Option<&'a ObjectExpression<'a>> {
    ctx.ancestors(id).find_map(|anc| {
        let AstKind::ObjectExpression(o) = anc.kind() else {
            return None;
        };
        let mut owner = ctx.parent(anc.id());
        while matches!(
            owner.kind(),
            AstKind::ParenthesizedExpression(_) | AstKind::TSAsExpression(_)
        ) {
            owner = ctx.parent(owner.id());
        }
        vue_component_object(ctx, owner).filter(|c| c.span == o.span)
    })
}

/// The `setup(props, context)` function of a component object.
fn setup_function<'a>(obj: &'a ObjectExpression<'a>) -> Option<&'a FormalParameters<'a>> {
    let p = find_property(obj, "setup")?;
    match strip_parens(&p.value) {
        Expression::FunctionExpression(f) => Some(&f.params),
        Expression::ArrowFunctionExpression(a) => Some(&a.params),
        _ => None,
    }
}

enum Emitter<'a> {
    /// Options API / `setup`: checked against the `emits` option.
    Options(&'a ObjectExpression<'a>),
    /// `<script setup>`: checked against `defineEmits(...)`.
    DefineEmits(&'a CallExpression<'a>),
}

/// The binding `sym` is the `context` (`ctx == true`) or the destructured `emit` of `setup`.
fn setup_binding<'a>(
    ctx: &Ctx<'a, '_>,
    sym: SymbolId,
    want_context: bool,
) -> Option<&'a ObjectExpression<'a>> {
    let decl = ctx.symbol_decl(sym);
    let param_node = std::iter::once(decl)
        .chain(ctx.ancestors(decl.id()).take(4))
        .find(|n| matches!(n.kind(), AstKind::FormalParameter(_)))?;
    let AstKind::FormalParameter(param) = param_node.kind() else {
        return None;
    };
    let component = enclosing_component(ctx, param_node.id())?;
    let params = setup_function(component)?;
    if params.items.get(1).map(|p| p.span) != Some(param.span) {
        return None;
    }
    let ok = match &param.pattern {
        BindingPattern::BindingIdentifier(b) => want_context && b.symbol_id.get() == Some(sym),
        BindingPattern::ObjectPattern(op) => {
            !want_context
                && op.properties.iter().any(|p| {
                    static_key_name(&p.key, p.computed).as_deref() == Some("emit")
                        && matches!(&p.value, BindingPattern::BindingIdentifier(b) if b.symbol_id.get() == Some(sym))
                })
        }
        _ => false,
    };
    ok.then_some(component)
}

fn emitter<'a>(
    ctx: &Ctx<'a, '_>,
    node: &AstNode<'a>,
    call: &'a CallExpression<'a>,
) -> Option<Emitter<'a>> {
    match &call.callee {
        Expression::StaticMemberExpression(m) if m.property.name == "$emit" => {
            matches!(m.object, Expression::ThisExpression(_))
                .then(|| enclosing_component(ctx, node.id()).map(Emitter::Options))
                .flatten()
        }
        Expression::StaticMemberExpression(m) if m.property.name == "emit" => {
            let Expression::Identifier(obj) = &m.object else {
                return None;
            };
            setup_binding(ctx, ctx.symbol_of(obj)?, true).map(Emitter::Options)
        }
        Expression::Identifier(id) => {
            let sym = ctx.symbol_of(id)?;
            if let Some(component) = setup_binding(ctx, sym, false) {
                return Some(Emitter::Options(component));
            }
            // `const emit = defineEmits([...])` in `<script setup>`.
            let AstKind::VariableDeclarator(v) = ctx.symbol_decl(sym).kind() else {
                return None;
            };
            match v.init.as_ref().map(strip_ts) {
                Some(Expression::CallExpression(c))
                    if ctx.file.path.ends_with(".vue")
                        && matches!(&c.callee, Expression::Identifier(d) if d.name == "defineEmits") =>
                {
                    Some(Emitter::DefineEmits(c))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        if !is_vue3_project(ctx) {
            return;
        }
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let Some(name_arg) = call.arguments.first().and_then(|a| a.as_expression()) else {
            return;
        };
        let Some(name) = string_value(name_arg) else {
            return;
        };
        let Some(emitter) = emitter(ctx, node, call) else {
            return;
        };
        let (declared, kind) = match emitter {
            Emitter::Options(obj) => (
                declared_names(find_property(obj, "emits").map(|p| &p.value)),
                "`emits` option",
            ),
            Emitter::DefineEmits(c) => {
                if c.type_arguments.is_some() {
                    return;
                }
                (
                    declared_names(c.arguments.first().and_then(|a| a.as_expression())),
                    "`defineEmits`",
                )
            }
        };
        let Some(declared) = declared else { return };
        if declared.iter().any(|d| d == name) {
            return;
        }
        ctx.report(
            name_arg.span(),
            format!("The \"{name}\" event has been triggered but not declared on {kind}."),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::helpers::batch_smell4::{check_project, project_with};

    const CODE: &str = r#"
export default defineComponent({
  emits: ['declared', 'other'],
  methods: {
    a() {
      this.$emit('declared');
      this.$emit('missing'); // Noncompliant {{The "missing" event has been triggered but not declared on `emits` option.}}
    },
  },
  setup(props, { emit }) {
    emit('declared');
    emit('nope'); // Noncompliant
  },
});
defineComponent({
  setup(props, context) {
    context.emit('x'); // Noncompliant
  },
});
defineComponent({
  emits: { ok: null, ...more },
  methods: { f() { this.$emit('whatever'); } },
});
defineComponent({
  emits: list,
  methods: { f() { this.$emit('whatever'); } },
});
other.$emit('free');
"#;

    #[test]
    fn rule() {
        check_project("S8961", "a.js", &project_with(&[("vue", "^3.2.0")]), CODE);
        let no_issue = CODE.replace("// Noncompliant", "//");
        check_project(
            "S8961",
            "a.js",
            &project_with(&[("vue", "^2.6.0")]),
            &no_issue,
        );
        check_project(
            "S8961",
            "a.vue",
            &project_with(&[("vue", "^3.2.0")]),
            r#"<template><div/></template>
<script setup>
const emit = defineEmits(['change']);
emit('change');
emit('input'); // Noncompliant {{The "input" event has been triggered but not declared on `defineEmits`.}}
</script>
"#,
        );
    }
}
