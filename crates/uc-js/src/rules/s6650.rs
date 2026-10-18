//! Renaming import, export, and destructuring assignments should not be to the same name
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn export_name<'a>(n: &'a ModuleExportName<'a>) -> &'a str {
    match n {
        ModuleExportName::IdentifierName(i) => i.name.as_str(),
        ModuleExportName::IdentifierReference(i) => i.name.as_str(),
        ModuleExportName::StringLiteral(s) => s.value.as_str(),
    }
}

fn key_name<'a>(k: &'a PropertyKey<'a>) -> Option<&'a str> {
    match k {
        PropertyKey::StaticIdentifier(i) => Some(i.name.as_str()),
        PropertyKey::StringLiteral(s) => Some(s.value.as_str()),
        _ => None,
    }
}

fn report(ctx: &Ctx<'_, '_>, span: Span, kind: &str, name: &str) {
    ctx.report(span, format!("{kind} {name} unnecessarily renamed."));
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::BindingProperty,
            AstType::AssignmentTargetPropertyProperty,
            AstType::ImportSpecifier,
            AstType::ExportSpecifier,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::BindingProperty(p) => {
                if p.shorthand || p.computed {
                    return;
                }
                let renamed = match &p.value {
                    BindingPattern::BindingIdentifier(id) => id.name.as_str(),
                    BindingPattern::AssignmentPattern(a) => match &a.left {
                        BindingPattern::BindingIdentifier(id) => id.name.as_str(),
                        _ => return,
                    },
                    _ => return,
                };
                if let Some(key) = key_name(&p.key)
                    && key == renamed
                {
                    report(ctx, p.span, "Destructuring assignment", key);
                }
            }
            AstKind::AssignmentTargetPropertyProperty(p) => {
                if p.computed {
                    return;
                }
                let target = match &p.binding {
                    AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(d) => &d.binding,
                    other => match other.as_assignment_target() {
                        Some(t) => t,
                        None => return,
                    },
                };
                let AssignmentTarget::AssignmentTargetIdentifier(id) = target else {
                    return;
                };
                if let Some(key) = key_name(&p.name)
                    && key == id.name.as_str()
                {
                    report(ctx, p.span, "Destructuring assignment", key);
                }
            }
            AstKind::ImportSpecifier(s) => {
                if s.imported.span() != s.local.span
                    && export_name(&s.imported) == s.local.name.as_str()
                {
                    report(ctx, s.span, "Import", export_name(&s.imported));
                }
            }
            AstKind::ExportSpecifier(s)
                if s.local.span() != s.exported.span()
                    && export_name(&s.local) == export_name(&s.exported) =>
            {
                report(ctx, s.span, "Export", export_name(&s.local));
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn useless_rename() {
        check(
            "S6650",
            "a.js",
            r#"
import { a as a } from 'mod';     // Noncompliant {{Import a unnecessarily renamed.}}
import { b as c } from 'mod';
import { d } from 'mod';
const { e: e } = obj;             // Noncompliant {{Destructuring assignment e unnecessarily renamed.}}
const { f: g, h } = obj;
const { i: i = 1 } = obj;         // Noncompliant
({ j: j } = obj);                 // Noncompliant
({ k: l } = obj);
export { a as a };                // Noncompliant {{Export a unnecessarily renamed.}}
export { c as m };
export { d };
"#,
        );
    }
}
