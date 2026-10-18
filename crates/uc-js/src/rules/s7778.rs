//! Multiple consecutive calls to methods that accept multiple arguments should be combined
use crate::helpers::batch_smell4::sibling_statements;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

#[derive(PartialEq)]
enum Case {
    Push,
    Unshift,
    ClassListAdd,
    ClassListRemove,
    ImportScripts,
}

fn classify(ctx: &Ctx<'_, '_>, call: &CallExpression<'_>) -> Option<Case> {
    if call.optional {
        return None;
    }
    match &call.callee {
        Expression::StaticMemberExpression(m) if !m.optional => match m.property.name.as_str() {
            "push" => Some(Case::Push),
            "unshift" => Some(Case::Unshift),
            name @ ("add" | "remove") => {
                let Expression::StaticMemberExpression(cl) = &m.object else {
                    return None;
                };
                if cl.property.name != "classList" || cl.optional {
                    return None;
                }
                Some(if name == "add" {
                    Case::ClassListAdd
                } else {
                    Case::ClassListRemove
                })
            }
            _ => None,
        },
        Expression::Identifier(id) if ctx.is_global(id, "importScripts") => {
            Some(Case::ImportScripts)
        }
        _ => None,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ExpressionStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::ExpressionStatement(stmt) = node.kind() else {
            return;
        };
        let Expression::CallExpression(second) = &stmt.expression else {
            return;
        };
        let Some(case) = classify(ctx, second) else {
            return;
        };
        let Some(siblings) = sibling_statements(ctx, node) else {
            return;
        };
        let Some(i) = siblings.iter().position(|s| s.span() == stmt.span) else {
            return;
        };
        let Some(Statement::ExpressionStatement(prev)) = i.checked_sub(1).map(|j| &siblings[j])
        else {
            return;
        };
        let Expression::CallExpression(first) = &prev.expression else {
            return;
        };
        if classify(ctx, first).as_ref() != Some(&case) {
            return;
        }
        let (report_span, description) = match (&first.callee, &second.callee) {
            (Expression::StaticMemberExpression(a), Expression::StaticMemberExpression(b)) => {
                if !ctx.same_tokens(a.object.span(), b.object.span()) {
                    return;
                }
                let receiver = match case {
                    Case::Push | Case::Unshift => &b.object,
                    _ => match &b.object {
                        Expression::StaticMemberExpression(cl) => &cl.object,
                        _ => return,
                    },
                };
                let t = type_of(ctx, receiver);
                // In plain JavaScript an untyped receiver counts as an array, like `any` in SonarJS.
                let array_like = t.is_array()
                    || (t == Ty::Unknown
                        && !ctx.is_ts()
                        && !matches!(
                            dotted_path(receiver).as_deref(),
                            Some(
                                "stream"
                                    | "this"
                                    | "this.stream"
                                    | "process.stdin"
                                    | "process.stdout"
                                    | "process.stderr"
                            )
                        ));
                let description = match case {
                    // Only arrays are known to accept several values in `push`.
                    Case::Push if array_like => "Array#push()",
                    Case::Unshift if array_like => "Array#unshift()",
                    Case::ClassListAdd if !matches!(t, Ty::Named(_)) => "Element#classList.add()",
                    Case::ClassListRemove if !matches!(t, Ty::Named(_)) => {
                        "Element#classList.remove()"
                    }
                    _ => return,
                };
                (b.property.span, description)
            }
            (Expression::Identifier(_), Expression::Identifier(b)) => (b.span, "importScripts()"),
            _ => return,
        };
        ctx.report(
            report_span,
            format!("Merge these {description} calls into a single call with multiple arguments."),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S7778",
            "a.ts",
            r#"
const items: number[] = [];
items.push(1);
items.push(2); // Noncompliant {{Merge these Array#push() calls into a single call with multiple arguments.}}
items.push(3); // Noncompliant
const element = document.createElement('div');
element.classList.add('foo');
element.classList.add('bar'); // Noncompliant {{Merge these Element#classList.add() calls into a single call with multiple arguments.}}
element.classList.remove('foo');
element.classList.remove('bar'); // Noncompliant
importScripts('a.js');
importScripts('b.js'); // Noncompliant {{Merge these importScripts() calls into a single call with multiple arguments.}}
class CustomClass { push(item: number): void {} }
const instance = new CustomClass();
instance.push(1);
instance.push(2);
const other: number[] = [];
items.push(1);
other.push(2);
items.push(1);
foo();
items.push(2);
"#,
        );
    }
}

#[cfg(test)]
mod tests_js {
    use crate::test_util::*;

    #[test]
    fn untyped_javascript() {
        check(
            "S7778",
            "a.js",
            r#"
list.push(1);
list.push(2); // Noncompliant {{Merge these Array#push() calls into a single call with multiple arguments.}}
list.unshift(1);
list.unshift(2); // Noncompliant {{Merge these Array#unshift() calls into a single call with multiple arguments.}}
this.push(1);
this.push(2);
stream.push(1);
stream.push(2);
"#,
        );
    }
}
