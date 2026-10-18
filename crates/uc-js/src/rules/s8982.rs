//! HTML elements with v-for should have a :key attribute.
use crate::helpers::vue::{Element, is_custom_component, parse_template};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn check(ctx: &Ctx<'_, '_>, els: &[Element<'_>], i: usize, under_keyed_template: bool) {
    let e = &els[i];
    let lower = e.name.to_ascii_lowercase();
    if lower == "template" || lower == "slot" {
        let keyed = under_keyed_template || e.has_key_binding();
        for &c in &e.children {
            check(ctx, els, c, keyed);
        }
    } else if !is_custom_component(e) && !e.has_key_binding() && !under_keyed_template {
        ctx.report(e.start_tag, "Add a ':key' binding to this element.");
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if !ctx.file.path.ends_with(".vue") {
            return;
        }
        let els = parse_template(ctx.file.raw);
        for (i, e) in els.iter().enumerate() {
            if e.directive("for", None).is_some() {
                check(ctx, &els, i, false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn v_for_key() {
        check(
            "S8982",
            "a.vue",
            r#"<template>
  <ul>
    <li v-for="todo in todos" :key="todo.id">{{ todo.text }}</li>
    <li v-for="item in items" v-bind:key="item.id">{{ item }}</li>
    <MyComponent v-for="item in items">{{ item }}</MyComponent>
    <li v-for="todo in todos">{{ todo.text }}</li> <!-- Noncompliant -->
  </ul>
  <div v-for="group in groups" :key="group.id">
    <span v-for="item in group.items">{{ item }}</span> <!-- Noncompliant -->
  </div>
  <template v-for="x in xs">
    <p>{{ x }}</p> <!-- Noncompliant -->
  </template>
  <template v-for="x in xs" :key="x">
    <p>{{ x }}</p>
  </template>
</template>
<script>
export default {};
</script>
"#,
        );
    }
}
