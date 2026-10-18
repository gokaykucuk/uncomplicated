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

/// `<script setup>`: top-level bindings named like a `defineProps` prop.
fn script_setup_props<'a>(ctx: &Ctx<'a, '_>, node: &AstNode<'a>, c: &CallExpression<'a>) {
    let mut names: Vec<&str> = Vec::new();
    match c
        .arguments
        .first()
        .and_then(|a| a.as_expression())
        .map(strip_ts)
    {
        Some(Expression::ObjectExpression(o)) => {
            let mut keys = Vec::new();
            object_keys(o, &mut keys);
            names.extend(keys.into_iter().map(|(n, _)| n));
        }
        Some(Expression::ArrayExpression(a)) => {
            for el in &a.elements {
                if let Some(Expression::StringLiteral(s)) = el.as_expression() {
                    names.push(s.value.as_str());
                }
            }
        }
        _ => {}
    }
    if let Some(targs) = &c.type_arguments
        && let Some(t) = targs.params.first()
    {
        let literals: Vec<&TSTypeLiteral<'a>> = match t {
            TSType::TSTypeLiteral(lit) => vec![lit],
            TSType::TSIntersectionType(i) => i
                .types
                .iter()
                .filter_map(|t| match t {
                    TSType::TSTypeLiteral(lit) => Some(&**lit),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        for lit in literals {
            for m in &lit.members {
                if let TSSignature::TSPropertySignature(p) = m
                    && !p.computed
                    && let Some(n) = property_key_name(&p.key)
                {
                    names.push(n);
                }
            }
        }
    }
    // The props pattern: `const props = defineProps()` / `withDefaults(defineProps(), {})`.
    let mut target = node;
    let parent = ctx.parent_skip_parens(node.id());
    if let AstKind::CallExpression(w) = parent.kind()
        && matches!(&w.callee, Expression::Identifier(id) if id.name == "withDefaults")
        && w.arguments.first().is_some_and(|a| a.span() == c.span)
    {
        target = parent;
    }
    let pattern = match ctx.parent_skip_parens(target.id()).kind() {
        AstKind::VariableDeclarator(v)
            if v.init
                .as_ref()
                .is_some_and(|i| i.span().contains_inclusive(target.span())) =>
        {
            Some(&v.id)
        }
        _ => None,
    };
    // References to the props object, plus the `defineProps()` call itself.
    let mut prop_refs: Vec<Span> = vec![c.span];
    let mut renamed: Vec<&str> = Vec::new();
    if let Some(pattern) = pattern {
        collect_refs(ctx, pattern, &mut prop_refs);
        if let BindingPattern::ObjectPattern(o) = pattern {
            for p in &o.properties {
                if let (PropertyKey::StaticIdentifier(k), BindingPattern::BindingIdentifier(v)) =
                    (&p.key, &p.value)
                    && k.name != v.name
                {
                    renamed.push(k.name.as_str());
                }
            }
        }
    }
    let scoping = ctx.scoping();
    for name in names {
        if renamed.contains(&name) {
            continue;
        }
        let Some(sym) = scoping.get_root_binding(name.into()) else {
            continue;
        };
        let decl = ctx.symbol_decl(sym);
        let span = match decl.kind() {
            AstKind::VariableDeclarator(v) => {
                let reads_props = v.init.as_ref().is_some_and(|init| {
                    let r = init.span();
                    prop_refs
                        .iter()
                        .any(|p| r.start <= p.start && p.end <= r.end)
                });
                if reads_props && !aliases_other_prop(v, name, &prop_refs) {
                    continue;
                }
                v.span
            }
            AstKind::Function(f) => f.span,
            AstKind::Class(cl) => cl.span,
            AstKind::ImportSpecifier(i) => i.span,
            AstKind::ImportDefaultSpecifier(i) => i.span,
            AstKind::ImportNamespaceSpecifier(i) => i.span,
            _ => continue,
        };
        ctx.report(span, format!("Duplicate key '{name}'."));
    }
}

fn collect_refs(ctx: &Ctx<'_, '_>, p: &BindingPattern<'_>, out: &mut Vec<Span>) {
    match p {
        BindingPattern::BindingIdentifier(id) => {
            if let Some(sym) = id.symbol_id.get() {
                for r in ctx.scoping().get_resolved_references(sym) {
                    out.push(ctx.node(r.node_id()).span());
                }
            }
        }
        BindingPattern::ObjectPattern(o) => {
            for prop in &o.properties {
                collect_refs(ctx, &prop.value, out);
            }
            if let Some(rest) = &o.rest {
                collect_refs(ctx, &rest.argument, out);
            }
        }
        BindingPattern::AssignmentPattern(a) => collect_refs(ctx, &a.left, out),
        BindingPattern::ArrayPattern(_) => {}
    }
}

fn is_prop_ref(e: &Expression<'_>, refs: &[Span]) -> bool {
    refs.contains(&strip_ts(e).span())
}

/// `const foo = props.bar` / `toRef(props, 'bar')` / `const { bar: foo } = props`: `foo` names
/// another prop, so it is still a duplicate of the `foo` prop.
fn aliases_other_prop(v: &VariableDeclarator<'_>, name: &str, refs: &[Span]) -> bool {
    let Some(init) = &v.init else { return false };
    let init = match strip_ts(init) {
        Expression::ChainExpression(c) => match c.expression.as_member_expression() {
            Some(m) => {
                return matches!(&v.id, BindingPattern::BindingIdentifier(id) if id.name == name)
                    && is_prop_ref(m.object(), refs)
                    && m.static_property_name().is_some_and(|n| n != name);
            }
            None => return false,
        },
        e => e,
    };
    match &v.id {
        BindingPattern::BindingIdentifier(id) if id.name == name => {
            if let Some(m) = init.as_member_expression() {
                return is_prop_ref(m.object(), refs)
                    && m.static_property_name().is_some_and(|n| n != name);
            }
            if let Expression::CallExpression(call) = init
                && matches!(&call.callee, Expression::Identifier(f) if f.name == "toRef")
                && call
                    .arguments
                    .first()
                    .and_then(|a| a.as_expression())
                    .is_some_and(|a| is_prop_ref(a, refs))
            {
                return matches!(call.arguments.get(1), Some(Argument::StringLiteral(s)) if s.value != name);
            }
            false
        }
        BindingPattern::ObjectPattern(o) => {
            let is_props_object = is_prop_ref(init, refs)
                || matches!(init, Expression::CallExpression(call)
                    if matches!(&call.callee, Expression::Identifier(f) if f.name == "toRefs")
                        && call.arguments.first().and_then(|a| a.as_expression()).is_some_and(|a| is_prop_ref(a, refs)));
            if !is_props_object {
                return false;
            }
            for p in &o.properties {
                let value = match &p.value {
                    BindingPattern::AssignmentPattern(a) => &a.left,
                    v => v,
                };
                if let BindingPattern::BindingIdentifier(id) = value
                    && id.name == name
                {
                    return property_key_name(&p.key).is_some_and(|k| k != name);
                }
            }
            false
        }
        _ => false,
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
        if let AstKind::CallExpression(c) = &kind
            && ctx.file.path.ends_with(".vue")
            && matches!(&c.callee, Expression::Identifier(id) if ctx.is_global(id, "defineProps"))
        {
            script_setup_props(ctx, node, c);
            return;
        }
        let Some(component) = vue_component_object(ctx, &kind) else {
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
    fn script_setup() {
        check(
            "S9128",
            "a.vue",
            r#"
<script setup lang="ts">
import { computed, toRefs } from 'vue'
const props = defineProps({ team: String, open: Boolean })
const team = useTeamStore() // Noncompliant {{Duplicate key 'team'.}}
function open() {} // Noncompliant
const other = 1
</script>
"#,
        );
        check(
            "S9128",
            "b.vue",
            r#"
<script setup lang="ts">
const props = withDefaults(defineProps<{ data?: string; headers?: string }>(), {})
const { data, headers: rawHeaders } = toRefs(props)
const headers = computed(() => rawHeaders.value) // Noncompliant
</script>
"#,
        );
        check(
            "S9128",
            "c.vue",
            r#"
<script setup lang="ts">
const props = defineProps<{ value: string; html: string; sponsor: string; other: string }>()
const value = useVModel(props, 'value', emit)
const sponsor = ref(props.sponsor)
const html = props.other // Noncompliant
</script>
"#,
        );
    }

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
