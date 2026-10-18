//! Getters and setters should not recursively access the same property
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Accessor key: (is private, name).
fn accessor_key<'a>(key: &PropertyKey<'a>) -> Option<(bool, &'a str)> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some((false, id.name.as_str())),
        PropertyKey::PrivateIdentifier(id) => Some((true, id.name.as_str())),
        _ => None,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ThisExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        // Closest non-arrow function, not crossing a class boundary.
        let mut func = None;
        for a in crate::helpers::batch_unicorn::ancestors(ctx, node.id()) {
            match a.kind() {
                AstKind::Class(_) => return,
                AstKind::Function(_) => {
                    func = Some(a);
                    break;
                }
                _ => {}
            }
        }
        let Some(func) = func else { return };
        let (is_get, key) = match ctx.parent_kind(func.id()) {
            AstKind::MethodDefinition(m) if !m.computed => match m.kind {
                MethodDefinitionKind::Get => (true, &m.key),
                MethodDefinitionKind::Set => (false, &m.key),
                _ => return,
            },
            AstKind::ObjectProperty(p) if !p.computed => match p.kind {
                PropertyKind::Get => (true, &p.key),
                PropertyKind::Set => (false, &p.key),
                PropertyKind::Init => return,
            },
            _ => return,
        };
        let Some(key) = accessor_key(key) else { return };
        let parent = ctx.parent(node.id());
        let member_key = match parent.kind() {
            AstKind::StaticMemberExpression(m) => Some((false, m.property.name.as_str())),
            AstKind::PrivateFieldExpression(m) => Some((true, m.field.name.as_str())),
            _ => None,
        };
        let member_access = member_key == Some(key);
        let msg = if is_get {
            "Disallow recursive access to `this` within getters."
        } else {
            "Disallow recursive access to `this` within setters."
        };
        if is_get {
            if member_access {
                ctx.report(parent.kind().span(), msg);
                return;
            }
            if let AstKind::VariableDeclarator(v) = parent.kind()
                && v.init
                    .as_ref()
                    .is_some_and(|i| i.span() == node.kind().span())
                && let BindingPattern::ObjectPattern(p) = &v.id
                && !key.0
                && p.properties.iter().any(|bp| {
                    !bp.computed
                        && matches!(&bp.key, PropertyKey::StaticIdentifier(id) if id.name == key.1)
                })
            {
                ctx.report(v.span, msg);
            }
        } else if member_access {
            let is_write = match ctx.parent_kind(parent.id()) {
                AstKind::AssignmentExpression(a) => a.left.span() == parent.kind().span(),
                AstKind::UpdateExpression(_)
                | AstKind::ArrayAssignmentTarget(_)
                | AstKind::AssignmentTargetPropertyProperty(_) => true,
                AstKind::AssignmentTargetWithDefault(d) => d.binding.span() == parent.kind().span(),
                _ => false,
            };
            if is_write {
                ctx.report(parent.kind().span(), msg);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn accessor_recursion() {
        check(
            "S7725",
            "a.js",
            r#"
class A {
  get foo() { return this.foo; } // Noncompliant {{Disallow recursive access to `this` within getters.}}
  set foo(v) { this.foo = v; } // Noncompliant {{Disallow recursive access to `this` within setters.}}
  get #bar() { return this.#bar; } // Noncompliant
  get baz() { const { baz } = this; return baz; } // Noncompliant
  set qux(v) { this.qux++; } // Noncompliant
  set quux(v) { [this.quux] = v; } // Noncompliant
  get ok() { return this._ok; }
  set ok(v) { this._ok = v; }
  set reads(v) { console.log(this.reads); }
  get arrow() { return () => this.arrow; } // Noncompliant
  get nested() { return function () { return this.nested; }; }
  get other() { return this.#other; }
}
const o = {
  get x() { return this.x; }, // Noncompliant
  set x(v) { this.x = v; }, // Noncompliant
  get y() { return this.z; },
  get [computed]() { return this.computed; },
};
"#,
        );
    }
}
