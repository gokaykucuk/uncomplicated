//! Unused private class members should be removed
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

struct Member<'a> {
    name: &'a str,
    key: Span,
    accessor: bool,
    used: bool,
}

/// True if the `#x` access (a `PrivateFieldExpression` node) only writes the member.
fn is_write_only(ctx: &Ctx<'_, '_>, node: &AstNode<'_>, span: Span) -> bool {
    let parent = ctx.parent(node.id());
    match parent.kind() {
        AstKind::AssignmentExpression(a) if a.left.span() == span => {
            a.operator == AssignmentOperator::Assign
                || matches!(
                    ctx.parent_kind(parent.id()),
                    AstKind::ExpressionStatement(_)
                )
        }
        AstKind::ForInStatement(f) => f.left.span() == span,
        AstKind::ForOfStatement(f) => f.left.span() == span,
        // `[this.#x = 1] = arr`
        AstKind::AssignmentTargetWithDefault(a) => a.binding.span() == span,
        // `this.#x++;`
        AstKind::UpdateExpression(_) => matches!(
            ctx.parent_kind(parent.id()),
            AstKind::ExpressionStatement(_)
        ),
        // `({ a: this.#x } = obj)`, `[...this.#x] = arr`, `[this.#x] = arr`
        AstKind::AssignmentTargetPropertyProperty(p) => p.binding.span() == span,
        AstKind::AssignmentTargetRest(_) | AstKind::ArrayAssignmentTarget(_) => true,
        _ => false,
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if memchr::memchr(b'#', ctx.src.as_bytes()).is_none() {
            return;
        }
        let nodes = ctx.nodes();
        // Class body node -> declared private members.
        let mut classes: FxHashMap<NodeId, Vec<Member<'a>>> = FxHashMap::default();
        let mut order: Vec<NodeId> = Vec::new();
        for node in nodes.iter() {
            let AstKind::ClassBody(body) = node.kind() else {
                continue;
            };
            let mut members = Vec::new();
            for el in &body.body {
                let (key, accessor) = match el {
                    ClassElement::MethodDefinition(m) => (
                        &m.key,
                        matches!(
                            m.kind,
                            MethodDefinitionKind::Get | MethodDefinitionKind::Set
                        ),
                    ),
                    ClassElement::PropertyDefinition(p) => (&p.key, false),
                    ClassElement::AccessorProperty(p) => (&p.key, true),
                    _ => continue,
                };
                if let PropertyKey::PrivateIdentifier(id) = key {
                    if let Some(m) = members
                        .iter_mut()
                        .find(|m: &&mut Member| m.name == id.name.as_str())
                    {
                        // getter/setter pair: same member
                        m.accessor = accessor;
                        m.key = id.span;
                        continue;
                    }
                    members.push(Member {
                        name: id.name.as_str(),
                        key: id.span,
                        accessor,
                        used: false,
                    });
                }
            }
            if !members.is_empty() {
                order.push(node.id());
                classes.insert(node.id(), members);
            }
        }
        if classes.is_empty() {
            return;
        }
        for node in nodes.iter() {
            let (name, span, check_write) = match node.kind() {
                AstKind::PrivateFieldExpression(e) => (e.field.name.as_str(), e.span, true),
                AstKind::PrivateInExpression(e) => (e.left.name.as_str(), e.span, false),
                _ => continue,
            };
            // Innermost enclosing class declaring the name.
            let Some(body_id) = nodes.ancestors(node.id()).find_map(|a| {
                matches!(a.kind(), AstKind::ClassBody(_))
                    .then(|| a.id())
                    .filter(|id| {
                        classes
                            .get(id)
                            .is_some_and(|ms| ms.iter().any(|m| m.name == name))
                    })
            }) else {
                continue;
            };
            let members = classes.get_mut(&body_id).unwrap();
            let m = members.iter_mut().find(|m| m.name == name).unwrap();
            if m.used {
                continue;
            }
            if m.accessor || !check_write || !is_write_only(ctx, node, span) {
                m.used = true;
            }
        }
        for id in order {
            for m in &classes[&id] {
                if !m.used {
                    ctx.report(m.key, format!("'#{}' is defined but never used.", m.name));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn private_members() {
        check(
            "S1068",
            "a.js",
            r#"
class A {
  #unused = 1;          // Noncompliant {{'#unused' is defined but never used.}}
  #used = 2;
  #writeOnly;           // Noncompliant
  #incremented = 0;     // Noncompliant
  #readByCompound = 0;
  #method() {}          // Noncompliant
  #called() {}
  get #acc() { return 1; }
  #checked;
  publicField = 1;
  m(o) {
    this.#writeOnly = 3;
    this.#incremented++;
    const x = this.#readByCompound += 1;
    this.#called();
    this.#acc = 4;
    return this.#used + (#checked in o);
  }
}
class B {
  #x;                   // Noncompliant
  m() {
    class C { #x; f() { return this.#x; } }
  }
}
"#,
        );
    }
}
