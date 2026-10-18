//! Vue v-for directives should be valid.
use crate::helpers::vue::{Element, directive_parts, is_custom_component, mentions, parse_template, v_for_parts};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Checks the `:key` of `e` (and, for `<template>`, of its children) against the v-for aliases.
fn check_key(ctx: &Ctx<'_, '_>, els: &[Element<'_>], i: usize, aliases: &[&str]) {
    let e = &els[i];
    if e.name.eq_ignore_ascii_case("template") || e.name.eq_ignore_ascii_case("slot") {
        if let Some(key) = e.directive("bind", Some("key")) {
            check_uses(ctx, key, aliases);
            return;
        }
        for &c in &e.children {
            if els[c].directive("for", None).is_none() {
                check_key(ctx, els, c, aliases);
            }
        }
        return;
    }
    match e.directive("bind", Some("key")) {
        Some(key) => check_uses(ctx, key, aliases),
        None if is_custom_component(e) => ctx.report(e.start_tag, "Add a ':key' binding to this custom component."),
        None => {}
    }
}

fn check_uses(ctx: &Ctx<'_, '_>, key: &crate::helpers::vue::Attr<'_>, aliases: &[&str]) {
    if let Some((v, _)) = key.value
        && !aliases.is_empty()
        && !aliases.iter().any(|a| mentions(v, a))
    {
        ctx.report(key.span, "':key' must use a variable defined by this 'v-for' directive.");
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if !ctx.file.path.ends_with(".vue") {
            return;
        }
        let els = parse_template(ctx.file.raw);
        for (i, e) in els.iter().enumerate() {
            let Some(d) = e.directive("for", None) else { continue };
            if directive_parts(d.name).1.is_some() {
                ctx.report(d.span, "'v-for' directives require no argument.");
                continue;
            }
            if d.name.strip_prefix("v-for").is_some_and(|r| r.starts_with('.')) {
                ctx.report(d.span, "'v-for' directives require no modifier.");
                continue;
            }
            let Some((value, _)) = d.value.filter(|(v, _)| !v.trim().is_empty()) else {
                ctx.report(d.span, "'v-for' directives require that attribute value.");
                continue;
            };
            let Some((aliases, _)) = v_for_parts(value) else { continue };
            check_key(ctx, &els, i, &aliases);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn valid_v_for() {
        check(
            "S8984",
            "a.vue",
            r#"<template>
  <div>
    <li v-for="x in xs" :key="x.id"></li>
    <li v-for></li> <!-- Noncompliant {{'v-for' directives require that attribute value.}} -->
    <li v-for:a="x in xs" :key="x"></li> <!-- Noncompliant {{'v-for' directives require no argument.}} -->
    <li v-for.m="x in xs" :key="x"></li> <!-- Noncompliant {{'v-for' directives require no modifier.}} -->
    <MyItem v-for="x in xs"></MyItem> <!-- Noncompliant {{Add a ':key' binding to this custom component.}} -->
    <MyItem v-for="(x, i) in xs" :key="i"></MyItem>
    <li v-for="x in xs" :key="other"></li> <!-- Noncompliant {{':key' must use a variable defined by this 'v-for' directive.}} -->
  </div>
</template>
"#,
        );
    }
}
