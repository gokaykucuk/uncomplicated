//! Class names should comply with a naming convention
use crate::helpers::batch_smell1::js_regex;
use crate::prelude::*;

const DEFAULT_FORMAT: &str = r"^\$?[A-Z][a-zA-Z0-9]*$";

pub struct Rule {
    format: String,
    re: Option<regex::Regex>,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        let format = p.str("format", DEFAULT_FORMAT).to_string();
        let re = js_regex(&format, "");
        Self { format, re }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Class, AstType::TSInterfaceDeclaration]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let Some(re) = &self.re else { return };
        let (id, what) = match node.kind() {
            AstKind::Class(c) if c.is_declaration() => match &c.id {
                Some(id) => (id, "class"),
                None => return,
            },
            AstKind::TSInterfaceDeclaration(i) => (&i.id, "interface"),
            _ => return,
        };
        if !re.is_match(&id.name) {
            ctx.report(
                id.span,
                format!(
                    "Rename {what} \"{}\" to match the regular expression {}.",
                    id.name, self.format
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn class_names() {
        check(
            "S101",
            "a.ts",
            r#"
class MyClass {}
class my_class {}      // Noncompliant {{Rename class "my_class" to match the regular expression ^\$?[A-Z][a-zA-Z0-9]*$.}}
interface foo {}       // Noncompliant {{Rename interface "foo" to match the regular expression ^\$?[A-Z][a-zA-Z0-9]*$.}}
interface Foo {}
const c = class lower {};
export default class $Dollar {}
abstract class bad_one {} // Noncompliant
"#,
        );
    }
}
