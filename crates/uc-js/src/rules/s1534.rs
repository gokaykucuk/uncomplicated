//! Member names should not be duplicated within a class or object literal
//!
//! Union of ESLint `no-dupe-keys`, `no-dupe-class-members` (TypeScript flavour, overloads are
//! fine) and `react/jsx-no-duplicate-props`.
use std::borrow::Cow;

use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const DUP: fn(&str) -> String = |n| format!("Duplicate name '{n}'.");

#[derive(Default, Clone, Copy)]
struct Seen {
    init: bool,
    get: bool,
    set: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Init,
    Get,
    Set,
}

impl Seen {
    /// Records `kind`, returns true if it conflicts with what was seen before.
    fn add(&mut self, kind: Kind) -> bool {
        let dup = match kind {
            Kind::Init => self.init || self.get || self.set,
            Kind::Get => self.init || self.get,
            Kind::Set => self.init || self.set,
        };
        match kind {
            Kind::Init => self.init = true,
            Kind::Get => self.get = true,
            Kind::Set => self.set = true,
        }
        dup
    }
}

/// ESLint's `getStaticPropertyName` for keys.
fn static_key<'a>(key: &PropertyKey<'a>, computed: bool) -> Option<Cow<'a, str>> {
    match key {
        PropertyKey::StaticIdentifier(id) if !computed => Some(Cow::Borrowed(id.name.as_str())),
        PropertyKey::StaticIdentifier(_) => None,
        PropertyKey::StringLiteral(s) => Some(Cow::Borrowed(s.value.as_str())),
        PropertyKey::NumericLiteral(n) => Some(Cow::Owned(num_to_string(n.value))),
        PropertyKey::TemplateLiteral(t) if t.expressions.is_empty() => t
            .quasis
            .first()
            .and_then(|q| q.value.cooked.as_ref())
            .map(|c| Cow::Borrowed(c.as_str())),
        PropertyKey::NullLiteral(_) => Some(Cow::Borrowed("null")),
        PropertyKey::BooleanLiteral(b) => {
            Some(Cow::Borrowed(if b.value { "true" } else { "false" }))
        }
        PropertyKey::BigIntLiteral(b) => Some(Cow::Borrowed(b.value.as_str())),
        _ => None,
    }
}

fn num_to_string(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e21 {
        format!("{v:.0}")
    } else {
        v.to_string()
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::ObjectExpression,
            AstType::ClassBody,
            AstType::JSXOpeningElement,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::ObjectExpression(o) => {
                let mut seen: FxHashMap<Cow<str>, Seen> = FxHashMap::default();
                for p in &o.properties {
                    let ObjectPropertyKind::ObjectProperty(p) = p else {
                        continue;
                    };
                    let Some(name) = static_key(&p.key, p.computed) else {
                        continue;
                    };
                    // `__proto__: x` sets the prototype, it is not a property.
                    if name == "__proto__" && !p.computed && !p.method && !p.shorthand {
                        continue;
                    }
                    let kind = match p.kind {
                        PropertyKind::Init => Kind::Init,
                        PropertyKind::Get => Kind::Get,
                        PropertyKind::Set => Kind::Set,
                    };
                    let msg = DUP(&name);
                    if seen.entry(name).or_default().add(kind) {
                        ctx.report(p.key.span(), msg);
                    }
                }
            }
            AstKind::ClassBody(b) => {
                let mut seen: [FxHashMap<Cow<str>, Seen>; 2] = Default::default();
                for el in &b.body {
                    let (key, computed, is_static, kind, span) = match el {
                        ClassElement::MethodDefinition(m) => {
                            if m.value.body.is_none() || m.kind == MethodDefinitionKind::Constructor
                            {
                                continue;
                            }
                            let kind = match m.kind {
                                MethodDefinitionKind::Get => Kind::Get,
                                MethodDefinitionKind::Set => Kind::Set,
                                _ => Kind::Init,
                            };
                            (&m.key, m.computed, m.r#static, kind, m.span)
                        }
                        ClassElement::PropertyDefinition(p) => {
                            if p.declare {
                                continue;
                            }
                            (&p.key, p.computed, p.r#static, Kind::Init, p.span)
                        }
                        _ => continue,
                    };
                    let Some(name) = static_key(key, computed) else {
                        continue;
                    };
                    let msg = DUP(&name);
                    if seen[is_static as usize].entry(name).or_default().add(kind) {
                        ctx.report(span, msg);
                    }
                }
            }
            AstKind::JSXOpeningElement(el) => {
                let mut seen: FxHashSet<&str> = FxHashSet::default();
                for a in &el.attributes {
                    let JSXAttributeItem::Attribute(a) = a else {
                        continue;
                    };
                    let Some(name) = attr_name(a) else { continue };
                    if !seen.insert(name) {
                        ctx.report(a.span, "No duplicate props allowed");
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn objects_and_classes() {
        check(
            "S1534",
            "a.js",
            r#"
const o = {
  p: "foo",
  p: "bar", // Noncompliant {{Duplicate name 'p'.}}
  "q": 1,
  q: 2, // Noncompliant
  1: 1,
  "1": 2, // Noncompliant
  get g() { return 1; },
  set g(v) {},
  get h() { return 1; },
  h: 2, // Noncompliant
  [x]: 1,
  [x]: 2,
  ...spread,
  __proto__: a,
  "__proto__": b,
};
class C {
  m() {}
  m() {} // Noncompliant {{Duplicate name 'm'.}}
  static m() {}
  get a() { return 1; }
  set a(v) {}
  f = 1;
  f = 2; // Noncompliant
}
"#,
        );
    }

    #[test]
    fn overloads_and_jsx() {
        check(
            "S1534",
            "a.tsx",
            r#"
class D {
  m(n: number): void;
  m(s: string): void;
  m(x: any) {}
}
const e = <div a="1" b a={2} />; // Noncompliant {{No duplicate props allowed}}
const f = <div a="1" {...p} b />;
"#,
        );
    }
}
