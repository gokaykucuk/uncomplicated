//! Property getters and setters should come in pairs
//!
//! ESLint `accessor-pairs` (setWithoutGet, enforceForClassMembers), with setters decorated by
//! Angular's `@Input()` exempted.
use crate::prelude::*;

pub struct Rule {
    get_without_set: bool,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            get_without_set: p.bool("getWithoutSet", false),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Acc {
    Get,
    Set,
}

struct Accessor<'s> {
    key: Key<'s>,
    kind: Acc,
    is_static: bool,
    /// Report location (from the member start to the opening parenthesis).
    loc: Span,
    /// ESLint's `getFunctionNameWithKind`, e.g. `setter 'foo'`.
    name: String,
}

enum Key<'s> {
    Static(String),
    Private(&'s str),
    Computed(Span),
}

fn same_key(ctx: &Ctx<'_, '_>, a: &Key<'_>, b: &Key<'_>) -> bool {
    match (a, b) {
        (Key::Static(x), Key::Static(y)) => x == y,
        (Key::Private(x), Key::Private(y)) => x == y,
        (Key::Computed(x), Key::Computed(y)) => ctx.same_tokens(*x, *y),
        _ => false,
    }
}

fn key_of<'s>(key: &'s PropertyKey<'_>, computed: bool) -> Key<'s> {
    match key {
        PropertyKey::PrivateIdentifier(p) => Key::Private(p.name.as_str()),
        PropertyKey::StaticIdentifier(id) if !computed => Key::Static(id.name.to_string()),
        k => match k.static_name() {
            Some(n) => Key::Static(n.into_owned()),
            None => Key::Computed(k.span()),
        },
    }
}

fn describe(kind: Acc, is_static: bool, key: &Key<'_>, f: &Function<'_>) -> String {
    let mut parts: Vec<String> = Vec::new();
    if is_static {
        parts.push("static".into());
    }
    if matches!(key, Key::Private(_)) {
        parts.push("private".into());
    }
    if f.r#async {
        parts.push("async".into());
    }
    if f.generator {
        parts.push("generator".into());
    }
    parts.push(if kind == Acc::Get { "getter" } else { "setter" }.into());
    match key {
        Key::Private(n) => parts.push(format!("#{n}")),
        Key::Static(n) => parts.push(format!("'{n}'")),
        Key::Computed(_) => {}
    }
    parts.join(" ")
}

fn head_loc(member_start: u32, f: &Function<'_>) -> Span {
    Span::new(member_start, f.params.span.start.max(member_start))
}

fn check(ctx: &Ctx<'_, '_>, accessors: &[Accessor<'_>], get_without_set: bool, in_class: bool) {
    let mut done = vec![false; accessors.len()];
    for i in 0..accessors.len() {
        if done[i] {
            continue;
        }
        let group: Vec<usize> = (i..accessors.len())
            .filter(|&j| {
                !done[j]
                    && accessors[j].is_static == accessors[i].is_static
                    && same_key(ctx, &accessors[j].key, &accessors[i].key)
            })
            .collect();
        for &j in &group {
            done[j] = true;
        }
        let has_get = group.iter().any(|&j| accessors[j].kind == Acc::Get);
        let has_set = group.iter().any(|&j| accessors[j].kind == Acc::Set);
        let suffix = if in_class { "class " } else { "" };
        if has_set && !has_get {
            for &j in &group {
                let a = &accessors[j];
                ctx.report(
                    a.loc,
                    format!("Getter is not present for {suffix}{}.", a.name),
                );
            }
        }
        if get_without_set && has_get && !has_set {
            for &j in &group {
                let a = &accessors[j];
                ctx.report(
                    a.loc,
                    format!("Setter is not present for {suffix}{}.", a.name),
                );
            }
        }
    }
}

fn is_angular_input(decorators: &[Decorator<'_>]) -> bool {
    decorators.iter().any(|d| {
        matches!(&d.expression, Expression::CallExpression(c) if matches!(&c.callee, Expression::Identifier(id) if id.name == "Input"))
    })
}

/// `Object.defineProperty(o, k, {set(v) {}})`, `Reflect.defineProperty`, `Object.create(p, {k: {...}})`,
/// `Object.defineProperties(o, {k: {...}})`.
fn is_property_descriptor(ctx: &Ctx<'_, '_>, node: &AstNode<'_>, span: Span) -> bool {
    let parent = ctx.parent(node.id());
    let is_arg =
        |c: &CallExpression<'_>, idx: usize| c.arguments.get(idx).is_some_and(|a| a.span() == span);
    match parent.kind() {
        AstKind::CallExpression(c) => {
            matches!(
                callee_path(c).as_deref(),
                Some("Object.defineProperty" | "Reflect.defineProperty")
            ) && is_arg(c, 2)
        }
        AstKind::ObjectProperty(p) if !p.computed && p.value.span() == span => {
            let obj = ctx.parent(parent.id());
            let AstKind::ObjectExpression(o) = obj.kind() else {
                return false;
            };
            let o_span = o.span;
            match ctx.parent(obj.id()).kind() {
                AstKind::CallExpression(c) => {
                    matches!(
                        callee_path(c).as_deref(),
                        Some("Object.create" | "Object.defineProperties")
                    ) && is_arg(c, 1)
                        && c.arguments[1].span() == o_span
                }
                _ => false,
            }
        }
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ObjectExpression, AstType::ClassBody]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::ObjectExpression(o) => {
                if is_property_descriptor(ctx, node, o.span) {
                    let names: Vec<&str> = o
                        .properties
                        .iter()
                        .filter_map(|p| p.as_property())
                        .filter(|p| p.kind == PropertyKind::Init && !p.computed)
                        .filter_map(|p| match &p.key {
                            PropertyKey::StaticIdentifier(id) => Some(id.name.as_str()),
                            _ => None,
                        })
                        .collect();
                    let (has_get, has_set) = (names.contains(&"get"), names.contains(&"set"));
                    if has_set && !has_get {
                        ctx.report(o.span, "Getter is not present in property descriptor.");
                    } else if self.get_without_set && has_get && !has_set {
                        ctx.report(o.span, "Setter is not present in property descriptor.");
                    }
                }
                let accessors: Vec<Accessor<'_>> = o
                    .properties
                    .iter()
                    .filter_map(|p| p.as_property())
                    .filter_map(|p| {
                        let kind = match p.kind {
                            PropertyKind::Get => Acc::Get,
                            PropertyKind::Set => Acc::Set,
                            PropertyKind::Init => return None,
                        };
                        let Expression::FunctionExpression(f) = &p.value else {
                            return None;
                        };
                        let key = key_of(&p.key, p.computed);
                        let name = describe(kind, false, &key, f);
                        Some(Accessor {
                            key,
                            kind,
                            is_static: false,
                            loc: head_loc(p.span.start, f),
                            name,
                        })
                    })
                    .collect();
                check(ctx, &accessors, self.get_without_set, false);
            }
            AstKind::ClassBody(b) => {
                let accessors: Vec<Accessor<'_>> = b
                    .body
                    .iter()
                    .filter_map(|e| match e {
                        ClassElement::MethodDefinition(m) => Some(m),
                        _ => None,
                    })
                    .filter_map(|m| {
                        let kind = match m.kind {
                            MethodDefinitionKind::Get => Acc::Get,
                            MethodDefinitionKind::Set => Acc::Set,
                            _ => return None,
                        };
                        let key = key_of(&m.key, m.computed);
                        let name = describe(kind, m.r#static, &key, &m.value);
                        Some(Accessor {
                            key,
                            kind,
                            is_static: m.r#static,
                            loc: head_loc(m.span.start, &m.value),
                            name,
                        })
                    })
                    .collect();
                let exempt: Vec<Span> = b
                    .body
                    .iter()
                    .filter_map(|e| match e {
                        ClassElement::MethodDefinition(m)
                            if m.kind == MethodDefinitionKind::Set
                                && is_angular_input(&m.decorators) =>
                        {
                            Some(m.span.start)
                        }
                        _ => None,
                    })
                    .map(|s| Span::new(s, s))
                    .collect();
                if exempt.is_empty() {
                    check(ctx, &accessors, self.get_without_set, true);
                } else {
                    let kept: Vec<Accessor<'_>> = accessors
                        .into_iter()
                        .filter(|a| !exempt.iter().any(|e| e.start == a.loc.start))
                        .collect();
                    check(ctx, &kept, self.get_without_set, true);
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
    fn accessor_pairs() {
        check(
            "S2376",
            "a.ts",
            r#"
var o = {
  set foo(v) {}, // Noncompliant {{Getter is not present for setter 'foo'.}}
  get bar() { return 1; },
  set baz(v) {},
  get baz() { return 1; },
};
class C {
  set a(v) {} // Noncompliant {{Getter is not present for class setter 'a'.}}
  static set b(v) {} // Noncompliant {{Getter is not present for class static setter 'b'.}}
  get b() { return 1; }
  set #p(v) {} // Noncompliant {{Getter is not present for class private setter #p.}}
  @Input()
  set name(v: string) {}
  set ok(v) {}
  get ok() { return 1; }
}
Object.defineProperty(o, "x", { set: function (v) {} }); // Noncompliant {{Getter is not present in property descriptor.}}
Object.defineProperty(o, "y", { set(v) {}, get() { return 1; } });
"#,
        );
    }
}
