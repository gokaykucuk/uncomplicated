//! Default export names and file names should match
//!
//! Sonar raises this as a file-level issue; it is reported at the start of the file here.
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn normalize(s: &str, strip_dots: bool) -> String {
    s.chars()
        .filter(|c| *c != '_' && *c != '-' && !(strip_dots && *c == '.'))
        .flat_map(char::to_lowercase)
        .collect()
}

fn same_name(exported: &str, file: &str) -> bool {
    normalize(exported, false) == normalize(file, true)
}

/// JavaScript's `fileName.slice(0, fileName.lastIndexOf('.'))` (which drops the last
/// character when there is no dot).
fn slice_off_postfix(file: &str) -> &str {
    match file.rfind('.') {
        Some(i) => &file[..i],
        None => {
            let mut chars = file.char_indices();
            chars.next_back().map_or("", |(i, _)| &file[..i])
        }
    }
}

fn default_export_name<'a>(ctx: &Ctx<'a, '_>, d: &ExportDefaultDeclaration<'a>) -> Option<&'a str> {
    match &d.declaration {
        ExportDefaultDeclarationKind::FunctionDeclaration(f) => {
            f.id.as_ref().map(|id| id.name.as_str())
        }
        ExportDefaultDeclarationKind::ClassDeclaration(c) => {
            c.id.as_ref().map(|id| id.name.as_str())
        }
        ExportDefaultDeclarationKind::Identifier(id) => {
            let sym = ctx.symbol_of(id)?;
            if !ctx.scoping().symbol_redeclarations(sym).is_empty() {
                return None;
            }
            let decl = ctx.symbol_decl(sym);
            let ok = match decl.kind() {
                AstKind::Class(_) => true,
                AstKind::Function(f) => f.is_declaration(),
                AstKind::VariableDeclarator(v) => {
                    matches!(v.id, BindingPattern::BindingIdentifier(_))
                        && matches!(ctx.parent_kind(decl.id()), AstKind::VariableDeclaration(vd) if vd.kind == VariableDeclarationKind::Const)
                }
                _ => false,
            };
            ok.then_some(id.name.as_str())
        }
        _ => None,
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let program = ctx.nodes().program();
        let mut name = None;
        for stmt in &program.body {
            match stmt {
                Statement::ExportDefaultDeclaration(d) => {
                    if let Some(n) = default_export_name(ctx, d) {
                        name = Some(n);
                    }
                }
                Statement::ExportAllDeclaration(_)
                | Statement::ExportNamedDeclaration(_)
                | Statement::ExportDeclaration(_)
                | Statement::ExportFromDeclaration(_) => return,
                _ => {}
            }
        }
        let Some(exported) = name else { return };
        let path = ctx.file.path.replace('\\', "/");
        let base = path.rsplit('/').next().unwrap_or(&path);
        let stem = match base.rfind('.') {
            Some(i) if i > 0 => &base[..i],
            _ => base,
        };
        if stem == "index"
            || same_name(exported, stem)
            || same_name(exported, slice_off_postfix(stem))
        {
            return;
        }
        ctx.report(
            Span::new(0, 0),
            format!("Rename this file to \"{exported}\""),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn mismatch() {
        check(
            "S3317",
            "src/my_class.js",
            r#"// Noncompliant {{Rename this file to "Foo"}}
export default class Foo {}
"#,
        );
    }

    #[test]
    fn matching_names() {
        check(
            "S3317",
            "src/my-class.js",
            "export default class MyClass {}\n",
        );
        check(
            "S3317",
            "src/MyClass.component.ts",
            "export default class MyClass {}\n",
        );
        check("S3317", "src/index.js", "export default class MyClass {}\n");
        check(
            "S3317",
            "src/other.js",
            "export default class MyClass {}\nexport const x = 1;\n",
        );
        check(
            "S3317",
            "src/other.js",
            "let Foo = 1;\nexport default Foo;\n",
        );
        check("S3317", "src/other.js", "export default function () {}\n");
    }

    #[test]
    fn identifier_export() {
        check(
            "S3317",
            "src/other.js",
            r#"// Noncompliant {{Rename this file to "Foo"}}
const Foo = 1;
export default Foo;
"#,
        );
    }
}
