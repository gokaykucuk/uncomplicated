//! Helpers owned by the `a11y` rule batch.
//!
//! - JSX attribute access and value extraction following `jsx-ast-utils` semantics (`getProp`,
//!   `getPropValue`, `getLiteralPropValue`, `hasProp`).
//! - ARIA tables (roles, states/properties, DOM elements, element/role and element/AX-object
//!   associations) generated from aria-query 5.3.2 and axobject-query 4.1.0, the versions used by
//!   eslint-plugin-jsx-a11y, plus the plugin's interactivity predicates.
use std::borrow::Cow;

use crate::prelude::*;

// ---- JSX names ---------------------------------------------------------------------------------

/// Element type of a JSX opening element (`div`, `Foo`); empty for member / namespaced names.
pub fn element_name<'a>(el: &JSXOpeningElement<'a>) -> &'a str {
    match &el.name {
        JSXElementName::Identifier(id) => id.name.as_str(),
        JSXElementName::IdentifierReference(id) => id.name.as_str(),
        _ => "",
    }
}

/// Name of a JSX attribute; empty for namespaced names (`xlink:href`).
pub fn attr_name<'a>(a: &JSXAttribute<'a>) -> &'a str {
    match &a.name {
        JSXAttributeName::Identifier(id) => id.name.as_str(),
        JSXAttributeName::NamespacedName(_) => "",
    }
}

/// The `JSXElement` owning an opening element.
pub fn owner_element<'a>(ctx: &Ctx<'a, '_>, opening_id: NodeId) -> Option<&'a JSXElement<'a>> {
    match ctx.parent_kind(opening_id) {
        AstKind::JSXElement(e) => Some(e),
        _ => None,
    }
}

pub fn has_spread(el: &JSXOpeningElement<'_>) -> bool {
    el.attributes
        .iter()
        .any(|a| matches!(a, JSXAttributeItem::SpreadAttribute(_)))
}

// ---- props -------------------------------------------------------------------------------------

/// The value side of a prop.
#[derive(Clone, Copy)]
pub enum PropValue<'a> {
    /// `<a disabled />`
    Absent,
    Str(&'a StringLiteral<'a>),
    Expr(&'a Expression<'a>),
    /// Element / fragment values, empty expressions.
    Other,
}

/// A prop found on an element: a JSX attribute, or a property of an inline object spread.
#[derive(Clone, Copy)]
pub struct Prop<'a> {
    pub span: Span,
    pub value: PropValue<'a>,
    pub attr: Option<&'a JSXAttribute<'a>>,
}

pub fn attr_value<'a>(a: &'a JSXAttribute<'a>) -> PropValue<'a> {
    match &a.value {
        None => PropValue::Absent,
        Some(JSXAttributeValue::StringLiteral(s)) => PropValue::Str(s),
        Some(JSXAttributeValue::ExpressionContainer(c)) => match c.expression.as_expression() {
            Some(e) => PropValue::Expr(e),
            None => PropValue::Other,
        },
        Some(_) => PropValue::Other,
    }
}

pub fn attr_prop<'a>(a: &'a JSXAttribute<'a>) -> Prop<'a> {
    Prop {
        span: a.span,
        value: attr_value(a),
        attr: Some(a),
    }
}

/// `getProp`: first attribute named `name` (case-insensitive), also looking into inline object
/// literal spreads (`{...{ name: x }}`).
pub fn get_prop<'a>(el: &'a JSXOpeningElement<'a>, name: &str) -> Option<Prop<'a>> {
    for item in &el.attributes {
        match item {
            JSXAttributeItem::Attribute(a) => {
                if attr_name(a).eq_ignore_ascii_case(name) {
                    return Some(attr_prop(a));
                }
            }
            JSXAttributeItem::SpreadAttribute(s) => {
                if name.eq_ignore_ascii_case("key") {
                    continue;
                }
                if let Expression::ObjectExpression(o) = &s.argument {
                    for p in &o.properties {
                        if let ObjectPropertyKind::ObjectProperty(p) = p
                            && let PropertyKey::StaticIdentifier(k) = &p.key
                            && k.name.eq_ignore_ascii_case(name)
                        {
                            return Some(Prop {
                                span: p.span,
                                value: PropValue::Expr(&p.value),
                                attr: None,
                            });
                        }
                    }
                }
            }
        }
    }
    None
}

/// `hasProp`: an attribute named `name` exists (case-insensitive, spreads ignored).
pub fn has_prop(el: &JSXOpeningElement<'_>, name: &str) -> bool {
    el.attributes.iter().any(
        |a| matches!(a, JSXAttributeItem::Attribute(a) if attr_name(a).eq_ignore_ascii_case(name)),
    )
}

pub fn has_any_prop(el: &JSXOpeningElement<'_>, names: &[&str]) -> bool {
    names.iter().any(|n| has_prop(el, n))
}

/// A statically extracted prop value.
#[derive(Clone, Debug, PartialEq)]
pub enum Val<'a> {
    Bool(bool),
    Str(Cow<'a, str>),
    Num(f64),
    Null,
    Undefined,
    /// Not statically known (identifiers, calls, functions, objects, ...).
    Dynamic,
}

impl Val<'_> {
    pub fn is_nullish(&self) -> bool {
        matches!(self, Val::Null | Val::Undefined)
    }

    /// JS truthiness; dynamic values count as truthy (jsx-ast-utils yields their source name).
    pub fn truthy(&self) -> bool {
        match self {
            Val::Bool(b) => *b,
            Val::Str(s) => !s.is_empty(),
            Val::Num(n) => *n != 0.0 && !n.is_nan(),
            Val::Null | Val::Undefined => false,
            Val::Dynamic => true,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Val::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn is_str(&self, s: &str) -> bool {
        self.as_str() == Some(s)
    }

    /// `String(value)` for values that have a static string form.
    pub fn to_js_string(&self) -> Option<Cow<'_, str>> {
        match self {
            Val::Str(s) => Some(Cow::Borrowed(s)),
            Val::Bool(b) => Some(Cow::Borrowed(if *b { "true" } else { "false" })),
            Val::Num(n) => Some(Cow::Owned(js_number(*n))),
            Val::Null => Some(Cow::Borrowed("null")),
            Val::Undefined => Some(Cow::Borrowed("undefined")),
            Val::Dynamic => None,
        }
    }

    /// `Number(value)`; `None` for dynamic values.
    pub fn to_number(&self) -> Option<f64> {
        match self {
            Val::Num(n) => Some(*n),
            Val::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            Val::Null => Some(0.0),
            Val::Undefined => Some(f64::NAN),
            Val::Str(s) => Some(js_str_to_number(s)),
            Val::Dynamic => None,
        }
    }
}

pub fn js_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e21 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// `Number(string)` for the usual decimal forms.
pub fn js_str_to_number(s: &str) -> f64 {
    let t = s.trim();
    if t.is_empty() {
        return 0.0;
    }
    if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return i64::from_str_radix(h, 16).map_or(f64::NAN, |v| v as f64);
    }
    if t.bytes()
        .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'+' | b'e' | b'E'))
    {
        t.parse::<f64>().unwrap_or(f64::NAN)
    } else {
        f64::NAN
    }
}

fn coerce_str(s: &str) -> Val<'_> {
    if s.eq_ignore_ascii_case("true") {
        Val::Bool(true)
    } else if s.eq_ignore_ascii_case("false") {
        Val::Bool(false)
    } else {
        Val::Str(Cow::Borrowed(s))
    }
}

/// jsx-ast-utils' template literal rendering: quasis with `{name}` placeholders.
pub fn template_text<'a>(t: &TemplateLiteral<'a>) -> Cow<'a, str> {
    if t.expressions.is_empty() && t.quasis.len() == 1 {
        return Cow::Borrowed(t.quasis[0].value.raw.as_str());
    }
    let mut out = String::new();
    for (i, q) in t.quasis.iter().enumerate() {
        out.push_str(q.value.raw.as_str());
        if let Some(e) = t.expressions.get(i) {
            match strip_parens(e) {
                Expression::Identifier(id) if id.name == "undefined" => out.push_str("undefined"),
                Expression::Identifier(id) => {
                    out.push('{');
                    out.push_str(id.name.as_str());
                    out.push('}');
                }
                e if e.is_literal() => {}
                _ => out.push_str("{Expression}"),
            }
        }
    }
    Cow::Owned(out)
}

/// Truthiness of a condition as jsx-ast-utils evaluates it (identifiers and member chains yield
/// their source text, hence truthy); `None` when unknown.
fn condition_truthiness(e: &Expression<'_>) -> Option<bool> {
    match strip_parens(e) {
        Expression::Identifier(id) if id.name == "undefined" => Some(false),
        Expression::Identifier(_)
        | Expression::StaticMemberExpression(_)
        | Expression::ThisExpression(_) => Some(true),
        Expression::UnaryExpression(u) if u.operator == UnaryOperator::LogicalNot => {
            condition_truthiness(&u.argument).map(|b| !b)
        }
        e => match expr_value(e, false) {
            Val::Dynamic => None,
            v => Some(v.truthy()),
        },
    }
}

fn expr_value<'a>(e: &'a Expression<'a>, literal: bool) -> Val<'a> {
    match strip_parens(e) {
        Expression::ConditionalExpression(c) if !literal => match condition_truthiness(&c.test) {
            Some(true) => expr_value(&c.consequent, false),
            Some(false) => expr_value(&c.alternate, false),
            None => Val::Dynamic,
        },
        Expression::LogicalExpression(l) if !literal => {
            let left = condition_truthiness(&l.left);
            match (l.operator, left) {
                (LogicalOperator::And, Some(true)) | (LogicalOperator::Or, Some(false)) => {
                    expr_value(&l.right, false)
                }
                (LogicalOperator::And, Some(false)) | (LogicalOperator::Or, Some(true)) => {
                    expr_value(&l.left, false)
                }
                (LogicalOperator::Coalesce, _) => match expr_value(&l.left, false) {
                    v if v.is_nullish() => expr_value(&l.right, false),
                    Val::Dynamic => Val::Dynamic,
                    v => v,
                },
                _ => Val::Dynamic,
            }
        }
        Expression::StringLiteral(s) => coerce_str(s.value.as_str()),
        Expression::NumericLiteral(n) => Val::Num(n.value),
        Expression::BooleanLiteral(b) => Val::Bool(b.value),
        Expression::NullLiteral(_) => {
            if literal {
                Val::Str(Cow::Borrowed("null"))
            } else {
                Val::Null
            }
        }
        Expression::Identifier(id) if id.name == "undefined" => Val::Undefined,
        Expression::TemplateLiteral(t) => Val::Str(template_text(t)),
        Expression::UnaryExpression(u) => match (u.operator, strip_parens(&u.argument)) {
            (UnaryOperator::UnaryNegation, Expression::NumericLiteral(n)) => Val::Num(-n.value),
            (UnaryOperator::UnaryPlus, Expression::NumericLiteral(n)) => Val::Num(n.value),
            (UnaryOperator::Void, _) => Val::Undefined,
            _ => Val::Dynamic,
        },
        _ => Val::Dynamic,
    }
}

fn value_of<'a>(v: PropValue<'a>, literal: bool) -> Val<'a> {
    match v {
        PropValue::Absent => Val::Bool(true),
        PropValue::Str(s) => coerce_str(s.value.as_str()),
        PropValue::Expr(e) => expr_value(e, literal),
        PropValue::Other => Val::Dynamic,
    }
}

/// `getPropValue`.
pub fn prop_value<'a>(p: &Prop<'a>) -> Val<'a> {
    value_of(p.value, false)
}

/// `getLiteralPropValue`: like [`prop_value`], `Dynamic` meaning "not a literal" (JS `null`), and a
/// `null` literal yielding the string `"null"`.
pub fn literal_value<'a>(p: &Prop<'a>) -> Val<'a> {
    value_of(p.value, true)
}

/// `getPropValue(getProp(el, name))`; `Undefined` when the prop is missing.
pub fn prop_value_of<'a>(el: &'a JSXOpeningElement<'a>, name: &str) -> Val<'a> {
    get_prop(el, name).map_or(Val::Undefined, |p| prop_value(&p))
}

/// `getLiteralPropValue(getProp(el, name))`; `Undefined` when the prop is missing.
pub fn literal_value_of<'a>(el: &'a JSXOpeningElement<'a>, name: &str) -> Val<'a> {
    get_prop(el, name).map_or(Val::Undefined, |p| literal_value(&p))
}

/// A template literal with substitutions: not a static value even though jsx-ast-utils renders it.
pub fn is_interpolated(p: &Prop<'_>) -> bool {
    matches!(p.value, PropValue::Expr(e) if matches!(strip_parens(e), Expression::TemplateLiteral(t) if !t.expressions.is_empty()))
}

/// The string value of a static string attribute (`a="x"`, `a={"x"}`, `` a={`x`} ``), without
/// boolean coercion.
pub fn static_string<'a>(v: PropValue<'a>) -> Option<&'a str> {
    match v {
        PropValue::Str(s) => Some(s.value.as_str()),
        PropValue::Expr(e) => string_value(e),
        _ => None,
    }
}

/// The literal `role` as lower-cased string, when it is a string literal.
pub fn role_string<'a>(el: &'a JSXOpeningElement<'a>) -> Option<String> {
    match literal_value_of(el, "role") {
        Val::Str(s) => Some(s.to_ascii_lowercase()),
        _ => None,
    }
}

/// Initializer of a never-reassigned plain variable (`const x = init`).
pub fn sole_init<'a>(
    ctx: &Ctx<'a, '_>,
    id: &IdentifierReference<'a>,
) -> Option<&'a Expression<'a>> {
    let sym = ctx.symbol_of(id)?;
    if ctx.scoping().symbol_is_mutated(sym) {
        return None;
    }
    let AstKind::VariableDeclarator(v) = ctx.symbol_decl(sym).kind() else {
        return None;
    };
    if !matches!(v.id, BindingPattern::BindingIdentifier(_)) {
        return None;
    }
    v.init.as_ref()
}

/// Resolves an expression to an object literal, directly or through a sole-write variable.
pub fn resolve_object<'a>(
    ctx: &Ctx<'a, '_>,
    e: &'a Expression<'a>,
) -> Option<&'a ObjectExpression<'a>> {
    match strip_parens(e) {
        Expression::ObjectExpression(o) => Some(o),
        Expression::Identifier(id) => match strip_parens(sole_init(ctx, id)?) {
            Expression::ObjectExpression(o) => Some(o),
            _ => None,
        },
        _ => None,
    }
}

/// Sonar's `getProperty`: `Some(true)` if the object (or a resolvable spread in it) has `key`,
/// `Some(false)` if it provably doesn't, `None` when an unresolvable spread could supply it.
pub fn object_has_key<'a>(
    ctx: &Ctx<'a, '_>,
    o: &'a ObjectExpression<'a>,
    key: &str,
    depth: u8,
) -> Option<bool> {
    let mut unresolved = false;
    for p in o.properties.iter().rev() {
        match p {
            ObjectPropertyKind::ObjectProperty(p) => {
                let k = match &p.key {
                    PropertyKey::StaticIdentifier(k) => Some(k.name.as_str()),
                    PropertyKey::StringLiteral(s) => Some(s.value.as_str()),
                    _ => None,
                };
                if k == Some(key) {
                    return Some(true);
                }
            }
            ObjectPropertyKind::SpreadProperty(s) => {
                match (depth < 4)
                    .then(|| resolve_object(ctx, &s.argument))
                    .flatten()
                {
                    Some(inner) => match object_has_key(ctx, inner, key, depth + 1) {
                        Some(true) => return Some(true),
                        Some(false) => {}
                        None => unresolved = true,
                    },
                    None => unresolved = true,
                }
            }
        }
    }
    if unresolved { None } else { Some(false) }
}

// ---- tables ------------------------------------------------------------------------------------

pub struct RoleInfo {
    pub name: &'static str,
    pub is_abstract: bool,
    /// Descends from the `widget` super class.
    pub widget: bool,
    /// Supported states and properties (including inherited ones), sorted.
    pub props: &'static [&'static str],
    pub required: &'static [&'static str],
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AriaType {
    Str,
    Id,
    IdList,
    Integer,
    Number,
    Boolean,
    Token,
    TokenList,
    Tristate,
}

pub struct AriaProp {
    pub name: &'static str,
    pub ty: AriaType,
    /// Permitted values; the flag tells a boolean `true` / `false` from a string token.
    pub values: &'static [(&'static str, bool)],
    pub allow_undefined: bool,
}

/// An element description from aria-query / axobject-query: tag plus required attributes.
pub struct Schema {
    pub name: &'static str,
    pub attrs: &'static [(&'static str, Option<&'static str>)],
}

pub fn role_info(name: &str) -> Option<&'static RoleInfo> {
    ROLES
        .binary_search_by(|r| r.name.cmp(name))
        .ok()
        .map(|i| &ROLES[i])
}

pub fn aria_prop(name: &str) -> Option<&'static AriaProp> {
    ARIA_PROPS
        .binary_search_by(|r| r.name.cmp(name))
        .ok()
        .map(|i| &ARIA_PROPS[i])
}

/// Known DOM element (aria-query `dom`).
pub fn is_dom(name: &str) -> bool {
    DOM.binary_search_by(|(n, _)| n.cmp(&name)).is_ok()
}

/// DOM element that does not support ARIA roles, states and properties.
pub fn is_reserved_dom(name: &str) -> bool {
    DOM.binary_search_by(|(n, _)| n.cmp(&name))
        .is_ok_and(|i| DOM[i].1)
}

/// (formatted tags, tag names) of the elements that have `role` natively.
pub fn role_tags(role: &str) -> Option<(&'static str, &'static [&'static str])> {
    ROLE_TAGS
        .binary_search_by(|(r, _, _)| r.cmp(&role))
        .ok()
        .map(|i| (ROLE_TAGS[i].1, ROLE_TAGS[i].2))
}

/// jsx-a11y's `attributesComparator` applied to a schema.
pub fn schema_matches(s: &Schema, tag: &str, el: &JSXOpeningElement<'_>) -> bool {
    s.name == tag
        && s.attrs.iter().all(|(name, value)| {
            el.attributes.iter().any(|item| match item {
                JSXAttributeItem::Attribute(a) => {
                    attr_name(a).eq_ignore_ascii_case(name)
                        && match value {
                            Some(v) if !v.is_empty() => literal_value(&attr_prop(a)).is_str(v),
                            _ => true,
                        }
                }
                JSXAttributeItem::SpreadAttribute(_) => false,
            })
        })
}

fn any_schema(list: &[Schema], tag: &str, el: &JSXOpeningElement<'_>) -> bool {
    list.iter().any(|s| schema_matches(s, tag, el))
}

/// jsx-a11y `isInteractiveElement`.
pub fn is_interactive_element(tag: &str, el: &JSXOpeningElement<'_>) -> bool {
    if !is_dom(tag) {
        return false;
    }
    if any_schema(INTERACTIVE_ELEMENT_ROLE_SCHEMAS, tag, el) {
        return true;
    }
    if any_schema(NON_INTERACTIVE_ELEMENT_ROLE_SCHEMAS, tag, el) {
        return false;
    }
    any_schema(INTERACTIVE_AX_SCHEMAS, tag, el)
}

/// jsx-a11y `isNonInteractiveElement`.
pub fn is_non_interactive_element(tag: &str, el: &JSXOpeningElement<'_>) -> bool {
    if !is_dom(tag) || tag == "header" || tag == "td" {
        return false;
    }
    if any_schema(NON_INTERACTIVE_ELEMENT_ROLE_SCHEMAS, tag, el) {
        return true;
    }
    if any_schema(INTERACTIVE_ELEMENT_ROLE_SCHEMAS, tag, el) {
        return false;
    }
    any_schema(NON_INTERACTIVE_AX_SCHEMAS, tag, el)
}

/// First token of the literal `role` that names a known role.
pub fn first_valid_role<'a>(el: &'a JSXOpeningElement<'a>) -> Option<&'static RoleInfo> {
    let v = literal_value_of(el, "role");
    let s = v.to_js_string()?;
    s.to_ascii_lowercase().split(' ').find_map(role_info)
}

/// jsx-a11y `isInteractiveRole`.
pub fn is_interactive_role<'a>(el: &'a JSXOpeningElement<'a>) -> bool {
    first_valid_role(el).is_some_and(|r| (!r.is_abstract && r.widget) || r.name == "toolbar")
}

/// jsx-a11y `isNonInteractiveRole`.
pub fn is_non_interactive_role<'a>(tag: &str, el: &'a JSXOpeningElement<'a>) -> bool {
    is_dom(tag)
        && first_valid_role(el).is_some_and(|r| {
            !r.is_abstract && !r.widget && r.name != "toolbar" && r.name != "generic"
        })
}

/// jsx-a11y `isPresentationRole`.
pub fn is_presentation_role<'a>(el: &'a JSXOpeningElement<'a>) -> bool {
    matches!(
        literal_value_of(el, "role").as_str(),
        Some("presentation" | "none")
    )
}

/// jsx-a11y `isAbstractRole`.
pub fn is_abstract_role<'a>(tag: &str, el: &'a JSXOpeningElement<'a>) -> bool {
    is_dom(tag)
        && literal_value_of(el, "role")
            .as_str()
            .and_then(role_info)
            .is_some_and(|r| r.is_abstract)
}

/// jsx-a11y `isHiddenFromScreenReader`.
pub fn is_hidden_from_screen_reader<'a>(tag: &str, el: &'a JSXOpeningElement<'a>) -> bool {
    if tag.eq_ignore_ascii_case("input")
        && literal_value_of(el, "type")
            .as_str()
            .is_some_and(|t| t.eq_ignore_ascii_case("hidden"))
    {
        return true;
    }
    prop_value_of(el, "aria-hidden") == Val::Bool(true)
}

/// jsx-a11y `isContentEditable` (conservative: any non-false value).
pub fn is_content_editable<'a>(el: &'a JSXOpeningElement<'a>) -> bool {
    get_prop(el, "contentEditable").is_some_and(|p| !matches!(literal_value(&p), Val::Bool(false)))
}

/// jsx-a11y `isNonLiteralProperty`.
pub fn is_non_literal_property<'a>(el: &'a JSXOpeningElement<'a>, name: &str) -> bool {
    match get_prop(el, name).map(|p| p.value) {
        Some(PropValue::Expr(e)) => {
            !matches!(e, Expression::Identifier(id) if id.name == "undefined")
                && expr_value(e, true) == Val::Dynamic
        }
        _ => false,
    }
}

/// A tab index as computed by jsx-a11y `getTabIndex`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum TabIndex {
    Undefined,
    Int(f64),
    /// Non-literal value: never compares as a number.
    Dynamic,
}

impl TabIndex {
    pub fn is_defined(self) -> bool {
        self != TabIndex::Undefined
    }

    /// `tabIndex >= n` in JS (dynamic values compare false).
    pub fn ge(self, n: f64) -> bool {
        matches!(self, TabIndex::Int(v) if v >= n)
    }
}

pub fn tab_index<'a>(el: &'a JSXOpeningElement<'a>) -> TabIndex {
    let Some(p) = get_prop(el, "tabIndex") else {
        return TabIndex::Undefined;
    };
    match literal_value(&p) {
        Val::Str(s) if s.is_empty() => TabIndex::Undefined,
        Val::Str(s) => {
            let n = js_str_to_number(&s);
            if n.is_nan() || n.fract() != 0.0 {
                TabIndex::Undefined
            } else {
                TabIndex::Int(n)
            }
        }
        Val::Num(n) => {
            if n.is_nan() || n.fract() != 0.0 || n.is_infinite() {
                TabIndex::Undefined
            } else {
                TabIndex::Int(n)
            }
        }
        Val::Bool(_) => TabIndex::Undefined,
        _ => match prop_value(&p) {
            Val::Undefined => TabIndex::Undefined,
            Val::Null => TabIndex::Int(0.0),
            _ => TabIndex::Dynamic,
        },
    }
}

/// jsx-a11y implicit role of a DOM element, `""` when it has none.
pub fn implicit_role<'a>(tag: &str, el: &'a JSXOpeningElement<'a>) -> &'static str {
    let upper_type = |el: &'a JSXOpeningElement<'a>| match literal_value_of(el, "type") {
        Val::Str(s) => s.to_ascii_uppercase(),
        _ => String::new(),
    };
    match tag {
        "a" | "area" | "link" => {
            if get_prop(el, "href").is_some() {
                "link"
            } else {
                ""
            }
        }
        "article" => "article",
        "aside" => "complementary",
        "body" => "document",
        "button" => "button",
        "datalist" => "listbox",
        "details" => "group",
        "dialog" => "dialog",
        "form" => "form",
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => "heading",
        "hr" => "separator",
        "img" => {
            if get_prop(el, "alt").is_some_and(|p| literal_value(&p).is_str("")) {
                ""
            } else {
                "img"
            }
        }
        "input" => {
            if get_prop(el, "type").is_none() {
                return "textbox";
            }
            match upper_type(el).as_str() {
                "BUTTON" | "IMAGE" | "RESET" | "SUBMIT" => "button",
                "CHECKBOX" => "checkbox",
                "RADIO" => "radio",
                "RANGE" => "slider",
                _ => "textbox",
            }
        }
        "li" => "listitem",
        "menu" => {
            if upper_type(el) == "TOOLBAR" {
                "toolbar"
            } else {
                ""
            }
        }
        "menuitem" => match upper_type(el).as_str() {
            "COMMAND" => "menuitem",
            "CHECKBOX" => "menuitemcheckbox",
            "RADIO" => "menuitemradio",
            _ => "",
        },
        "meter" | "progress" => "progressbar",
        "nav" => "navigation",
        "ol" | "ul" => "list",
        "option" => "option",
        "output" => "status",
        "section" => "region",
        "select" => "listbox",
        "tbody" | "tfoot" | "thead" => "rowgroup",
        "textarea" => "textbox",
        _ => "",
    }
}

/// An event handler prop with a non-null value (`hasProp && getPropValue != null`).
pub fn has_handler<'a>(el: &'a JSXOpeningElement<'a>, name: &str) -> bool {
    has_prop(el, name) && get_prop(el, name).is_some_and(|p| !prop_value(&p).is_nullish())
}

pub const FOCUS_HANDLERS: &[&str] = &["onFocus", "onBlur"];
pub const KEYBOARD_HANDLERS: &[&str] = &["onKeyDown", "onKeyPress", "onKeyUp"];
pub const MOUSE_HANDLERS: &[&str] = &[
    "onClick",
    "onContextMenu",
    "onDblClick",
    "onDoubleClick",
    "onDrag",
    "onDragEnd",
    "onDragEnter",
    "onDragExit",
    "onDragLeave",
    "onDragOver",
    "onDragStart",
    "onDrop",
    "onMouseDown",
    "onMouseEnter",
    "onMouseLeave",
    "onMouseMove",
    "onMouseOut",
    "onMouseOver",
    "onMouseUp",
];

/// jsx-a11y `hasAccessibleChild`.
pub fn has_accessible_child<'a>(el: &'a JSXElement<'a>) -> bool {
    el.children.iter().any(|c| match c {
        JSXChild::Text(t) => !t.value.is_empty(),
        JSXChild::Element(e) => {
            !is_hidden_from_screen_reader(element_name(&e.opening_element), &e.opening_element)
        }
        JSXChild::ExpressionContainer(c) => !c.expression.is_undefined(),
        _ => false,
    }) || has_any_prop(
        &el.opening_element,
        &["dangerouslySetInnerHTML", "children"],
    )
}

/// Sonar's `hasAccessibleNameAttribute`: a non-empty static string, a possibly non-empty template
/// literal or a non-nullish dynamic value.
pub fn has_accessible_name_attribute<'a>(el: &'a JSXOpeningElement<'a>, name: &str) -> bool {
    let Some(p) = get_prop(el, name) else {
        return false;
    };
    match p.value {
        PropValue::Str(s) => return !s.value.trim().is_empty(),
        PropValue::Expr(e) => match strip_parens(e) {
            Expression::StringLiteral(s) => return !s.value.trim().is_empty(),
            Expression::TemplateLiteral(t) => {
                return !t.expressions.is_empty()
                    || t.quasis
                        .iter()
                        .any(|q| q.value.cooked.as_ref().is_none_or(|c| !c.trim().is_empty()));
            }
            _ => {}
        },
        _ => {}
    }
    literal_value(&p) == Val::Dynamic && !prop_value(&p).is_nullish()
}

/// HTML tag names known to SonarJS (`isHtmlElement`).
pub fn is_html_tag(name: &str) -> bool {
    HTML_TAG_NAMES.binary_search(&name).is_ok()
}

static HTML_TAG_NAMES: &[&str] = &[
    "a",
    "abbr",
    "acronym",
    "address",
    "applet",
    "area",
    "article",
    "aside",
    "audio",
    "b",
    "base",
    "basefont",
    "bdi",
    "bdo",
    "bgsound",
    "big",
    "blink",
    "blockquote",
    "body",
    "br",
    "button",
    "canvas",
    "caption",
    "center",
    "cite",
    "code",
    "col",
    "colgroup",
    "command",
    "content",
    "data",
    "datalist",
    "dd",
    "del",
    "details",
    "dfn",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "element",
    "em",
    "embed",
    "fieldset",
    "figcaption",
    "figure",
    "font",
    "footer",
    "form",
    "frame",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hgroup",
    "hr",
    "html",
    "i",
    "iframe",
    "image",
    "img",
    "input",
    "ins",
    "isindex",
    "kbd",
    "keygen",
    "label",
    "legend",
    "li",
    "link",
    "listing",
    "main",
    "map",
    "mark",
    "marquee",
    "math",
    "menu",
    "menuitem",
    "meta",
    "meter",
    "multicol",
    "nav",
    "nextid",
    "nobr",
    "noembed",
    "noframes",
    "noscript",
    "object",
    "ol",
    "optgroup",
    "option",
    "output",
    "p",
    "param",
    "picture",
    "plaintext",
    "pre",
    "progress",
    "q",
    "rb",
    "rbc",
    "rp",
    "rt",
    "rtc",
    "ruby",
    "s",
    "samp",
    "script",
    "search",
    "section",
    "select",
    "shadow",
    "slot",
    "small",
    "source",
    "spacer",
    "span",
    "strike",
    "strong",
    "style",
    "sub",
    "summary",
    "sup",
    "svg",
    "table",
    "tbody",
    "td",
    "template",
    "textarea",
    "tfoot",
    "th",
    "thead",
    "time",
    "title",
    "tr",
    "track",
    "tt",
    "u",
    "ul",
    "var",
    "video",
    "wbr",
    "xmp",
];

// ---- generated from aria-query 5.3.2 / axobject-query 4.1.0 ------------------------------------

/// (name, abstract, widget-descendant, supported props, required props)
pub static ROLES: &[RoleInfo] = &[
    RoleInfo {
        name: "alert",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "alertdialog",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-modal",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "application",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-activedescendant",
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "article",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-posinset",
            "aria-relevant",
            "aria-roledescription",
            "aria-setsize",
        ],
        required: &[],
    },
    RoleInfo {
        name: "banner",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "blockquote",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "button",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-pressed",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "caption",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "cell",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-colindex",
            "aria-colspan",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
            "aria-rowindex",
            "aria-rowspan",
        ],
        required: &[],
    },
    RoleInfo {
        name: "checkbox",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-checked",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-readonly",
            "aria-relevant",
            "aria-required",
            "aria-roledescription",
        ],
        required: &["aria-checked"],
    },
    RoleInfo {
        name: "code",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "columnheader",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-colindex",
            "aria-colspan",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-readonly",
            "aria-relevant",
            "aria-required",
            "aria-roledescription",
            "aria-rowindex",
            "aria-rowspan",
            "aria-selected",
            "aria-sort",
        ],
        required: &[],
    },
    RoleInfo {
        name: "combobox",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-activedescendant",
            "aria-atomic",
            "aria-autocomplete",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-readonly",
            "aria-relevant",
            "aria-required",
            "aria-roledescription",
        ],
        required: &["aria-controls", "aria-expanded"],
    },
    RoleInfo {
        name: "command",
        is_abstract: true,
        widget: true,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "complementary",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "composite",
        is_abstract: true,
        widget: true,
        props: &[
            "aria-activedescendant",
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "contentinfo",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "definition",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "deletion",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "dialog",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-modal",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "directory",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-abstract",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-acknowledgments",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-afterword",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-appendix",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-backlink",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-biblioentry",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-level",
            "aria-live",
            "aria-owns",
            "aria-posinset",
            "aria-relevant",
            "aria-roledescription",
            "aria-setsize",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-bibliography",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-biblioref",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-chapter",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-colophon",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-conclusion",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-cover",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-credit",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-credits",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-dedication",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-endnote",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-level",
            "aria-live",
            "aria-owns",
            "aria-posinset",
            "aria-relevant",
            "aria-roledescription",
            "aria-setsize",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-endnotes",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-epigraph",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-epilogue",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-errata",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-example",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-footnote",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-foreword",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-glossary",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-glossref",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-index",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-introduction",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-noteref",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-notice",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-pagebreak",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-orientation",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
            "aria-valuemax",
            "aria-valuemin",
            "aria-valuenow",
            "aria-valuetext",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-pagefooter",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-braillelabel",
            "aria-brailleroledescription",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-description",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-pageheader",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-braillelabel",
            "aria-brailleroledescription",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-description",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-pagelist",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-part",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-preface",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-prologue",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-pullquote",
        is_abstract: false,
        widget: false,
        props: &[],
        required: &[],
    },
    RoleInfo {
        name: "doc-qna",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-subtitle",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-tip",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "doc-toc",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "document",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "emphasis",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "feed",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "figure",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "form",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "generic",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "graphics-document",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "graphics-object",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-activedescendant",
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "graphics-symbol",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "grid",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-activedescendant",
            "aria-atomic",
            "aria-busy",
            "aria-colcount",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-multiselectable",
            "aria-owns",
            "aria-readonly",
            "aria-relevant",
            "aria-roledescription",
            "aria-rowcount",
        ],
        required: &[],
    },
    RoleInfo {
        name: "gridcell",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-colindex",
            "aria-colspan",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-readonly",
            "aria-relevant",
            "aria-required",
            "aria-roledescription",
            "aria-rowindex",
            "aria-rowspan",
            "aria-selected",
        ],
        required: &[],
    },
    RoleInfo {
        name: "group",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-activedescendant",
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "heading",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-level",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &["aria-level"],
    },
    RoleInfo {
        name: "img",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "input",
        is_abstract: true,
        widget: true,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "insertion",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "landmark",
        is_abstract: true,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "link",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "list",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "listbox",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-activedescendant",
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-multiselectable",
            "aria-orientation",
            "aria-owns",
            "aria-readonly",
            "aria-relevant",
            "aria-required",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "listitem",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-level",
            "aria-live",
            "aria-owns",
            "aria-posinset",
            "aria-relevant",
            "aria-roledescription",
            "aria-setsize",
        ],
        required: &[],
    },
    RoleInfo {
        name: "log",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "main",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "mark",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-braillelabel",
            "aria-brailleroledescription",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-description",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "marquee",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "math",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "menu",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-activedescendant",
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-orientation",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "menubar",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-activedescendant",
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-orientation",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "menuitem",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-posinset",
            "aria-relevant",
            "aria-roledescription",
            "aria-setsize",
        ],
        required: &[],
    },
    RoleInfo {
        name: "menuitemcheckbox",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-checked",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-posinset",
            "aria-readonly",
            "aria-relevant",
            "aria-required",
            "aria-roledescription",
            "aria-setsize",
        ],
        required: &["aria-checked"],
    },
    RoleInfo {
        name: "menuitemradio",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-checked",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-posinset",
            "aria-readonly",
            "aria-relevant",
            "aria-required",
            "aria-roledescription",
            "aria-setsize",
        ],
        required: &["aria-checked"],
    },
    RoleInfo {
        name: "meter",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
            "aria-valuemax",
            "aria-valuemin",
            "aria-valuenow",
            "aria-valuetext",
        ],
        required: &["aria-valuenow"],
    },
    RoleInfo {
        name: "navigation",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "none",
        is_abstract: false,
        widget: false,
        props: &[],
        required: &[],
    },
    RoleInfo {
        name: "note",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "option",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-checked",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-posinset",
            "aria-relevant",
            "aria-roledescription",
            "aria-selected",
            "aria-setsize",
        ],
        required: &["aria-selected"],
    },
    RoleInfo {
        name: "paragraph",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "presentation",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "progressbar",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
            "aria-valuemax",
            "aria-valuemin",
            "aria-valuenow",
            "aria-valuetext",
        ],
        required: &[],
    },
    RoleInfo {
        name: "radio",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-checked",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-posinset",
            "aria-relevant",
            "aria-roledescription",
            "aria-setsize",
        ],
        required: &["aria-checked"],
    },
    RoleInfo {
        name: "radiogroup",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-activedescendant",
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-orientation",
            "aria-owns",
            "aria-readonly",
            "aria-relevant",
            "aria-required",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "range",
        is_abstract: true,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
            "aria-valuemax",
            "aria-valuemin",
            "aria-valuenow",
        ],
        required: &[],
    },
    RoleInfo {
        name: "region",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "roletype",
        is_abstract: true,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "row",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-activedescendant",
            "aria-atomic",
            "aria-busy",
            "aria-colindex",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-level",
            "aria-live",
            "aria-owns",
            "aria-posinset",
            "aria-relevant",
            "aria-roledescription",
            "aria-rowindex",
            "aria-selected",
            "aria-setsize",
        ],
        required: &[],
    },
    RoleInfo {
        name: "rowgroup",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "rowheader",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-colindex",
            "aria-colspan",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-readonly",
            "aria-relevant",
            "aria-required",
            "aria-roledescription",
            "aria-rowindex",
            "aria-rowspan",
            "aria-selected",
            "aria-sort",
        ],
        required: &[],
    },
    RoleInfo {
        name: "scrollbar",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-orientation",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
            "aria-valuemax",
            "aria-valuemin",
            "aria-valuenow",
            "aria-valuetext",
        ],
        required: &["aria-controls", "aria-valuenow"],
    },
    RoleInfo {
        name: "search",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "searchbox",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-activedescendant",
            "aria-atomic",
            "aria-autocomplete",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-multiline",
            "aria-owns",
            "aria-placeholder",
            "aria-readonly",
            "aria-relevant",
            "aria-required",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "section",
        is_abstract: true,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "sectionhead",
        is_abstract: true,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "select",
        is_abstract: true,
        widget: true,
        props: &[
            "aria-activedescendant",
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-orientation",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "separator",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-orientation",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
            "aria-valuemax",
            "aria-valuemin",
            "aria-valuenow",
            "aria-valuetext",
        ],
        required: &[],
    },
    RoleInfo {
        name: "slider",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-orientation",
            "aria-owns",
            "aria-readonly",
            "aria-relevant",
            "aria-roledescription",
            "aria-valuemax",
            "aria-valuemin",
            "aria-valuenow",
            "aria-valuetext",
        ],
        required: &["aria-valuenow"],
    },
    RoleInfo {
        name: "spinbutton",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-activedescendant",
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-readonly",
            "aria-relevant",
            "aria-required",
            "aria-roledescription",
            "aria-valuemax",
            "aria-valuemin",
            "aria-valuenow",
            "aria-valuetext",
        ],
        required: &[],
    },
    RoleInfo {
        name: "status",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "strong",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "structure",
        is_abstract: true,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "subscript",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "superscript",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "switch",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-checked",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-readonly",
            "aria-relevant",
            "aria-required",
            "aria-roledescription",
        ],
        required: &["aria-checked"],
    },
    RoleInfo {
        name: "tab",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-posinset",
            "aria-relevant",
            "aria-roledescription",
            "aria-selected",
            "aria-setsize",
        ],
        required: &[],
    },
    RoleInfo {
        name: "table",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-colcount",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
            "aria-rowcount",
        ],
        required: &[],
    },
    RoleInfo {
        name: "tablist",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-activedescendant",
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-level",
            "aria-live",
            "aria-multiselectable",
            "aria-orientation",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "tabpanel",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "term",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "textbox",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-activedescendant",
            "aria-atomic",
            "aria-autocomplete",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-multiline",
            "aria-owns",
            "aria-placeholder",
            "aria-readonly",
            "aria-relevant",
            "aria-required",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "time",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "timer",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "toolbar",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-activedescendant",
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-orientation",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "tooltip",
        is_abstract: false,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "tree",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-activedescendant",
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-multiselectable",
            "aria-orientation",
            "aria-owns",
            "aria-relevant",
            "aria-required",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "treegrid",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-activedescendant",
            "aria-atomic",
            "aria-busy",
            "aria-colcount",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-errormessage",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-invalid",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-multiselectable",
            "aria-orientation",
            "aria-owns",
            "aria-readonly",
            "aria-relevant",
            "aria-required",
            "aria-roledescription",
            "aria-rowcount",
        ],
        required: &[],
    },
    RoleInfo {
        name: "treeitem",
        is_abstract: false,
        widget: true,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-checked",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-disabled",
            "aria-dropeffect",
            "aria-expanded",
            "aria-flowto",
            "aria-grabbed",
            "aria-haspopup",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-level",
            "aria-live",
            "aria-owns",
            "aria-posinset",
            "aria-relevant",
            "aria-roledescription",
            "aria-selected",
            "aria-setsize",
        ],
        required: &["aria-selected"],
    },
    RoleInfo {
        name: "widget",
        is_abstract: true,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
    RoleInfo {
        name: "window",
        is_abstract: true,
        widget: false,
        props: &[
            "aria-atomic",
            "aria-busy",
            "aria-controls",
            "aria-current",
            "aria-describedby",
            "aria-details",
            "aria-dropeffect",
            "aria-flowto",
            "aria-grabbed",
            "aria-hidden",
            "aria-keyshortcuts",
            "aria-label",
            "aria-labelledby",
            "aria-live",
            "aria-modal",
            "aria-owns",
            "aria-relevant",
            "aria-roledescription",
        ],
        required: &[],
    },
];
pub static ARIA_PROPS: &[AriaProp] = &[
    AriaProp {
        name: "aria-activedescendant",
        ty: AriaType::Id,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-atomic",
        ty: AriaType::Boolean,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-autocomplete",
        ty: AriaType::Token,
        values: &[
            ("inline", false),
            ("list", false),
            ("both", false),
            ("none", false),
        ],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-braillelabel",
        ty: AriaType::Str,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-brailleroledescription",
        ty: AriaType::Str,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-busy",
        ty: AriaType::Boolean,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-checked",
        ty: AriaType::Tristate,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-colcount",
        ty: AriaType::Integer,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-colindex",
        ty: AriaType::Integer,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-colspan",
        ty: AriaType::Integer,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-controls",
        ty: AriaType::IdList,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-current",
        ty: AriaType::Token,
        values: &[
            ("page", false),
            ("step", false),
            ("location", false),
            ("date", false),
            ("time", false),
            ("true", true),
            ("false", true),
        ],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-describedby",
        ty: AriaType::IdList,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-description",
        ty: AriaType::Str,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-details",
        ty: AriaType::Id,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-disabled",
        ty: AriaType::Boolean,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-dropeffect",
        ty: AriaType::TokenList,
        values: &[
            ("copy", false),
            ("execute", false),
            ("link", false),
            ("move", false),
            ("none", false),
            ("popup", false),
        ],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-errormessage",
        ty: AriaType::Id,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-expanded",
        ty: AriaType::Boolean,
        values: &[],
        allow_undefined: true,
    },
    AriaProp {
        name: "aria-flowto",
        ty: AriaType::IdList,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-grabbed",
        ty: AriaType::Boolean,
        values: &[],
        allow_undefined: true,
    },
    AriaProp {
        name: "aria-haspopup",
        ty: AriaType::Token,
        values: &[
            ("false", true),
            ("true", true),
            ("menu", false),
            ("listbox", false),
            ("tree", false),
            ("grid", false),
            ("dialog", false),
        ],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-hidden",
        ty: AriaType::Boolean,
        values: &[],
        allow_undefined: true,
    },
    AriaProp {
        name: "aria-invalid",
        ty: AriaType::Token,
        values: &[
            ("grammar", false),
            ("false", true),
            ("spelling", false),
            ("true", true),
        ],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-keyshortcuts",
        ty: AriaType::Str,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-label",
        ty: AriaType::Str,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-labelledby",
        ty: AriaType::IdList,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-level",
        ty: AriaType::Integer,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-live",
        ty: AriaType::Token,
        values: &[("assertive", false), ("off", false), ("polite", false)],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-modal",
        ty: AriaType::Boolean,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-multiline",
        ty: AriaType::Boolean,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-multiselectable",
        ty: AriaType::Boolean,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-orientation",
        ty: AriaType::Token,
        values: &[
            ("vertical", false),
            ("undefined", false),
            ("horizontal", false),
        ],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-owns",
        ty: AriaType::IdList,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-placeholder",
        ty: AriaType::Str,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-posinset",
        ty: AriaType::Integer,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-pressed",
        ty: AriaType::Tristate,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-readonly",
        ty: AriaType::Boolean,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-relevant",
        ty: AriaType::TokenList,
        values: &[
            ("additions", false),
            ("all", false),
            ("removals", false),
            ("text", false),
        ],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-required",
        ty: AriaType::Boolean,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-roledescription",
        ty: AriaType::Str,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-rowcount",
        ty: AriaType::Integer,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-rowindex",
        ty: AriaType::Integer,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-rowspan",
        ty: AriaType::Integer,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-selected",
        ty: AriaType::Boolean,
        values: &[],
        allow_undefined: true,
    },
    AriaProp {
        name: "aria-setsize",
        ty: AriaType::Integer,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-sort",
        ty: AriaType::Token,
        values: &[
            ("ascending", false),
            ("descending", false),
            ("none", false),
            ("other", false),
        ],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-valuemax",
        ty: AriaType::Number,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-valuemin",
        ty: AriaType::Number,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-valuenow",
        ty: AriaType::Number,
        values: &[],
        allow_undefined: false,
    },
    AriaProp {
        name: "aria-valuetext",
        ty: AriaType::Str,
        values: &[],
        allow_undefined: false,
    },
];
/// (element, reserved)
pub static DOM: &[(&str, bool)] = &[
    ("a", false),
    ("abbr", false),
    ("acronym", false),
    ("address", false),
    ("applet", false),
    ("area", false),
    ("article", false),
    ("aside", false),
    ("audio", false),
    ("b", false),
    ("base", true),
    ("bdi", false),
    ("bdo", false),
    ("big", false),
    ("blink", false),
    ("blockquote", false),
    ("body", false),
    ("br", false),
    ("button", false),
    ("canvas", false),
    ("caption", false),
    ("center", false),
    ("cite", false),
    ("code", false),
    ("col", true),
    ("colgroup", true),
    ("content", false),
    ("data", false),
    ("datalist", false),
    ("dd", false),
    ("del", false),
    ("details", false),
    ("dfn", false),
    ("dialog", false),
    ("dir", false),
    ("div", false),
    ("dl", false),
    ("dt", false),
    ("em", false),
    ("embed", false),
    ("fieldset", false),
    ("figcaption", false),
    ("figure", false),
    ("font", false),
    ("footer", false),
    ("form", false),
    ("frame", false),
    ("frameset", false),
    ("h1", false),
    ("h2", false),
    ("h3", false),
    ("h4", false),
    ("h5", false),
    ("h6", false),
    ("head", true),
    ("header", false),
    ("hgroup", false),
    ("hr", false),
    ("html", true),
    ("i", false),
    ("iframe", false),
    ("img", false),
    ("input", false),
    ("ins", false),
    ("kbd", false),
    ("keygen", false),
    ("label", false),
    ("legend", false),
    ("li", false),
    ("link", true),
    ("main", false),
    ("map", false),
    ("mark", false),
    ("marquee", false),
    ("menu", false),
    ("menuitem", false),
    ("meta", true),
    ("meter", false),
    ("nav", false),
    ("noembed", true),
    ("noscript", true),
    ("object", false),
    ("ol", false),
    ("optgroup", false),
    ("option", false),
    ("output", false),
    ("p", false),
    ("param", true),
    ("picture", true),
    ("pre", false),
    ("progress", false),
    ("q", false),
    ("rp", false),
    ("rt", false),
    ("rtc", false),
    ("ruby", false),
    ("s", false),
    ("samp", false),
    ("script", true),
    ("section", false),
    ("select", false),
    ("small", false),
    ("source", true),
    ("spacer", false),
    ("span", false),
    ("strike", false),
    ("strong", false),
    ("style", true),
    ("sub", false),
    ("summary", false),
    ("sup", false),
    ("table", false),
    ("tbody", false),
    ("td", false),
    ("textarea", false),
    ("tfoot", false),
    ("th", false),
    ("thead", false),
    ("time", false),
    ("title", true),
    ("tr", false),
    ("track", true),
    ("tt", false),
    ("u", false),
    ("ul", false),
    ("var", false),
    ("video", false),
    ("wbr", false),
    ("xmp", false),
];
pub static INTERACTIVE_ELEMENT_ROLE_SCHEMAS: &[Schema] = &[
    Schema {
        name: "input",
        attrs: &[("type", Some("button"))],
    },
    Schema {
        name: "input",
        attrs: &[("type", Some("image"))],
    },
    Schema {
        name: "input",
        attrs: &[("type", Some("reset"))],
    },
    Schema {
        name: "input",
        attrs: &[("type", Some("submit"))],
    },
    Schema {
        name: "button",
        attrs: &[],
    },
    Schema {
        name: "input",
        attrs: &[("type", Some("checkbox"))],
    },
    Schema {
        name: "th",
        attrs: &[],
    },
    Schema {
        name: "th",
        attrs: &[("scope", Some("col"))],
    },
    Schema {
        name: "th",
        attrs: &[("scope", Some("colgroup"))],
    },
    Schema {
        name: "input",
        attrs: &[("list", None), ("type", Some("email"))],
    },
    Schema {
        name: "input",
        attrs: &[("list", None), ("type", Some("search"))],
    },
    Schema {
        name: "input",
        attrs: &[("list", None), ("type", Some("tel"))],
    },
    Schema {
        name: "input",
        attrs: &[("list", None), ("type", Some("text"))],
    },
    Schema {
        name: "input",
        attrs: &[("list", None), ("type", Some("url"))],
    },
    Schema {
        name: "select",
        attrs: &[("multiple", None), ("size", None)],
    },
    Schema {
        name: "td",
        attrs: &[],
    },
    Schema {
        name: "a",
        attrs: &[("href", None)],
    },
    Schema {
        name: "area",
        attrs: &[("href", None)],
    },
    Schema {
        name: "select",
        attrs: &[("size", None)],
    },
    Schema {
        name: "select",
        attrs: &[("multiple", None)],
    },
    Schema {
        name: "datalist",
        attrs: &[],
    },
    Schema {
        name: "option",
        attrs: &[],
    },
    Schema {
        name: "input",
        attrs: &[("type", Some("radio"))],
    },
    Schema {
        name: "tr",
        attrs: &[],
    },
    Schema {
        name: "th",
        attrs: &[("scope", Some("row"))],
    },
    Schema {
        name: "th",
        attrs: &[("scope", Some("rowgroup"))],
    },
    Schema {
        name: "input",
        attrs: &[("list", None), ("type", Some("search"))],
    },
    Schema {
        name: "input",
        attrs: &[("type", Some("range"))],
    },
    Schema {
        name: "input",
        attrs: &[("type", Some("number"))],
    },
    Schema {
        name: "input",
        attrs: &[("type", None), ("list", None)],
    },
    Schema {
        name: "input",
        attrs: &[("list", None), ("type", Some("email"))],
    },
    Schema {
        name: "input",
        attrs: &[("list", None), ("type", Some("tel"))],
    },
    Schema {
        name: "input",
        attrs: &[("list", None), ("type", Some("text"))],
    },
    Schema {
        name: "input",
        attrs: &[("list", None), ("type", Some("url"))],
    },
    Schema {
        name: "textarea",
        attrs: &[],
    },
];
pub static NON_INTERACTIVE_ELEMENT_ROLE_SCHEMAS: &[Schema] = &[
    Schema {
        name: "article",
        attrs: &[],
    },
    Schema {
        name: "header",
        attrs: &[],
    },
    Schema {
        name: "blockquote",
        attrs: &[],
    },
    Schema {
        name: "caption",
        attrs: &[],
    },
    Schema {
        name: "td",
        attrs: &[],
    },
    Schema {
        name: "code",
        attrs: &[],
    },
    Schema {
        name: "aside",
        attrs: &[],
    },
    Schema {
        name: "aside",
        attrs: &[("aria-label", None)],
    },
    Schema {
        name: "aside",
        attrs: &[("aria-labelledby", None)],
    },
    Schema {
        name: "footer",
        attrs: &[],
    },
    Schema {
        name: "dd",
        attrs: &[],
    },
    Schema {
        name: "del",
        attrs: &[],
    },
    Schema {
        name: "dialog",
        attrs: &[],
    },
    Schema {
        name: "html",
        attrs: &[],
    },
    Schema {
        name: "em",
        attrs: &[],
    },
    Schema {
        name: "figure",
        attrs: &[],
    },
    Schema {
        name: "form",
        attrs: &[("aria-label", None)],
    },
    Schema {
        name: "form",
        attrs: &[("aria-labelledby", None)],
    },
    Schema {
        name: "form",
        attrs: &[("name", None)],
    },
    Schema {
        name: "details",
        attrs: &[],
    },
    Schema {
        name: "fieldset",
        attrs: &[],
    },
    Schema {
        name: "optgroup",
        attrs: &[],
    },
    Schema {
        name: "address",
        attrs: &[],
    },
    Schema {
        name: "h1",
        attrs: &[],
    },
    Schema {
        name: "h2",
        attrs: &[],
    },
    Schema {
        name: "h3",
        attrs: &[],
    },
    Schema {
        name: "h4",
        attrs: &[],
    },
    Schema {
        name: "h5",
        attrs: &[],
    },
    Schema {
        name: "h6",
        attrs: &[],
    },
    Schema {
        name: "img",
        attrs: &[("alt", None)],
    },
    Schema {
        name: "img",
        attrs: &[("alt", None)],
    },
    Schema {
        name: "ins",
        attrs: &[],
    },
    Schema {
        name: "menu",
        attrs: &[],
    },
    Schema {
        name: "ol",
        attrs: &[],
    },
    Schema {
        name: "ul",
        attrs: &[],
    },
    Schema {
        name: "li",
        attrs: &[],
    },
    Schema {
        name: "main",
        attrs: &[],
    },
    Schema {
        name: "mark",
        attrs: &[],
    },
    Schema {
        name: "math",
        attrs: &[],
    },
    Schema {
        name: "meter",
        attrs: &[],
    },
    Schema {
        name: "nav",
        attrs: &[],
    },
    Schema {
        name: "p",
        attrs: &[],
    },
    Schema {
        name: "img",
        attrs: &[("alt", Some(""))],
    },
    Schema {
        name: "progress",
        attrs: &[],
    },
    Schema {
        name: "section",
        attrs: &[("aria-label", None)],
    },
    Schema {
        name: "section",
        attrs: &[("aria-labelledby", None)],
    },
    Schema {
        name: "tbody",
        attrs: &[],
    },
    Schema {
        name: "tfoot",
        attrs: &[],
    },
    Schema {
        name: "thead",
        attrs: &[],
    },
    Schema {
        name: "hr",
        attrs: &[],
    },
    Schema {
        name: "output",
        attrs: &[],
    },
    Schema {
        name: "strong",
        attrs: &[],
    },
    Schema {
        name: "sub",
        attrs: &[],
    },
    Schema {
        name: "sup",
        attrs: &[],
    },
    Schema {
        name: "table",
        attrs: &[],
    },
    Schema {
        name: "dfn",
        attrs: &[],
    },
    Schema {
        name: "dt",
        attrs: &[],
    },
    Schema {
        name: "time",
        attrs: &[],
    },
];
pub static INTERACTIVE_AX_SCHEMAS: &[Schema] = &[
    Schema {
        name: "audio",
        attrs: &[],
    },
    Schema {
        name: "button",
        attrs: &[],
    },
    Schema {
        name: "canvas",
        attrs: &[],
    },
    Schema {
        name: "td",
        attrs: &[],
    },
    Schema {
        name: "input",
        attrs: &[("type", Some("checkbox"))],
    },
    Schema {
        name: "input",
        attrs: &[("type", Some("color"))],
    },
    Schema {
        name: "th",
        attrs: &[],
    },
    Schema {
        name: "select",
        attrs: &[],
    },
    Schema {
        name: "input",
        attrs: &[("type", Some("date"))],
    },
    Schema {
        name: "input",
        attrs: &[("type", Some("datetime"))],
    },
    Schema {
        name: "summary",
        attrs: &[],
    },
    Schema {
        name: "embed",
        attrs: &[],
    },
    Schema {
        name: "input",
        attrs: &[],
    },
    Schema {
        name: "input",
        attrs: &[("type", Some("time"))],
    },
    Schema {
        name: "a",
        attrs: &[("href", None)],
    },
    Schema {
        name: "option",
        attrs: &[],
    },
    Schema {
        name: "datalist",
        attrs: &[],
    },
    Schema {
        name: "menuitem",
        attrs: &[],
    },
    Schema {
        name: "input",
        attrs: &[("type", Some("radio"))],
    },
    Schema {
        name: "th",
        attrs: &[("scope", Some("row"))],
    },
    Schema {
        name: "input",
        attrs: &[("type", Some("search"))],
    },
    Schema {
        name: "input",
        attrs: &[("type", Some("range"))],
    },
    Schema {
        name: "input",
        attrs: &[("type", Some("number"))],
    },
    Schema {
        name: "textarea",
        attrs: &[],
    },
    Schema {
        name: "input",
        attrs: &[("type", Some("text"))],
    },
    Schema {
        name: "video",
        attrs: &[],
    },
];
pub static NON_INTERACTIVE_AX_SCHEMAS: &[Schema] = &[
    Schema {
        name: "abbr",
        attrs: &[],
    },
    Schema {
        name: "article",
        attrs: &[],
    },
    Schema {
        name: "blockquote",
        attrs: &[],
    },
    Schema {
        name: "caption",
        attrs: &[],
    },
    Schema {
        name: "dfn",
        attrs: &[],
    },
    Schema {
        name: "dd",
        attrs: &[],
    },
    Schema {
        name: "dl",
        attrs: &[],
    },
    Schema {
        name: "dt",
        attrs: &[],
    },
    Schema {
        name: "details",
        attrs: &[],
    },
    Schema {
        name: "dialog",
        attrs: &[],
    },
    Schema {
        name: "dir",
        attrs: &[],
    },
    Schema {
        name: "figcaption",
        attrs: &[],
    },
    Schema {
        name: "figure",
        attrs: &[],
    },
    Schema {
        name: "footer",
        attrs: &[],
    },
    Schema {
        name: "form",
        attrs: &[],
    },
    Schema {
        name: "h1",
        attrs: &[],
    },
    Schema {
        name: "h2",
        attrs: &[],
    },
    Schema {
        name: "h3",
        attrs: &[],
    },
    Schema {
        name: "h4",
        attrs: &[],
    },
    Schema {
        name: "h5",
        attrs: &[],
    },
    Schema {
        name: "h6",
        attrs: &[],
    },
    Schema {
        name: "iframe",
        attrs: &[],
    },
    Schema {
        name: "img",
        attrs: &[("usemap", None)],
    },
    Schema {
        name: "img",
        attrs: &[],
    },
    Schema {
        name: "label",
        attrs: &[],
    },
    Schema {
        name: "legend",
        attrs: &[],
    },
    Schema {
        name: "br",
        attrs: &[],
    },
    Schema {
        name: "li",
        attrs: &[],
    },
    Schema {
        name: "ul",
        attrs: &[],
    },
    Schema {
        name: "ol",
        attrs: &[],
    },
    Schema {
        name: "main",
        attrs: &[],
    },
    Schema {
        name: "mark",
        attrs: &[],
    },
    Schema {
        name: "marquee",
        attrs: &[],
    },
    Schema {
        name: "menu",
        attrs: &[],
    },
    Schema {
        name: "meter",
        attrs: &[],
    },
    Schema {
        name: "nav",
        attrs: &[],
    },
    Schema {
        name: "p",
        attrs: &[],
    },
    Schema {
        name: "pre",
        attrs: &[],
    },
    Schema {
        name: "progress",
        attrs: &[],
    },
    Schema {
        name: "tr",
        attrs: &[],
    },
    Schema {
        name: "ruby",
        attrs: &[],
    },
    Schema {
        name: "table",
        attrs: &[],
    },
    Schema {
        name: "time",
        attrs: &[],
    },
];
pub static SEMANTIC_AX: &[(Schema, &[&str])] = &[
    (
        Schema {
            name: "article",
            attrs: &[],
        },
        &["article"],
    ),
    (
        Schema {
            name: "button",
            attrs: &[],
        },
        &["button"],
    ),
    (
        Schema {
            name: "td",
            attrs: &[],
        },
        &["cell", "gridcell"],
    ),
    (
        Schema {
            name: "input",
            attrs: &[("type", Some("checkbox"))],
        },
        &["checkbox", "switch"],
    ),
    (
        Schema {
            name: "th",
            attrs: &[],
        },
        &["columnheader"],
    ),
    (
        Schema {
            name: "select",
            attrs: &[],
        },
        &["combobox", "listbox"],
    ),
    (
        Schema {
            name: "dialog",
            attrs: &[],
        },
        &["dialog"],
    ),
    (
        Schema {
            name: "dir",
            attrs: &[],
        },
        &["directory"],
    ),
    (
        Schema {
            name: "figure",
            attrs: &[],
        },
        &["figure"],
    ),
    (
        Schema {
            name: "form",
            attrs: &[],
        },
        &["form"],
    ),
    (
        Schema {
            name: "h1",
            attrs: &[],
        },
        &["heading"],
    ),
    (
        Schema {
            name: "h2",
            attrs: &[],
        },
        &["heading"],
    ),
    (
        Schema {
            name: "h3",
            attrs: &[],
        },
        &["heading"],
    ),
    (
        Schema {
            name: "h4",
            attrs: &[],
        },
        &["heading"],
    ),
    (
        Schema {
            name: "h5",
            attrs: &[],
        },
        &["heading"],
    ),
    (
        Schema {
            name: "h6",
            attrs: &[],
        },
        &["heading"],
    ),
    (
        Schema {
            name: "img",
            attrs: &[],
        },
        &["img"],
    ),
    (
        Schema {
            name: "input",
            attrs: &[],
        },
        &["textbox"],
    ),
    (
        Schema {
            name: "a",
            attrs: &[("href", None)],
        },
        &["link"],
    ),
    (
        Schema {
            name: "option",
            attrs: &[],
        },
        &["option"],
    ),
    (
        Schema {
            name: "datalist",
            attrs: &[],
        },
        &["listbox"],
    ),
    (
        Schema {
            name: "li",
            attrs: &[],
        },
        &["listitem"],
    ),
    (
        Schema {
            name: "ul",
            attrs: &[],
        },
        &["list"],
    ),
    (
        Schema {
            name: "ol",
            attrs: &[],
        },
        &["list"],
    ),
    (
        Schema {
            name: "main",
            attrs: &[],
        },
        &["main"],
    ),
    (
        Schema {
            name: "marquee",
            attrs: &[],
        },
        &["marquee"],
    ),
    (
        Schema {
            name: "menuitem",
            attrs: &[],
        },
        &["menuitem"],
    ),
    (
        Schema {
            name: "menu",
            attrs: &[],
        },
        &["menu"],
    ),
    (
        Schema {
            name: "nav",
            attrs: &[],
        },
        &["navigation"],
    ),
    (
        Schema {
            name: "progress",
            attrs: &[],
        },
        &["progressbar"],
    ),
    (
        Schema {
            name: "input",
            attrs: &[("type", Some("radio"))],
        },
        &["radio"],
    ),
    (
        Schema {
            name: "th",
            attrs: &[("scope", Some("row"))],
        },
        &["rowheader"],
    ),
    (
        Schema {
            name: "tr",
            attrs: &[],
        },
        &["row"],
    ),
    (
        Schema {
            name: "input",
            attrs: &[("type", Some("search"))],
        },
        &["searchbox"],
    ),
    (
        Schema {
            name: "input",
            attrs: &[("type", Some("range"))],
        },
        &["slider"],
    ),
    (
        Schema {
            name: "input",
            attrs: &[("type", Some("number"))],
        },
        &["spinbutton"],
    ),
    (
        Schema {
            name: "table",
            attrs: &[],
        },
        &["table"],
    ),
    (
        Schema {
            name: "textarea",
            attrs: &[],
        },
        &["textbox"],
    ),
    (
        Schema {
            name: "input",
            attrs: &[("type", Some("text"))],
        },
        &["textbox"],
    ),
];
/// (role, formatted tags, tag names)
pub static ROLE_TAGS: &[(&str, &str, &[&str])] = &[
    ("article", "<article>", &["article"]),
    ("banner", "<header>", &["header"]),
    ("blockquote", "<blockquote>", &["blockquote"]),
    (
        "button",
        "<input type=\"button\">, <input type=\"image\">, <input type=\"reset\">, <input type=\"submit\">, or <button>",
        &["input", "button"],
    ),
    ("caption", "<caption>", &["caption"]),
    ("cell", "<td>", &["td"]),
    ("checkbox", "<input type=\"checkbox\">", &["input"]),
    ("code", "<code>", &["code"]),
    (
        "columnheader",
        "<th>, <th scope=\"col\">, or <th scope=\"colgroup\">",
        &["th"],
    ),
    (
        "combobox",
        "<input list=...>, <input list=...>, <input list=...>, <input list=...>, <input list=...>, or <select multiple=...>",
        &["input", "select"],
    ),
    (
        "complementary",
        "<aside>, <aside aria-label=...>, or <aside aria-labelledby=...>",
        &["aside"],
    ),
    ("contentinfo", "<footer>", &["footer"]),
    ("definition", "<dd>", &["dd"]),
    ("deletion", "<del>", &["del"]),
    ("dialog", "<dialog>", &["dialog"]),
    ("document", "<html>", &["html"]),
    ("emphasis", "<em>", &["em"]),
    ("figure", "<figure>", &["figure"]),
    (
        "form",
        "<form aria-label=...>, <form aria-labelledby=...>, or <form name=...>",
        &["form"],
    ),
    (
        "generic",
        "<a>, <area>, <aside>, <b>, <bdo>, <body>, <data>, <div>, <footer>, <header>, <hgroup>, <i>, <pre>, <q>, <samp>, <section>, <small>, <span>, or <u>",
        &[
            "a", "area", "aside", "b", "bdo", "body", "data", "div", "footer", "header", "hgroup",
            "i", "pre", "q", "samp", "section", "small", "span", "u",
        ],
    ),
    ("gridcell", "<td>", &["td"]),
    (
        "group",
        "<details>, <fieldset>, <optgroup>, or <address>",
        &["details", "fieldset", "optgroup", "address"],
    ),
    (
        "heading",
        "<h1>, <h2>, <h3>, <h4>, <h5>, or <h6>",
        &["h1", "h2", "h3", "h4", "h5", "h6"],
    ),
    ("img", "<img alt=...>, or <img alt=...>", &["img"]),
    ("insertion", "<ins>", &["ins"]),
    ("link", "<a href=...>, or <area href=...>", &["a", "area"]),
    ("list", "<menu>, <ol>, or <ul>", &["menu", "ol", "ul"]),
    (
        "listbox",
        "<select size=...>, <select multiple=...>, or <datalist>",
        &["select", "datalist"],
    ),
    ("listitem", "<li>", &["li"]),
    ("main", "<main>", &["main"]),
    ("mark", "<mark>", &["mark"]),
    ("math", "<math>", &["math"]),
    ("meter", "<meter>", &["meter"]),
    ("navigation", "<nav>", &["nav"]),
    ("option", "<option>", &["option"]),
    ("paragraph", "<p>", &["p"]),
    ("presentation", "<img alt=...>", &["img"]),
    ("progressbar", "<progress>", &["progress"]),
    ("radio", "<input type=\"radio\">", &["input"]),
    (
        "region",
        "<section aria-label=...>, or <section aria-labelledby=...>",
        &["section"],
    ),
    ("row", "<tr>", &["tr"]),
    (
        "rowgroup",
        "<tbody>, <tfoot>, or <thead>",
        &["tbody", "tfoot", "thead"],
    ),
    (
        "rowheader",
        "<th scope=\"row\">, or <th scope=\"rowgroup\">",
        &["th"],
    ),
    ("searchbox", "<input list=...>", &["input"]),
    ("separator", "<hr>", &["hr"]),
    ("slider", "<input type=\"range\">", &["input"]),
    ("spinbutton", "<input type=\"number\">", &["input"]),
    ("status", "<output>", &["output"]),
    ("strong", "<strong>", &["strong"]),
    ("subscript", "<sub>", &["sub"]),
    ("superscript", "<sup>", &["sup"]),
    ("table", "<table>", &["table"]),
    ("term", "<dfn>, or <dt>", &["dfn", "dt"]),
    (
        "textbox",
        "<input type=...>, <input list=...>, <input list=...>, <input list=...>, <input list=...>, or <textarea>",
        &["input", "textarea"],
    ),
    ("time", "<time>", &["time"]),
];

// ---- tables (SonarJS `helpers/table.ts`) -------------------------------------------------------

/// A cell of a computed table grid.
#[derive(Clone)]
pub struct TableCell<'a> {
    pub is_header: bool,
    pub headers: Option<Vec<String>>,
    pub id: Option<String>,
    pub node: &'a JSXElement<'a>,
    /// Identifies the source `<td>`/`<th>` (cells spanning several slots share it).
    pub internal_id: usize,
}

struct InternalCell<'a> {
    cell: TableCell<'a>,
    row_span: i64,
}

const MAX_ROW_SPAN: i64 = 65534;
const MAX_COL_SPAN: i64 = 1000;

/// `Number.parseInt(String(value))`.
fn js_parse_int(s: &str) -> Option<i64> {
    let t = s.trim_start();
    let (neg, digits) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let end = digits
        .bytes()
        .position(|b| !b.is_ascii_digit())
        .unwrap_or(digits.len());
    if end == 0 {
        return None;
    }
    let v: i64 = digits[..end].parse().unwrap_or(i64::MAX);
    Some(if neg { -v } else { v })
}

/// `s.split(/\s+/)`.
fn js_split_whitespace(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_ws = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !in_ws {
                out.push(std::mem::take(&mut cur));
                in_ws = true;
            }
        } else {
            in_ws = false;
            cur.push(c);
        }
    }
    out.push(cur);
    out
}

fn span_attr<'a>(el: &'a JSXOpeningElement<'a>, key: &str) -> Option<i64> {
    match get_prop(el, key) {
        None => Some(1),
        Some(p) => js_parse_int(&literal_value(&p).to_js_string()?),
    }
}

fn cell_of<'a>(e: &'a JSXElement<'a>, internal_id: usize) -> Option<(InternalCell<'a>, i64)> {
    let el = &e.opening_element;
    let col_span = span_attr(el, "colspan")?.clamp(1, MAX_COL_SPAN);
    let row_span = match span_attr(el, "rowspan")? {
        0 => 0,
        v => v.clamp(1, MAX_ROW_SPAN),
    };
    let headers = get_prop(el, "headers").and_then(|p| {
        let v = literal_value(&p);
        if !v.truthy() || v == Val::Dynamic {
            return None;
        }
        let s = v.to_js_string()?.into_owned();
        if s.trim().is_empty() {
            return None;
        }
        Some(js_split_whitespace(&s))
    });
    let id = get_prop(el, "id").map(|p| match literal_value(&p) {
        Val::Dynamic => "null".to_string(),
        v => v.to_js_string().map(|s| s.into_owned()).unwrap_or_default(),
    });
    let is_header = matches!(&el.name, JSXElementName::Identifier(id) if id.name == "th");
    Some((
        InternalCell {
            cell: TableCell {
                is_header,
                headers,
                id,
                node: e,
                internal_id,
            },
            row_span,
        },
        col_span,
    ))
}

fn extract_row<'a>(tr: &'a JSXElement<'a>, next_id: &mut usize) -> Option<Vec<InternalCell<'a>>> {
    let mut row = Vec::new();
    for child in &tr.children {
        match child {
            JSXChild::Text(_) => continue,
            JSXChild::ExpressionContainer(c) if c.expression.as_expression().is_none() => continue,
            JSXChild::Element(e) if matches!(&e.opening_element.name, JSXElementName::Identifier(id) if id.name == "td" || id.name == "th") =>
            {
                let (cell, col_span) = cell_of(e, *next_id)?;
                for _ in 1..col_span {
                    row.push(InternalCell {
                        cell: cell.cell.clone(),
                        row_span: cell.row_span,
                    });
                }
                row.push(cell);
                *next_id += 1;
            }
            _ => return None,
        }
    }
    Some(row)
}

fn extract_rows<'a>(
    children: &'a [JSXChild<'a>],
    next_id: &mut usize,
    depth: u8,
) -> Option<Vec<Vec<InternalCell<'a>>>> {
    let mut rows = Vec::new();
    let mut unknown = false;
    for child in children {
        match child {
            JSXChild::Element(e) => {
                let ty = element_name(&e.opening_element).to_ascii_lowercase();
                match ty.as_str() {
                    "tr" => match extract_row(e, next_id) {
                        Some(r) => rows.push(r),
                        None => unknown = true,
                    },
                    "table" => {}
                    "thead" | "tbody" | "tfoot" if depth < 8 => {
                        match extract_rows(&e.children, next_id, depth + 1) {
                            Some(r) => rows.extend(r),
                            None => unknown = true,
                        }
                    }
                    _ => {
                        let html = matches!(&e.opening_element.name, JSXElementName::Identifier(id) if is_html_tag(id.name.as_str()));
                        if !html || ty == "thead" || ty == "tbody" || ty == "tfoot" {
                            unknown = true;
                        }
                    }
                }
            }
            JSXChild::ExpressionContainer(c) if c.expression.as_expression().is_some() => {
                unknown = true
            }
            JSXChild::Fragment(f) if depth < 8 => {
                match extract_rows(&f.children, next_id, depth + 1) {
                    Some(r) => rows.extend(r),
                    None => unknown = true,
                }
            }
            JSXChild::Spread(_) => unknown = true,
            _ => {}
        }
    }
    if unknown { None } else { Some(rows) }
}

/// The cell grid of a `<table>`, resolving row and column spans; `None` when the structure isn't
/// statically known.
pub fn compute_grid<'a>(table: &'a JSXElement<'a>) -> Option<Vec<Vec<TableCell<'a>>>> {
    let mut next_id = 0;
    let rows = extract_rows(&table.children, &mut next_id, 0)?;
    let Some(first) = rows.first() else {
        return Some(Vec::new());
    };
    let nb_columns = first.len();
    // Each column holds (index of row in `rows`, index in row, remaining row span).
    let mut columns: Vec<Option<(usize, usize, i64)>> = vec![None; nb_columns];
    let mut result = Vec::new();
    let mut row = 0;
    // Guard against pathological spans.
    let mut guard = 0usize;
    while row < rows.len() && guard < 100_000 {
        guard += 1;
        let mut result_row = Vec::new();
        let mut index_in_row = 0;
        let mut used_current_row = false;
        let mut only_max_row_span = true;
        for column in columns.iter_mut() {
            if column.is_none() {
                if index_in_row == rows[row].len() {
                    continue;
                }
                *column = Some((row, index_in_row, rows[row][index_in_row].row_span));
                index_in_row += 1;
                used_current_row = true;
            }
            let Some((r, i, span)) = column.as_mut() else {
                continue;
            };
            result_row.push(rows[*r][*i].cell.clone());
            if *span > 0 {
                only_max_row_span = false;
                *span -= 1;
                if *span == 0 {
                    *column = None;
                }
            }
        }
        if only_max_row_span {
            break;
        }
        result.push(result_row);
        if used_current_row {
            row += 1;
        }
    }
    Some(result)
}
