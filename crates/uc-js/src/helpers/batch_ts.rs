//! Helpers owned by the `ts` rule batch.
use oxc_ast::{AstKind, ast::*};
use oxc_span::{GetSpan, Span};

use crate::Ctx;
use crate::helpers::ast::{property_key_name, string_value, strip_parens};

/// Strips parenthesized types: `((A))` → `A`.
pub fn strip_type_parens<'b, 'a>(mut t: &'b TSType<'a>) -> &'b TSType<'a> {
    while let TSType::TSParenthesizedType(p) = t {
        t = &p.type_annotation;
    }
    t
}

/// Span of a type without its surrounding parentheses (ESTree drops them).
pub fn type_span(t: &TSType<'_>) -> Span {
    strip_type_parens(t).span()
}

/// Parent node skipping parenthesized types.
pub fn type_parent<'s, 'a>(
    ctx: &Ctx<'a, 's>,
    id: oxc_semantic::NodeId,
) -> &'s oxc_semantic::AstNode<'a> {
    let mut cur = ctx.parent(id);
    while let AstKind::TSParenthesizedType(_) = cur.kind() {
        cur = ctx.parent(cur.id());
    }
    cur
}

/// Name of a decorator `@Foo(...)` or `@Foo`.
pub fn decorator_name<'a>(d: &Decorator<'a>) -> Option<&'a str> {
    match strip_parens(&d.expression) {
        Expression::CallExpression(c) => match strip_parens(&c.callee) {
            Expression::Identifier(id) => Some(id.name.as_str()),
            _ => None,
        },
        Expression::Identifier(id) => Some(id.name.as_str()),
        _ => None,
    }
}

/// Call expression of a decorator `@Foo(...)`, if it is called.
pub fn decorator_call<'b, 'a>(d: &'b Decorator<'a>) -> Option<&'b CallExpression<'a>> {
    match strip_parens(&d.expression) {
        Expression::CallExpression(c) => Some(c),
        _ => None,
    }
}

/// Angular class decorators.
pub const ANGULAR_CLASS_DECORATORS: &[&str] = &[
    "Component",
    "Directive",
    "Injectable",
    "NgModule",
    "Pipe",
    "Service",
];

/// Angular lifecycle methods and their interfaces.
pub const ANGULAR_LIFECYCLE: &[(&str, &str)] = &[
    ("ngAfterContentChecked", "AfterContentChecked"),
    ("ngAfterContentInit", "AfterContentInit"),
    ("ngAfterViewChecked", "AfterViewChecked"),
    ("ngAfterViewInit", "AfterViewInit"),
    ("ngDoBootstrap", "DoBootstrap"),
    ("ngDoCheck", "DoCheck"),
    ("ngOnChanges", "OnChanges"),
    ("ngOnDestroy", "OnDestroy"),
    ("ngOnInit", "OnInit"),
];

pub fn lifecycle_interface(method: &str) -> Option<&'static str> {
    ANGULAR_LIFECYCLE
        .iter()
        .find(|(m, _)| *m == method)
        .map(|(_, i)| *i)
}

/// The first Angular class decorator of a class (`Component`, `Directive`, …), with its node.
pub fn angular_decorator<'b, 'a>(
    class: &'b Class<'a>,
    names: &[&str],
) -> Option<(&'a str, &'b Decorator<'a>)> {
    class.decorators.iter().find_map(|d| {
        let n = decorator_name(d)?;
        names.contains(&n).then_some((n, d))
    })
}

/// The object literal passed to an Angular decorator: `@Component({...})`.
pub fn decorator_metadata<'b, 'a>(d: &'b Decorator<'a>) -> Option<&'b ObjectExpression<'a>> {
    let call = decorator_call(d)?;
    match call.arguments.first()?.as_expression().map(strip_parens)? {
        Expression::ObjectExpression(o) => Some(o),
        _ => None,
    }
}

/// A non-computed, statically named property of an object literal.
pub fn object_property<'b, 'a>(
    o: &'b ObjectExpression<'a>,
    name: &str,
) -> Option<&'b ObjectProperty<'a>> {
    o.properties.iter().find_map(|p| match p {
        ObjectPropertyKind::ObjectProperty(p)
            if !p.computed && property_key_name(&p.key) == Some(name) =>
        {
            Some(&**p)
        }
        _ => None,
    })
}

/// Static key name of a class member key (identifier, string, number) — `None` when computed.
pub fn member_key_name<'a>(
    key: &PropertyKey<'a>,
    computed: bool,
) -> Option<std::borrow::Cow<'a, str>> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.as_str().into()),
        PropertyKey::PrivateIdentifier(id) => Some(format!("#{}", id.name).into()),
        PropertyKey::StringLiteral(s) => Some(s.value.as_str().into()),
        PropertyKey::NumericLiteral(n) => Some(fmt_number(n.value).into()),
        _ if computed => None,
        _ => None,
    }
}

/// Formats a number like JavaScript's `String(n)` for common values.
pub fn fmt_number(v: f64) -> String {
    if v.is_finite() && v.fract() == 0.0 && v.abs() < 1e21 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// A member expression written to: its object and static property name (`#x` for private).
pub struct MemberWrite<'b, 'a> {
    pub object: &'b Expression<'a>,
    pub name: std::borrow::Cow<'a, str>,
}

fn simple_target_member<'b, 'a>(
    t: &'b SimpleAssignmentTarget<'a>,
    out: &mut Vec<MemberWrite<'b, 'a>>,
) {
    match t {
        SimpleAssignmentTarget::StaticMemberExpression(m) => out.push(MemberWrite {
            object: &m.object,
            name: m.property.name.as_str().into(),
        }),
        SimpleAssignmentTarget::PrivateFieldExpression(m) => out.push(MemberWrite {
            object: &m.object,
            name: format!("#{}", m.field.name).into(),
        }),
        SimpleAssignmentTarget::ComputedMemberExpression(m) => {
            if let Expression::StringLiteral(s) = &m.expression {
                out.push(MemberWrite {
                    object: &m.object,
                    name: s.value.as_str().into(),
                })
            }
        }
        SimpleAssignmentTarget::TSAsExpression(e) => expression_member(&e.expression, out),
        SimpleAssignmentTarget::TSSatisfiesExpression(e) => expression_member(&e.expression, out),
        SimpleAssignmentTarget::TSNonNullExpression(e) => expression_member(&e.expression, out),
        SimpleAssignmentTarget::TSTypeAssertion(e) => expression_member(&e.expression, out),
        SimpleAssignmentTarget::AssignmentTargetIdentifier(_) => {}
    }
}

/// Member written through an expression (`delete this.x`, `(this.x as any) = …`).
pub fn expression_member<'b, 'a>(e: &'b Expression<'a>, out: &mut Vec<MemberWrite<'b, 'a>>) {
    match crate::helpers::ast::strip_ts(e) {
        Expression::StaticMemberExpression(m) => out.push(MemberWrite {
            object: &m.object,
            name: m.property.name.as_str().into(),
        }),
        Expression::PrivateFieldExpression(m) => out.push(MemberWrite {
            object: &m.object,
            name: format!("#{}", m.field.name).into(),
        }),
        Expression::ComputedMemberExpression(m) => {
            if let Expression::StringLiteral(s) = &m.expression {
                out.push(MemberWrite {
                    object: &m.object,
                    name: s.value.as_str().into(),
                })
            }
        }
        _ => {}
    }
}

fn maybe_default_members<'b, 'a>(
    t: &'b AssignmentTargetMaybeDefault<'a>,
    out: &mut Vec<MemberWrite<'b, 'a>>,
) {
    match t {
        AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(d) => {
            target_members(&d.binding, out)
        }
        t => {
            if let Some(t) = t.as_assignment_target() {
                target_members(t, out)
            }
        }
    }
}

/// Members written by an assignment target, including destructuring patterns.
pub fn target_members<'b, 'a>(t: &'b AssignmentTarget<'a>, out: &mut Vec<MemberWrite<'b, 'a>>) {
    match t {
        AssignmentTarget::ArrayAssignmentTarget(a) => {
            for e in a.elements.iter().flatten() {
                maybe_default_members(e, out);
            }
            if let Some(r) = &a.rest {
                target_members(&r.target, out);
            }
        }
        AssignmentTarget::ObjectAssignmentTarget(o) => {
            for p in &o.properties {
                if let AssignmentTargetProperty::AssignmentTargetPropertyProperty(p) = p {
                    maybe_default_members(&p.binding, out);
                }
            }
            if let Some(r) = &o.rest {
                target_members(&r.target, out);
            }
        }
        t => {
            if let Some(s) = t.as_simple_assignment_target() {
                simple_target_member(s, out);
            }
        }
    }
}

/// Members written by any write node (assignment, update, `delete`, `for…in/of` target).
pub fn written_members<'a>(kind: AstKind<'a>, out: &mut Vec<MemberWrite<'a, 'a>>) {
    match kind {
        AstKind::AssignmentExpression(a) => target_members(&a.left, out),
        AstKind::UpdateExpression(u) => simple_target_member(&u.argument, out),
        AstKind::UnaryExpression(u) if u.operator == UnaryOperator::Delete => {
            expression_member(&u.argument, out)
        }
        AstKind::ForInStatement(f) => {
            if let Some(t) = f.left.as_assignment_target() {
                target_members(t, out)
            }
        }
        AstKind::ForOfStatement(f) => {
            if let Some(t) = f.left.as_assignment_target() {
                target_members(t, out)
            }
        }
        _ => {}
    }
}

/// Name a callee identifier is imported as from `module` (`import { output as o } from '@angular/core'`
/// gives `output` for `o`).
pub fn imported_name<'a>(
    ctx: &Ctx<'a, '_>,
    id: &IdentifierReference<'a>,
    module: &str,
) -> Option<&'a str> {
    let sym = ctx.symbol_of(id)?;
    let decl = ctx.symbol_decl(sym);
    let AstKind::ImportSpecifier(spec) = decl.kind() else {
        return None;
    };
    let AstKind::ImportDeclaration(imp) = ctx.parent_kind(decl.id()) else {
        return None;
    };
    (imp.source.value == module).then(|| spec.imported.name().as_str())
}

/// An Angular binding declared on a class member: `@Input('alias') name`, `name = output({ alias })`, ….
pub struct Binding<'a> {
    /// Span and text of the member key.
    pub key: Span,
    pub name: &'a str,
    /// Span and value of the alias string literal, if any.
    pub alias: Option<(Span, &'a str)>,
    pub member_span: Span,
    pub member_start: u32,
}

fn alias_in_options<'a>(o: &ObjectExpression<'a>) -> Option<(Span, &'a str)> {
    let p = object_property(o, "alias")?;
    match strip_parens(&p.value) {
        Expression::StringLiteral(s) => Some((s.span, s.value.as_str())),
        _ => None,
    }
}

/// Bindings declared with the `decorator` (`Input` on properties and setters, `Output` on
/// properties and getters) or the matching signal function (`input()` / `input.required()`,
/// `output()`), as angular-eslint's selectors find them (by callee name).
pub fn angular_bindings<'a>(
    ctx: &Ctx<'a, '_>,
    class: &Class<'a>,
    decorator: &str,
    signal: &str,
) -> Vec<Binding<'a>> {
    let accessor = if decorator == "Output" {
        MethodDefinitionKind::Get
    } else {
        MethodDefinitionKind::Set
    };
    let mut out = Vec::new();
    for el in &class.body.body {
        let (key, computed, decorators, value, span) = match el {
            ClassElement::PropertyDefinition(p) => {
                (&p.key, p.computed, &p.decorators, p.value.as_ref(), p.span)
            }
            ClassElement::MethodDefinition(m) if m.kind == accessor => {
                (&m.key, m.computed, &m.decorators, None, m.span)
            }
            _ => continue,
        };
        if computed {
            continue;
        }
        let PropertyKey::StaticIdentifier(id) = key else {
            continue;
        };
        let mut found = false;
        let mut alias = None;
        for d in decorators.iter() {
            if decorator_name(d) != Some(decorator) {
                continue;
            }
            found = true;
            if let Some(call) = decorator_call(d)
                && let Some(arg) = call.arguments.first().and_then(|a| a.as_expression())
            {
                alias = match strip_parens(arg) {
                    Expression::StringLiteral(s) => Some((s.span, s.value.as_str())),
                    Expression::ObjectExpression(o) => alias_in_options(o),
                    _ => None,
                };
            }
        }
        if !found && let Some(Expression::CallExpression(call)) = value {
            let is_signal = match &call.callee {
                Expression::Identifier(c) => c.name == signal,
                Expression::StaticMemberExpression(m) => {
                    signal == "input"
                        && m.property.name == "required"
                        && matches!(&m.object, Expression::Identifier(c) if c.name == "input")
                }
                _ => false,
            };
            if is_signal {
                found = true;
                alias = call
                    .arguments
                    .iter()
                    .filter_map(|a| a.as_expression())
                    .find_map(|a| match a {
                        Expression::ObjectExpression(o) => alias_in_options(o),
                        _ => None,
                    });
            }
        }
        if found {
            let member_start = decorators.last().map_or(span.start, |d| {
                crate::helpers::ast::token_after(ctx, d.span.end).map_or(span.start, |t| t.start)
            });
            out.push(Binding {
                key: id.span,
                name: id.name.as_str(),
                alias,
                member_span: span,
                member_start,
            });
        }
    }
    out
}

/// The `selector` of a `@Component` / `@Directive` decorator, as written.
fn selector_text<'a>(class: &Class<'a>) -> Option<&'a str> {
    let (_, d) = angular_decorator(class, &["Component", "Directive"])?;
    let p = object_property(decorator_metadata(d)?, "selector")?;
    string_value(&p.value)
}

/// angular-eslint's selector names: the selector without brackets and whitespace, split on `,`
/// (`[tooltip], a[b]` → `tooltip`, `ab`), plus the first bracketed attribute name.
pub fn selector_names(class: &Class<'_>) -> (Vec<String>, Option<String>) {
    let Some(text) = selector_text(class) else {
        return (Vec::new(), None);
    };
    let cleaned: String = text
        .chars()
        .filter(|c| !matches!(c, '[' | ']') && !c.is_whitespace())
        .collect();
    let directive = text.find('[').and_then(|i| {
        let rest = &text[i + 1..];
        rest.find(']').map(|j| rest[..j].to_string())
    });
    (cleaned.split(',').map(str::to_string).collect(), directive)
}

/// angular-eslint's `isAliasNameAllowed`: the alias is a selector name, the directive attribute
/// (inputs only) or a selector name followed by the capitalized member name.
pub fn alias_allowed_by_selector(
    class: &Class<'_>,
    name: &str,
    alias: &str,
    with_directive: bool,
) -> bool {
    let (selectors, directive) = selector_names(class);
    let cap = capitalize(name);
    selectors.iter().any(|s| {
        s == alias
            || (with_directive && directive.as_deref() == Some(alias))
            || format!("{s}{cap}") == alias
    })
}

/// String entries of the `inputs` / `outputs` metadata arrays of `@Component` / `@Directive`.
pub fn metadata_entries<'a>(class: &Class<'a>, property: &str) -> Vec<(Span, &'a str)> {
    let mut out = Vec::new();
    let Some((_, d)) = angular_decorator(class, &["Component", "Directive"]) else {
        return out;
    };
    let Some(meta) = decorator_metadata(d) else {
        return out;
    };
    let Some(p) = object_property(meta, property) else {
        return out;
    };
    let Expression::ArrayExpression(a) = strip_parens(&p.value) else {
        return out;
    };
    for e in &a.elements {
        if let Some(e) = e.as_expression()
            && let Some(v) = string_value(e)
        {
            out.push((e.span(), v));
        }
    }
    out
}

/// Reports the `property` metadata entry of `@Component` / `@Directive` decorators.
pub fn check_metadata_property(ctx: &Ctx<'_, '_>, class: &Class<'_>, property: &str, msg: &str) {
    let Some((_, decorator)) = angular_decorator(class, &["Component", "Directive"]) else {
        return;
    };
    let Some(meta) = decorator_metadata(decorator) else {
        return;
    };
    if let Some(p) = object_property(meta, property) {
        ctx.report(p.span, msg);
    }
}

/// angular-eslint's `getMethodName`: a string-literal key, or a non-computed identifier.
pub fn angular_method_name<'a>(m: &MethodDefinition<'a>) -> Option<(&'a str, Span)> {
    match &m.key {
        PropertyKey::StringLiteral(s) => Some((s.value.as_str(), s.span)),
        PropertyKey::StaticIdentifier(id) if !m.computed => Some((id.name.as_str(), id.span)),
        _ => None,
    }
}

/// `changed` → `Changed`.
pub fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Name of an enum member.
pub fn enum_member_name<'a>(ctx: &Ctx<'a, '_>, id: &TSEnumMemberName<'a>) -> &'a str {
    match id {
        TSEnumMemberName::Identifier(i) => i.name.as_str(),
        TSEnumMemberName::String(s) | TSEnumMemberName::ComputedString(s) => s.value.as_str(),
        TSEnumMemberName::ComputedTemplateString(t) => ctx.text(t.span),
    }
}

/// True if the class `implements` an interface called `name` (also `ns.name`).
pub fn implements_name<'a>(class: &Class<'a>, name: &str) -> bool {
    class.implements.iter().any(|i| match &i.expression {
        TSTypeName::IdentifierReference(id) => id.name == name,
        TSTypeName::QualifiedName(q) => q.right.name == name,
        TSTypeName::ThisExpression(_) => false,
    })
}

/// Sonar has no TypeScript type information in Vue single-file components, so its
/// type-dependent rules raise nothing there.
pub fn no_type_info(ctx: &Ctx<'_, '_>) -> bool {
    ctx.file.path.ends_with(".vue")
}
