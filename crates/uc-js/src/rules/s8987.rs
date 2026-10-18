//! "v-if" and "v-for" should not be used on the same element.
use crate::helpers::vue::{mentions, parse_template, v_for_parts};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if !ctx.file.path.ends_with(".vue") {
            return;
        }
        for e in parse_template(ctx.file.raw) {
            let (Some(vfor), Some(vif)) = (e.directive("for", None), e.directive("if", None)) else { continue };
            let Some((for_value, _)) = vfor.value else { continue };
            let Some((aliases, iterable)) = v_for_parts(for_value) else { continue };
            let cond = vif.value.map_or("", |(v, _)| v);
            if aliases.iter().any(|a| mentions(cond, a)) {
                ctx.report(vif.span, format!("Replace `{iterable}` with a computed property that returns a filtered array."));
            } else {
                ctx.report(vif.span, "Move this `v-if` to a parent element.");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn v_if_with_v_for() {
        check(
            "S8987",
            "a.vue",
            r#"<template>
  <ul v-if="showList">
    <li v-for="user in users" :key="user.id">{{ user.name }}</li>
    <li v-for="user in users" v-if="user.active" :key="user.id"></li> <!-- Noncompliant {{Replace `users` with a computed property that returns a filtered array.}} -->
    <li v-for="user in getUsers()" v-if="user.active" :key="user.id"></li> <!-- Noncompliant {{Replace `getUsers()` with a computed property that returns a filtered array.}} -->
    <li v-for="user in users" v-if="showList" :key="user.id"></li> <!-- Noncompliant {{Move this `v-if` to a parent element.}} -->
  </ul>
</template>
<script setup lang="ts">
const props = defineProps<{ users: { name: string }[]; showList: boolean }>();
</script>
"#,
        );
    }
}
