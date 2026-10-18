//! Getters and setters should access the expected fields
use oxc_ast_visit::Visit;
use oxc_syntax::scope::ScopeFlags;

use crate::helpers::batch_bug2::stmts_complete;
use crate::prelude::*;

pub struct Rule {
    allow_implicit: bool,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            allow_implicit: p.bool("allowImplicit", false),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AccessorType {
    Getter,
    Setter,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Definition {
    Class,
    Object,
    Descriptor,
}

struct Info {
    name: String,
    ty: AccessorType,
    def: Definition,
}

struct Field {
    name: String,
    span: Span,
}

fn key_name(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::NumericLiteral(n) => Some(if n.value.fract() == 0.0 && n.value.abs() < 1e21 {
            format!("{}", n.value as i64)
        } else {
            format!("{}", n.value)
        }),
        k => property_key_name(k).map(str::to_string),
    }
}

fn is_global_member_call(
    ctx: &Ctx<'_, '_>,
    call: &CallExpression<'_>,
    obj: &str,
    methods: &[&str],
    min_args: usize,
) -> bool {
    let Expression::StaticMemberExpression(m) = &call.callee else {
        return false;
    };
    matches!(&m.object, Expression::Identifier(id) if ctx.is_global(id, obj))
        && methods.contains(&m.property.name.as_str())
        && call.arguments.len() >= min_args
}

fn arg_is(call: &CallExpression<'_>, idx: usize, span: Span) -> bool {
    call.arguments.get(idx).is_some_and(|a| a.span() == span)
}

/// For an object expression used as a property descriptor: the described property name.
/// `single` is true for `Object.defineProperty`/`Reflect.defineProperty`.
/// With `any_name`, also accepts descriptors whose property name is not static (returned empty).
fn descriptor_name(
    ctx: &Ctx<'_, '_>,
    obj_id: NodeId,
    obj_span: Span,
    any_name: bool,
) -> Option<String> {
    let parent = ctx.parent(obj_id);
    match parent.kind() {
        AstKind::CallExpression(call) => {
            let single = is_global_member_call(ctx, call, "Object", &["defineProperty"], 3)
                || is_global_member_call(ctx, call, "Reflect", &["defineProperty"], 3);
            if single && arg_is(call, 2, obj_span) {
                return match call.arguments[1].as_expression().map(strip_parens) {
                    Some(Expression::StringLiteral(s)) => Some(s.value.to_string()),
                    _ => any_name.then(String::new),
                };
            }
            None
        }
        AstKind::ObjectProperty(p) if p.value.span() == obj_span => {
            let outer = ctx.parent(parent.id());
            let AstKind::ObjectExpression(o) = outer.kind() else {
                return None;
            };
            let AstKind::CallExpression(call) = ctx.parent(outer.id()).kind() else {
                return None;
            };
            if is_global_member_call(ctx, call, "Object", &["defineProperties", "create"], 2)
                && arg_is(call, 1, o.span)
                && matches!(
                    p.key,
                    PropertyKey::StaticIdentifier(_)
                        | PropertyKey::StringLiteral(_)
                        | PropertyKey::NumericLiteral(_)
                )
            {
                return key_name(&p.key);
            }
            if any_name
                && is_global_member_call(ctx, call, "Object", &["defineProperties", "create"], 2)
                && arg_is(call, 1, o.span)
            {
                return Some(String::new());
            }
            None
        }
        _ => None,
    }
}

/// Getter-return target: is this function the `get` of an accessor or descriptor?
fn is_getter_function(ctx: &Ctx<'_, '_>, func_id: NodeId) -> Option<&'static str> {
    let parent = ctx.parent(func_id);
    match parent.kind() {
        AstKind::MethodDefinition(m) if m.kind == MethodDefinitionKind::Get => Some("getter"),
        AstKind::ObjectProperty(p) if p.kind == PropertyKind::Get => Some("getter"),
        AstKind::ObjectProperty(p)
            if p.kind == PropertyKind::Init && key_name(&p.key).as_deref() == Some("get") =>
        {
            let obj = ctx.parent(parent.id());
            let AstKind::ObjectExpression(o) = obj.kind() else {
                return None;
            };
            descriptor_name(ctx, obj.id(), o.span, true).map(|_| "descriptor")
        }
        _ => None,
    }
}

struct BareReturns(Vec<Span>, bool);

impl<'a> Visit<'a> for BareReturns {
    fn visit_return_statement(&mut self, it: &ReturnStatement<'a>) {
        self.1 = true;
        if it.argument.is_none() {
            self.0.push(it.span);
        }
    }
    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}
    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
    fn visit_class(&mut self, _: &Class<'a>) {}
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::Function,
            AstType::ArrowFunctionExpression,
            AstType::ObjectProperty,
            AstType::MethodDefinition,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::Function(f) if f.r#type == FunctionType::FunctionExpression => {
                if let Some(body) = &f.body {
                    self.getter_return(node, body, ctx);
                }
            }
            AstKind::ArrowFunctionExpression(a) => {
                if let Some(body) = a.body.as_function_body() {
                    self.getter_return(node, body, ctx);
                }
            }
            AstKind::ObjectProperty(_) | AstKind::MethodDefinition(_) => {
                let Some(info) = accessor_info(ctx, node) else {
                    return;
                };
                let Some((stmt, fields)) = get_accessor(ctx, node, &info) else {
                    return;
                };
                let accessor_kind_get = match node.kind() {
                    AstKind::ObjectProperty(p) => p.kind == PropertyKind::Get,
                    AstKind::MethodDefinition(m) => m.kind == MethodDefinitionKind::Get,
                    _ => false,
                };
                // Empty getters are reported through the getter-return part.
                if info.ty == AccessorType::Getter
                    && stmt.is_none()
                    && (info.def == Definition::Descriptor || accessor_kind_get)
                {
                    return;
                }
                if !uses_field(stmt, &info, &fields) {
                    report_mismatch(ctx, node, &info, &fields);
                }
            }
            _ => {}
        }
    }
}

impl Rule {
    fn getter_return<'a>(&self, node: &AstNode<'a>, body: &FunctionBody<'a>, ctx: &Ctx<'a, '_>) {
        if is_getter_function(ctx, node.id()).is_none() {
            return;
        }
        let mut scan = BareReturns(Vec::new(), false);
        for s in &body.statements {
            scan.visit_statement(s);
        }
        if !self.allow_implicit {
            for s in &scan.0 {
                ctx.report(*s, "Refactor this getter to return a value.");
            }
        }
        if !stmts_complete(&body.statements) {
            return;
        }
        let accessor = ctx.parent(node.id());
        // When the accessor has a matching field, the field mismatch message is used instead.
        if let Some(info) = accessor_info(ctx, accessor)
            && let Some((_, fields)) = get_accessor(ctx, accessor, &info)
        {
            report_mismatch(ctx, accessor, &info, &fields);
            return;
        }
        let start = accessor.span().start;
        let paren = find_token(
            ctx,
            Span::new(start, body.span.start.max(start)),
            Kind::LParen,
        );
        let span = Span::new(
            start,
            paren.map_or(body.span.start, |p| p.start).max(start + 1),
        );
        let msg = if scan.1 {
            "Refactor this getter to always return a value."
        } else {
            "Refactor this getter to return a value."
        };
        ctx.report(span, msg);
    }
}

fn accessor_info(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> Option<Info> {
    let (key, kind, value_fn, is_property) = match node.kind() {
        AstKind::ObjectProperty(p) => {
            let f = match &p.value {
                Expression::FunctionExpression(f) => Some(&**f),
                _ => None,
            };
            let kind = match p.kind {
                PropertyKind::Get => Some(AccessorType::Getter),
                PropertyKind::Set => Some(AccessorType::Setter),
                PropertyKind::Init => None,
            };
            (&p.key, kind, f, true)
        }
        AstKind::MethodDefinition(m) => {
            let kind = match m.kind {
                MethodDefinitionKind::Get => Some(AccessorType::Getter),
                MethodDefinitionKind::Set => Some(AccessorType::Setter),
                _ => None,
            };
            (&m.key, kind, Some(&*m.value), false)
        }
        _ => return None,
    };
    if is_property {
        // Property descriptor `get` / `set` functions.
        if value_fn.is_some() && kind.is_none() {
            let key = key_name(key)?;
            if key == "get" || key == "set" {
                let obj = ctx.parent(node.id());
                if let AstKind::ObjectExpression(o) = obj.kind()
                    && let Some(name) = descriptor_name(ctx, obj.id(), o.span, false)
                {
                    let ty = if key == "get" {
                        AccessorType::Getter
                    } else {
                        AccessorType::Setter
                    };
                    return Some(Info {
                        name,
                        ty,
                        def: Definition::Descriptor,
                    });
                }
            }
        }
    }
    let mut name = key_name(key)?.to_lowercase();
    let ty = match kind {
        Some(t) => t,
        None => {
            let f = value_fn?;
            let params = f.params.items.len() + usize::from(f.params.rest.is_some());
            if name.len() > 3 && name.starts_with("set") && params == 1 {
                name = name[3..].to_string();
                AccessorType::Setter
            } else if name.len() > 3 && name.starts_with("get") && params == 0 {
                name = name[3..].to_string();
                AccessorType::Getter
            } else {
                return None;
            }
        }
    };
    Some(Info {
        name,
        ty,
        def: if is_property {
            Definition::Object
        } else {
            Definition::Class
        },
    })
}

/// Returns the single statement (if any) and the matching fields.
fn get_accessor<'a>(
    ctx: &Ctx<'a, '_>,
    node: &AstNode<'a>,
    info: &Info,
) -> Option<(Option<&'a Statement<'a>>, Vec<Field>)> {
    let func: &'a Function<'a> = match node.kind() {
        AstKind::ObjectProperty(p) => match &p.value {
            Expression::FunctionExpression(f) => f,
            _ => return None,
        },
        AstKind::MethodDefinition(m) => {
            if m.accessibility
                .is_some_and(|a| a != TSAccessibility::Public)
            {
                return None;
            }
            &m.value
        }
        _ => return None,
    };
    let body = func.body.as_ref()?;
    if body.statements.len() > 1 {
        return None;
    }
    let fields = matching_fields(ctx, node, info);
    if fields.is_empty() {
        return None;
    }
    Some((body.statements.first(), fields))
}

fn matching_fields(ctx: &Ctx<'_, '_>, node: &AstNode<'_>, info: &Info) -> Vec<Field> {
    let name = &info.name;
    let candidates = [name.clone(), format!("_{name}"), format!("{name}_")];
    let parent = ctx.parent(node.id());
    if info.def == Definition::Descriptor {
        let scope = node.scope_id();
        for c in &candidates {
            if let Some(sym) = ctx.scoping().find_binding(scope, c.as_str().into()) {
                return vec![Field {
                    name: c.clone(),
                    span: ctx.symbol_decl(sym).span(),
                }];
            }
        }
        return Vec::new();
    }
    // name (lower-cased for object / class fields) -> field
    let mut map: FxHashMap<String, Field> = FxHashMap::default();
    match parent.kind() {
        AstKind::ObjectExpression(o) => {
            for p in &o.properties {
                if let ObjectPropertyKind::ObjectProperty(p) = p
                    && !p.method
                    && p.kind == PropertyKind::Init
                    && let Some(n) = key_name(&p.key)
                {
                    map.insert(
                        n.to_lowercase(),
                        Field {
                            name: n,
                            span: p.span,
                        },
                    );
                }
            }
        }
        AstKind::ClassBody(b) => {
            for e in &b.body {
                match e {
                    ClassElement::PropertyDefinition(p) if !p.r#static => {
                        if let Some(n) = key_name(&p.key) {
                            map.insert(
                                n.to_lowercase(),
                                Field {
                                    name: n,
                                    span: p.span,
                                },
                            );
                        }
                    }
                    ClassElement::MethodDefinition(m)
                        if m.kind == MethodDefinitionKind::Constructor =>
                    {
                        for param in &m.value.params.items {
                            if (param.accessibility.is_some() || param.readonly)
                                && let BindingPattern::BindingIdentifier(id) = &param.pattern
                            {
                                // Constructor parameter properties keep their case.
                                map.insert(
                                    id.name.to_string(),
                                    Field {
                                        name: id.name.to_string(),
                                        span: param.span,
                                    },
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => return Vec::new(),
    }
    let mut out = Vec::new();
    for c in candidates {
        if let Some(f) = map.remove(&c) {
            out.push(f);
        }
    }
    out
}

fn uses_field(stmt: Option<&Statement<'_>>, info: &Info, fields: &[Field]) -> bool {
    let Some(stmt) = stmt else { return false };
    let target: Option<&Expression<'_>> = match (info.ty, stmt) {
        (AccessorType::Setter, Statement::ExpressionStatement(es)) => match &es.expression {
            Expression::AssignmentExpression(a) => {
                let used = match &a.left {
                    AssignmentTarget::StaticMemberExpression(m)
                        if info.def != Definition::Descriptor =>
                    {
                        matches!(m.object, Expression::ThisExpression(_))
                            .then(|| m.property.name.to_string())
                    }
                    AssignmentTarget::PrivateFieldExpression(m)
                        if info.def != Definition::Descriptor =>
                    {
                        matches!(m.object, Expression::ThisExpression(_))
                            .then(|| m.field.name.to_string())
                    }
                    AssignmentTarget::ComputedMemberExpression(m)
                        if info.def != Definition::Descriptor =>
                    {
                        if matches!(m.object, Expression::ThisExpression(_)) {
                            match &m.expression {
                                Expression::StringLiteral(s) => Some(s.value.to_string()),
                                _ => None,
                            }
                        } else {
                            None
                        }
                    }
                    AssignmentTarget::AssignmentTargetIdentifier(id)
                        if info.def == Definition::Descriptor =>
                    {
                        Some(id.name.to_string())
                    }
                    _ => None,
                };
                return used.is_none_or(|u| fields.iter().any(|f| f.name == u));
            }
            _ => None,
        },
        (AccessorType::Getter, Statement::ReturnStatement(r)) => r.argument.as_ref(),
        _ => None,
    };
    let Some(e) = target else { return true };
    let used = if info.def == Definition::Descriptor {
        match e {
            Expression::Identifier(id) => Some(id.name.to_string()),
            _ => None,
        }
    } else {
        match e {
            Expression::StaticMemberExpression(m)
                if matches!(m.object, Expression::ThisExpression(_)) =>
            {
                Some(m.property.name.to_string())
            }
            Expression::PrivateFieldExpression(m)
                if matches!(m.object, Expression::ThisExpression(_)) =>
            {
                Some(m.field.name.to_string())
            }
            Expression::ComputedMemberExpression(m)
                if matches!(m.object, Expression::ThisExpression(_)) =>
            {
                match &m.expression {
                    Expression::StringLiteral(s) => Some(s.value.to_string()),
                    _ => None,
                }
            }
            _ => None,
        }
    };
    used.is_none_or(|u| fields.iter().any(|f| f.name == u))
}

fn report_mismatch(ctx: &Ctx<'_, '_>, node: &AstNode<'_>, info: &Info, fields: &[Field]) {
    let key_span = match node.kind() {
        AstKind::ObjectProperty(p) => p.key.span(),
        AstKind::MethodDefinition(m) => m.key.span(),
        _ => return,
    };
    let field = &fields[0];
    let (r, rc) = if info.def == Definition::Descriptor {
        ("variable", "Variable")
    } else {
        ("property", "Property")
    };
    let ty = if info.ty == AccessorType::Getter {
        "getter"
    } else {
        "setter"
    };
    ctx.report_with(
        key_span,
        format!(
            "Refactor this {ty} so that it actually refers to the {r} '{}'.",
            field.name
        ),
        [(field.span, Some(format!("{rc} which should be referred.")))],
    );
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn getter_return() {
        check(
            "S4275",
            "a.js",
            r#"
const o1 = {
  get a() { // Noncompliant {{Refactor this getter to return a value.}}
    foo();
    bar();
  },
  get b() { // Noncompliant {{Refactor this getter to always return a value.}}
    if (x) { return 1; }
    foo();
  },
  get c() { return 1; },
  get d() { if (x) { return 1; } else { throw new Error(); } },
  get e() {
    return; // Noncompliant {{Refactor this getter to return a value.}}
  },
};
Object.defineProperty(obj, "foo", {
  get: function () { // Noncompliant {{Refactor this getter to return a value.}}
    foo();
    bar();
  },
});
Object.defineProperty(obj, name, {
  get: () => {}, // Noncompliant
});
class A {
  get x() { while (true) { if (y) return 1; } }
  get z() { switch (k) { case 1: return 1; default: return 2; } }
}
"#,
        );
    }

    #[test]
    fn field_mismatch() {
        check(
            "S4275",
            "a.ts",
            r#"
class A {
  private x: number = 0;
  private y: number = 0;
  public getX() { return this.y; } // Noncompliant {{Refactor this getter so that it actually refers to the property 'x'.}}
  public setX(v: number) { this.y = v; } // Noncompliant {{Refactor this setter so that it actually refers to the property 'x'.}}
  get y2() { return this.x; }
  public getY() { return this.y; }
}
class B {
  _name = "";
  get name() { return this._name; }
  set name(v) { this._name = v; }
  get other() { return this.foo; }
}
class C {
  _a = 1;
  get a() { return this._b; } // Noncompliant
  get b() {} // Noncompliant
}
const obj = {
  foo: 1,
  bar: 2,
  getFoo() { return this.bar; }, // Noncompliant {{Refactor this getter so that it actually refers to the property 'foo'.}}
  getBar() { return this.bar; },
};
class D {
  constructor(private value: number) {}
  getValue() { return this.other; } // Noncompliant
}
let name;
Object.defineProperty(o, "name", {
  get: function () { return other; }, // Noncompliant {{Refactor this getter so that it actually refers to the variable 'name'.}}
  set: function (v) { name = v; },
});
"#,
        );
    }
}
