//! Destructuring syntax should be used for assignments
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MAX_INDEX: f64 = 4.0;

struct Candidate<'a> {
    key: &'a str,
    span: Span,
    kind: VariableDeclarationKind,
}

fn flush(ctx: &Ctx<'_, '_>, group: &mut Vec<Candidate<'_>>) {
    // Group by object text, keeping first-seen order.
    let mut keys: Vec<&str> = Vec::new();
    let mut by_key: FxHashMap<&str, Vec<&Candidate<'_>>> = FxHashMap::default();
    for c in group.iter() {
        by_key
            .entry(c.key)
            .or_insert_with(|| {
                keys.push(c.key);
                Vec::new()
            })
            .push(c);
    }
    for key in keys {
        let decls = &by_key[key];
        if decls.len() > 1 && decls[1..].iter().all(|d| d.kind == decls[0].kind) {
            ctx.report_with(
                decls[0].span,
                format!("Use destructuring syntax for these assignments from \"{key}\"."),
                decls[1..]
                    .iter()
                    .map(|d| (d.span, Some("Replace this assignment.".to_string())))
                    .collect::<Vec<_>>(),
            );
        }
    }
    group.clear();
}

fn visit<'a>(ctx: &Ctx<'a, '_>, stmts: &[Statement<'a>]) {
    let mut group: Vec<Candidate<'a>> = Vec::new();
    for stmt in stmts {
        let Statement::VariableDeclaration(vd) = stmt else {
            if !group.is_empty() {
                flush(ctx, &mut group);
            }
            continue;
        };
        for d in &vd.declarations {
            let (BindingPattern::BindingIdentifier(id), Some(init)) = (&d.id, &d.init) else {
                continue;
            };
            let object = match init {
                Expression::StaticMemberExpression(m) if m.property.name == id.name => &m.object,
                Expression::ComputedMemberExpression(m) => {
                    let ok = match &m.expression {
                        Expression::Identifier(p) => p.name == id.name,
                        Expression::NumericLiteral(n) => (0.0..=MAX_INDEX).contains(&n.value),
                        _ => false,
                    };
                    if !ok {
                        continue;
                    }
                    &m.object
                }
                _ => continue,
            };
            group.push(Candidate {
                key: ctx.text_of(object),
                span: d.span,
                kind: vd.kind,
            });
        }
    }
    if !group.is_empty() {
        flush(ctx, &mut group);
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::Program,
            AstType::BlockStatement,
            AstType::FunctionBody,
            AstType::SwitchCase,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::Program(p) => visit(ctx, &p.body),
            AstKind::BlockStatement(b) => visit(ctx, &b.body),
            AstKind::FunctionBody(b) => visit(ctx, &b.statements),
            AstKind::SwitchCase(c) => visit(ctx, &c.consequent),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn destructuring() {
        check(
            "S3514",
            "a.js",
            r#"
function f(obj, arr) {
  const a = obj.a; // Noncompliant {{Use destructuring syntax for these assignments from "obj".}}
  const b = obj.b;
  const x = arr[0]; // Noncompliant {{Use destructuring syntax for these assignments from "arr".}}
  const y = arr[1];
  foo();
  const c = obj.c;
  let d = obj.d;
  const e = obj.e;
  const g = obj.other;
  const z = arr[9];
  const w = arr[8];
}
"#,
        );
    }
}
