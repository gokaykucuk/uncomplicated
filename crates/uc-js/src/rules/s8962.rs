//! Vue ref and shallowRef calls should be typed
use crate::helpers::batch_ts::imported_name;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// True if `callee` is `ref` / `shallowRef` from `vue` (named import or namespace member).
fn is_vue_ref<'a>(ctx: &Ctx<'a, '_>, callee: &Expression<'a>) -> bool {
    match strip_parens(callee) {
        Expression::Identifier(id) => {
            matches!(imported_name(ctx, id, "vue"), Some("ref" | "shallowRef"))
        }
        Expression::StaticMemberExpression(m)
            if matches!(m.property.name.as_str(), "ref" | "shallowRef") =>
        {
            let Expression::Identifier(ns) = strip_parens(&m.object) else {
                return false;
            };
            let Some(sym) = ctx.symbol_of(ns) else {
                return false;
            };
            let decl = ctx.symbol_decl(sym);
            matches!(decl.kind(), AstKind::ImportNamespaceSpecifier(_))
                && matches!(ctx.parent_kind(decl.id()), AstKind::ImportDeclaration(i) if i.source.value == "vue")
        }
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        // Upstream only checks `.ts` files (and `<script lang="ts">` blocks), not `.tsx`.
        if call.type_arguments.is_some() || ctx.file.path.ends_with(".tsx") {
            return;
        }
        let untyped_arg = match call.arguments.first() {
            None => true,
            Some(Argument::NullLiteral(_)) => true,
            Some(Argument::Identifier(id)) => id.name == "undefined",
            _ => false,
        };
        if !untyped_arg || !is_vue_ref(ctx, &call.callee) {
            return;
        }
        match ctx.parent_kind(node.id()) {
            AstKind::VariableDeclarator(v) if v.type_annotation.is_some() => return,
            AstKind::PropertyDefinition(p) if p.type_annotation.is_some() => return,
            _ => {}
        }
        ctx.report(call.span, "Specify type parameter.");
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn typed_ref() {
        check(
            "S8962",
            "a.ts",
            r#"
import { ref, shallowRef as sr } from 'vue';
import * as Vue from 'vue';
const a = ref(); // Noncompliant {{Specify type parameter.}}
const b = ref(null); // Noncompliant
const c = sr(undefined); // Noncompliant
const d = Vue.ref(); // Noncompliant
const e = ref<number>();
const f: Ref<number | undefined> = ref();
const g = ref(0);
function other(ref: any) { ref(); }
"#,
        );
    }
}
