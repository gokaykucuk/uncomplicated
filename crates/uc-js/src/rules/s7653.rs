//! Output bindings should not be aliased
use crate::helpers::batch_ts::{
    alias_allowed_by_selector, angular_bindings, angular_decorator, capitalize, decorator_metadata,
    object_property,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// `@Directive({ selector: 'foo[bar]' })` → `fooBar`, the identity directive aliases derive from.
fn compound_selector_identity(class: &Class<'_>) -> Option<String> {
    let (name, d) = angular_decorator(class, &["Component", "Directive"])?;
    if name != "Directive" {
        return None;
    }
    let meta = decorator_metadata(d)?;
    let Expression::StringLiteral(s) = strip_parens(&object_property(meta, "selector")?.value)
    else {
        return None;
    };
    let sel = s.value.as_str();
    let open = sel.find('[')?;
    let (element, attr) = (&sel[..open], sel[open + 1..].strip_suffix(']')?);
    let valid = |p: &str| {
        p.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    };
    (valid(element) && valid(attr)).then(|| format!("{element}{}", capitalize(attr)))
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Class]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::Class(class) = node.kind() else {
            return;
        };
        let bindings = angular_bindings(ctx, class, "Output", "output");
        if bindings.is_empty() {
            return;
        }
        let identity = compound_selector_identity(class);
        for b in bindings {
            let Some((span, alias)) = b.alias else {
                continue;
            };
            if alias != b.name {
                if alias_allowed_by_selector(class, b.name, alias) {
                    continue;
                }
                if let Some(id) = &identity
                    && (alias == id || alias == format!("{id}{}", capitalize(b.name)))
                {
                    continue;
                }
            }
            ctx.report(span, "Output bindings should not be aliased");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn output_rename() {
        check(
            "S7653",
            "a.ts",
            r#"
import { output } from '@angular/core';
@Component({ selector: 'app-a' })
class A {
  @Output('changed') change = new EventEmitter(); // Noncompliant {{Output bindings should not be aliased}}
  @Output() plain = new EventEmitter();
  sig = output({ alias: 'other' }); // Noncompliant
}
@Directive({ selector: 'foo[bar]' })
class D {
  @Output('fooBar') event = new EventEmitter();
  @Output('fooBarChanged') changed = new EventEmitter();
  @Output('unrelated') x = new EventEmitter(); // Noncompliant
}
"#,
        );
    }
}
