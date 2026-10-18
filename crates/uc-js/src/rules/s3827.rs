//! Variables should be defined before being used
use crate::helpers::batch_bug2::KNOWN_GLOBALS;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// The global object inherits these from `Object.prototype` (the browser environment lists them).
const OBJECT_PROTOTYPE: [&str; 7] = [
    "constructor",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toLocaleString",
    "toString",
    "valueOf",
];

/// `<script setup>` compiler macros.
const VUE_MACROS: [&str; 6] = [
    "defineProps",
    "defineEmits",
    "defineExpose",
    "defineOptions",
    "defineSlots",
    "withDefaults",
];

/// Names declared with `/* global a, b:writable */` comments.
fn comment_globals<'a>(ctx: &Ctx<'a, '_>) -> FxHashSet<&'a str> {
    let mut out = FxHashSet::default();
    for c in ctx.comments() {
        if !c.is_block() {
            continue;
        }
        let text = ctx.comment_text(c).trim_start();
        let rest = if let Some(r) = text.strip_prefix("globals") {
            r
        } else if let Some(r) = text.strip_prefix("global") {
            r
        } else {
            continue;
        };
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        for item in rest.split([',', '\n', '\r', '\t', ' ']) {
            let name = item.split(':').next().unwrap_or("").trim();
            if !name.is_empty() {
                out.insert(name);
            }
        }
    }
    out
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let scoping = ctx.scoping();
        let unresolved = scoping.root_unresolved_references();
        if unresolved.is_empty() {
            return;
        }
        let declared = comment_globals(ctx);
        let vue = ctx.file.path.ends_with(".vue");
        for (name, refs) in unresolved.iter() {
            let name: &str = name.as_str();
            if name == "arguments"
                || KNOWN_GLOBALS.binary_search(&name).is_ok()
                || OBJECT_PROTOTYPE.contains(&name)
                || (vue && VUE_MACROS.contains(&name))
                || declared.contains(name)
            {
                continue;
            }
            let mut ids: Vec<(Span, NodeId, bool)> = Vec::with_capacity(refs.len());
            for &r in refs.iter() {
                let reference = scoping.get_reference(r);
                if reference.is_type() && !reference.is_value() {
                    continue;
                }
                let node_id = reference.node_id();
                ids.push((ctx.node(node_id).span(), node_id, reference.is_write()));
            }
            ids.sort_unstable_by_key(|(s, _, _)| s.start);
            let mut spans: Vec<Span> = Vec::new();
            for (span, node_id, is_write) in ids {
                let parent = ctx.parent_skip_parens(node_id);
                let excluded = is_write
                    || matches!(parent.kind(), AstKind::UnaryExpression(u) if u.operator == UnaryOperator::Typeof)
                    || ctx
                        .nodes()
                        .ancestor_kinds(node_id)
                        .any(|k| matches!(k, AstKind::WithStatement(_)));
                if excluded {
                    break;
                }
                if matches!(
                    parent.kind(),
                    AstKind::JSXOpeningElement(_)
                        | AstKind::JSXClosingElement(_)
                        | AstKind::JSXMemberExpression(_)
                ) {
                    continue;
                }
                spans.push(span);
            }
            if let Some((first, rest)) = spans.split_first() {
                ctx.report_with(
                    *first,
                    format!(
                        "\"{name}\" does not exist. Change its name or declare it so that its usage doesn't result in a \"ReferenceError\"."
                    ),
                    rest.iter().map(|s| (*s, None)),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn undefined_variables() {
        check(
            "S3827",
            "a.js",
            r#"
function foo() {}
var a1;
foo(a1);
foo(a2);
var a2;
a3 = "";
foo(a3);
typeof x3;
if (typeof x5 !== "undefined") { foo(x5); }
foo(x1); // Noncompliant {{"x1" does not exist. Change its name or declare it so that its usage doesn't result in a "ReferenceError".}}
foo(x1);
typeof x4.x; // Noncompliant
console.log(window.document, require, module, process, $, _);
function f() { return arguments.length; }
/* global myGlobal, other:writable */
myGlobal.call(other);
with (something) { foo(bar); }
const C = <Comp />;
"#,
        );
    }
}
