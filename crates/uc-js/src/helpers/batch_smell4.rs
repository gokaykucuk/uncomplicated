//! Helpers owned by the `smell4` rule batch.
use oxc_ast::{AstKind, ast::*};
use oxc_semantic::{AstNode, SymbolId};
use oxc_span::{GetSpan, Span};

use crate::Ctx;
use crate::helpers::ast::{string_value, strip_parens};

/// Module name of a `require('m')` call.
pub fn require_module<'a>(e: &Expression<'a>) -> Option<&'a str> {
    let Expression::CallExpression(c) = strip_parens(e) else {
        return None;
    };
    require_call_module(c)
}

/// Module name of a `require('m')` call expression.
pub fn require_call_module<'a>(c: &CallExpression<'a>) -> Option<&'a str> {
    let Expression::Identifier(id) = &c.callee else {
        return None;
    };
    if id.name != "require" || c.arguments.len() != 1 {
        return None;
    }
    match &c.arguments[0] {
        Argument::StringLiteral(s) => Some(s.value.as_str()),
        _ => None,
    }
}

/// Static name of a member expression property (`a.b`, `a['b']`).
pub fn member_static_name<'a>(m: &MemberExpression<'a>) -> Option<&'a str> {
    m.static_property_name()
}

/// Object, property name and property span of a static member access (`a.b`, `a['b']`), looking
/// through parentheses.
pub fn static_member<'b, 'a>(e: &'b Expression<'a>) -> Option<(&'b Expression<'a>, &'a str, Span)> {
    match strip_parens(e) {
        Expression::StaticMemberExpression(m) => {
            Some((&m.object, m.property.name.as_str(), m.property.span))
        }
        Expression::ComputedMemberExpression(m) => {
            let name = string_value(&m.expression)?;
            Some((&m.object, name, m.expression.span()))
        }
        _ => None,
    }
}

/// True if `e` is a reference to the unshadowed global `name`.
pub fn is_global_ref(ctx: &Ctx<'_, '_>, e: &Expression<'_>, name: &str) -> bool {
    matches!(strip_parens(e), Expression::Identifier(id) if ctx.is_global(id, name))
}

/// True if `e` is `<global obj>.<prop>` (static access).
pub fn is_global_member(ctx: &Ctx<'_, '_>, e: &Expression<'_>, obj: &str, prop: &str) -> bool {
    matches!(static_member(e), Some((o, p, _)) if p == prop && is_global_ref(ctx, o, obj))
}

/// Unique initializer of a variable that is never reassigned, declared with a plain identifier.
pub fn unique_init<'a>(ctx: &Ctx<'a, '_>, sym: SymbolId) -> Option<&'a Expression<'a>> {
    if ctx.scoping().symbol_is_mutated(sym) || !ctx.scoping().symbol_redeclarations(sym).is_empty()
    {
        return None;
    }
    match ctx.symbol_decl(sym).kind() {
        AstKind::VariableDeclarator(v) if matches!(v.id, BindingPattern::BindingIdentifier(_)) => {
            v.init.as_ref()
        }
        _ => None,
    }
}

const MAX_FQN_DEPTH: u8 = 8;

/// Fully qualified name of an expression resolved through imports and `require` calls, in the
/// spirit of SonarJS' `getFullyQualifiedName`: `_.map` with `import _ from 'lodash'` gives
/// `lodash.map`, `memoize` with `import memoize from 'lodash/memoize'` gives `lodash.memoize`.
/// Globals and unresolvable names give `None`.
pub fn fqn<'a>(ctx: &Ctx<'a, '_>, e: &Expression<'a>) -> Option<String> {
    let mut parts: Vec<&'a str> = Vec::new();
    fqn_rec(ctx, e, &mut parts, 0)
}

fn finish(module: &str, parts: &mut [&str]) -> String {
    parts.reverse();
    let mut out = String::new();
    for (i, seg) in module.split('/').enumerate() {
        if i > 0 {
            out.push('.');
        }
        out.push_str(seg);
    }
    for p in parts.iter() {
        out.push('.');
        out.push_str(p);
    }
    match out.strip_prefix("node:") {
        Some(s) => s.to_string(),
        None => out,
    }
}

fn fqn_rec<'a>(
    ctx: &Ctx<'a, '_>,
    e: &Expression<'a>,
    parts: &mut Vec<&'a str>,
    depth: u8,
) -> Option<String> {
    if depth > MAX_FQN_DEPTH {
        return None;
    }
    let mut cur = e;
    let id = loop {
        match cur {
            Expression::ParenthesizedExpression(p) => cur = &p.expression,
            Expression::TSNonNullExpression(x) => cur = &x.expression,
            Expression::StaticMemberExpression(m) => {
                parts.push(m.property.name.as_str());
                cur = &m.object;
            }
            Expression::ComputedMemberExpression(m) => {
                if let Expression::StringLiteral(s) = &m.expression {
                    parts.push(s.value.as_str());
                }
                cur = &m.object;
            }
            Expression::CallExpression(c) => {
                if let Some(m) = require_call_module(c) {
                    return Some(finish(m, parts));
                }
                cur = &c.callee;
            }
            Expression::NewExpression(n) => cur = &n.callee,
            Expression::TaggedTemplateExpression(t) => cur = &t.tag,
            Expression::ChainExpression(ch) => match &ch.expression {
                ChainElement::CallExpression(c) => {
                    if let Some(m) = require_call_module(c) {
                        return Some(finish(m, parts));
                    }
                    cur = &c.callee;
                }
                ChainElement::StaticMemberExpression(m) => {
                    parts.push(m.property.name.as_str());
                    cur = &m.object;
                }
                ChainElement::ComputedMemberExpression(m) => {
                    if let Expression::StringLiteral(s) = &m.expression {
                        parts.push(s.value.as_str());
                    }
                    cur = &m.object;
                }
                ChainElement::TSNonNullExpression(x) => cur = &x.expression,
                _ => return None,
            },
            Expression::Identifier(id) => break id,
            _ => return None,
        }
    };
    let sym = ctx.symbol_of(id)?;
    fqn_of_symbol(ctx, sym, parts, depth)
}

fn fqn_of_symbol<'a>(
    ctx: &Ctx<'a, '_>,
    sym: SymbolId,
    parts: &mut Vec<&'a str>,
    depth: u8,
) -> Option<String> {
    if !ctx.scoping().symbol_redeclarations(sym).is_empty() {
        return None;
    }
    let decl = ctx.symbol_decl(sym);
    match decl.kind() {
        AstKind::ImportSpecifier(s) => {
            if s.import_kind.is_type() {
                return None;
            }
            let AstKind::ImportDeclaration(imp) = ctx.parent_kind(decl.id()) else {
                return None;
            };
            if imp.import_kind.is_type() {
                return None;
            }
            if let ModuleExportName::IdentifierName(n) = &s.imported
                && n.name != "default"
            {
                parts.push(n.name.as_str());
            }
            Some(finish(imp.source.value.as_str(), parts))
        }
        AstKind::ImportDefaultSpecifier(_) | AstKind::ImportNamespaceSpecifier(_) => {
            let AstKind::ImportDeclaration(imp) = ctx.parent_kind(decl.id()) else {
                return None;
            };
            if imp.import_kind.is_type() {
                return None;
            }
            Some(finish(imp.source.value.as_str(), parts))
        }
        AstKind::VariableDeclarator(v) => {
            if ctx.scoping().symbol_is_mutated(sym) {
                return None;
            }
            let init = v.init.as_ref()?;
            match &v.id {
                BindingPattern::BindingIdentifier(_) => {}
                BindingPattern::ObjectPattern(op) => {
                    let prop = op.properties.iter().find(|p| {
                        matches!(&p.value, BindingPattern::BindingIdentifier(b) if b.symbol_id.get() == Some(sym))
                    })?;
                    match &prop.key {
                        PropertyKey::StaticIdentifier(k) => parts.push(k.name.as_str()),
                        _ => return None,
                    }
                }
                _ => return None,
            }
            fqn_rec(ctx, init, parts, depth + 1)
        }
        _ => None,
    }
}

/// Statements of the list containing statement `node`, if its parent is a statement list.
pub fn sibling_statements<'a>(
    ctx: &Ctx<'a, '_>,
    node: &AstNode<'a>,
) -> Option<&'a [Statement<'a>]> {
    match ctx.parent_kind(node.id()) {
        AstKind::Program(p) => Some(&p.body),
        AstKind::BlockStatement(b) => Some(&b.body),
        AstKind::FunctionBody(b) => Some(&b.statements),
        AstKind::SwitchCase(c) => Some(&c.consequent),
        AstKind::StaticBlock(b) => Some(&b.body),
        AstKind::TSModuleBlock(b) => Some(&b.body),
        _ => None,
    }
}

/// Static name of a property key, as ESLint's `getStaticPropertyName` (identifier when not
/// computed, string / numeric literal, substitution-free template).
pub fn static_key_name<'a>(
    key: &PropertyKey<'a>,
    computed: bool,
) -> Option<std::borrow::Cow<'a, str>> {
    use std::borrow::Cow;
    match key {
        PropertyKey::StaticIdentifier(id) if !computed => Some(Cow::Borrowed(id.name.as_str())),
        PropertyKey::StringLiteral(s) => Some(Cow::Borrowed(s.value.as_str())),
        PropertyKey::NumericLiteral(n) => Some(Cow::Owned(format_number(n.value))),
        PropertyKey::TemplateLiteral(t) if t.expressions.is_empty() && t.quasis.len() == 1 => {
            t.quasis[0].value.cooked.map(|c| Cow::Borrowed(c.as_str()))
        }
        _ => None,
    }
}

fn format_number(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e21 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// ESLint-utils' `getFunctionNameWithKind` for a function or arrow node, e.g.
/// `arrow function 'toBoolean'`, `method 'foo'`, `function`.
pub fn function_name_with_kind(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> String {
    let (is_async, is_generator, is_arrow, id) = match node.kind() {
        AstKind::Function(f) => (
            f.r#async,
            f.generator,
            false,
            f.id.as_ref().map(|i| i.name.as_str()),
        ),
        AstKind::ArrowFunctionExpression(a) => (a.r#async, false, true, None),
        _ => return String::new(),
    };
    let parent = ctx.parent(node.id());
    let mut tokens: Vec<String> = Vec::new();
    enum Owner<'b, 'a> {
        ObjectMethod(&'b ObjectProperty<'a>),
        ClassMethod(&'b MethodDefinition<'a>),
        ClassField(&'b PropertyDefinition<'a>),
        None,
    }
    let owner = match parent.kind() {
        AstKind::ObjectProperty(p) if p.value.span() == node.kind().span() => {
            Owner::ObjectMethod(p)
        }
        AstKind::MethodDefinition(m) => Owner::ClassMethod(m),
        AstKind::PropertyDefinition(p) => Owner::ClassField(p),
        _ => Owner::None,
    };
    match &owner {
        Owner::ClassMethod(m) => {
            if m.r#static {
                tokens.push("static".into());
            }
            if matches!(m.key, PropertyKey::PrivateIdentifier(_)) {
                tokens.push("private".into());
            }
        }
        Owner::ClassField(p) => {
            if p.r#static {
                tokens.push("static".into());
            }
            if matches!(p.key, PropertyKey::PrivateIdentifier(_)) {
                tokens.push("private".into());
            }
        }
        _ => {}
    }
    if is_async {
        tokens.push("async".into());
    }
    if is_generator {
        tokens.push("generator".into());
    }
    match &owner {
        Owner::ObjectMethod(p) => tokens.push(
            match p.kind {
                PropertyKind::Get => "getter",
                PropertyKind::Set => "setter",
                PropertyKind::Init => "method",
            }
            .into(),
        ),
        Owner::ClassMethod(m) => match m.kind {
            MethodDefinitionKind::Constructor => return "constructor".into(),
            MethodDefinitionKind::Get => tokens.push("getter".into()),
            MethodDefinitionKind::Set => tokens.push("setter".into()),
            MethodDefinitionKind::Method => tokens.push("method".into()),
        },
        Owner::ClassField(_) => tokens.push("method".into()),
        Owner::None => {
            if is_arrow {
                tokens.push("arrow".into());
            }
            tokens.push("function".into());
        }
    }
    let key = match &owner {
        Owner::ObjectMethod(p) => Some((&p.key, p.computed)),
        Owner::ClassMethod(m) => Some((&m.key, m.computed)),
        Owner::ClassField(p) => Some((&p.key, p.computed)),
        Owner::None => None,
    };
    if let Some((key, computed)) = key {
        if let PropertyKey::PrivateIdentifier(p) = key {
            tokens.push(format!("#{}", p.name));
        } else if let Some(name) = static_key_name(key, computed) {
            tokens.push(format!("'{name}'"));
        } else {
            let text = ctx.text(key.span());
            if !text.contains('\n') {
                tokens.push(format!("[{text}]"));
            }
        }
    } else if let Some(id) = id {
        tokens.push(format!("'{id}'"));
    } else {
        match parent.kind() {
            AstKind::VariableDeclarator(v) => {
                if let BindingPattern::BindingIdentifier(b) = &v.id {
                    tokens.push(format!("'{}'", b.name));
                }
            }
            AstKind::AssignmentExpression(a) => {
                if let AssignmentTarget::AssignmentTargetIdentifier(i) = &a.left {
                    tokens.push(format!("'{}'", i.name));
                }
            }
            AstKind::AssignmentPattern(a) => {
                if let BindingPattern::BindingIdentifier(b) = &a.left {
                    tokens.push(format!("'{}'", b.name));
                }
            }
            AstKind::FormalParameter(p) if p.initializer.is_some() => {
                if let BindingPattern::BindingIdentifier(b) = &p.pattern {
                    tokens.push(format!("'{}'", b.name));
                }
            }
            AstKind::ExportDefaultDeclaration(_) => tokens.push("'default'".into()),
            _ => {}
        }
    }
    tokens.join(" ")
}

/// Strips parentheses and `as` expressions (eslint-plugin-vue's `skipTSAsExpression`).
fn skip_as<'b, 'a>(mut e: &'b Expression<'a>) -> &'b Expression<'a> {
    loop {
        e = match e {
            Expression::ParenthesizedExpression(p) => &p.expression,
            Expression::TSAsExpression(x) => &x.expression,
            _ => return e,
        }
    }
}

fn last_object_argument<'b, 'a>(args: &'b [Argument<'a>]) -> Option<&'b ObjectExpression<'a>> {
    match skip_as(args.last()?.as_expression()?) {
        Expression::ObjectExpression(o) => Some(o),
        _ => None,
    }
}

/// The options object of a Vue component definition, as recognised by eslint-plugin-vue in script
/// files: `Vue.component('x', {...})`, `Vue.extend({...})`, `app.component('x', {...})`,
/// `app.mixin({...})`, `component('x', {...})`, `createApp({...})`, `defineComponent({...})`,
/// `new Vue({...})` and `export default {...}` in `.jsx` / `.tsx` files.
pub fn vue_component_object<'a>(
    ctx: &Ctx<'a, '_>,
    node: &AstNode<'a>,
) -> Option<&'a ObjectExpression<'a>> {
    match node.kind() {
        AstKind::CallExpression(c) => {
            let obj = last_object_argument(&c.arguments)?;
            match &c.callee {
                Expression::StaticMemberExpression(m) => {
                    let Expression::Identifier(o) = skip_as(&m.object) else {
                        return None;
                    };
                    let p = m.property.name.as_str();
                    let ok = if o.name == "Vue" {
                        matches!(p, "component" | "mixin" | "extend")
                    } else {
                        matches!(p, "component" | "mixin")
                    };
                    ok.then_some(obj)
                }
                Expression::Identifier(id) => matches!(
                    id.name.as_str(),
                    "component" | "createApp" | "defineComponent"
                )
                .then_some(obj),
                _ => None,
            }
        }
        AstKind::NewExpression(n) => {
            let Expression::Identifier(id) = &n.callee else {
                return None;
            };
            if id.name != "Vue" {
                return None;
            }
            match skip_as(n.arguments.first()?.as_expression()?) {
                Expression::ObjectExpression(o) => Some(o),
                _ => None,
            }
        }
        AstKind::ExportDefaultDeclaration(d) => {
            let path = ctx.file.path;
            if !(path.ends_with(".jsx") || path.ends_with(".tsx") || path.ends_with(".vue")) {
                return None;
            }
            match d.declaration.as_expression().map(skip_as) {
                Some(Expression::ObjectExpression(o)) => Some(o),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Value of the property named `name` in an object literal.
pub fn find_property<'b, 'a>(
    obj: &'b ObjectExpression<'a>,
    name: &str,
) -> Option<&'b ObjectProperty<'a>> {
    obj.properties.iter().find_map(|p| match p {
        ObjectPropertyKind::ObjectProperty(p)
            if static_key_name(&p.key, p.computed).as_deref() == Some(name) =>
        {
            Some(&**p)
        }
        _ => None,
    })
}

/// Vue `props` option of a component definition node.
pub fn vue_props<'a>(ctx: &Ctx<'a, '_>, node: &AstNode<'a>) -> Option<&'a Expression<'a>> {
    // `<script setup>`: `defineProps({...})` / `defineProps([...])`.
    if let AstKind::CallExpression(c) = node.kind()
        && matches!(&c.callee, Expression::Identifier(id) if id.name == "defineProps" && ctx.symbol_of(id).is_none())
    {
        if !ctx.file.path.ends_with(".vue") || c.type_arguments.is_some() {
            return None;
        }
        return c.arguments.first()?.as_expression().map(skip_as);
    }
    let obj = vue_component_object(ctx, node)?;
    let props = find_property(obj, "props")?;
    Some(skip_as(&props.value))
}

/// Parses the leading `major[.minor[.patch]]` of a version, `x`/`*` parts counting as 0.
fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let s = s
        .trim()
        .trim_start_matches(['^', '~', '>', '<', '=', 'v', ' ']);
    let core = s.split(['-', '+', ' ']).next()?;
    let mut parts = core.split('.');
    let num = |p: Option<&str>| -> Option<u64> {
        match p {
            None | Some("x" | "X" | "*") => Some(0),
            Some(p) => p.parse().ok(),
        }
    };
    let major = match parts.next()? {
        "x" | "X" | "*" | "" => return None,
        m => m.parse().ok()?,
    };
    Some((major, num(parts.next())?, num(parts.next())?))
}

/// Lowest version satisfying a semver range (`semver.minVersion`), approximately: the lowest
/// lower bound over the `||` alternatives. `None` when unparseable.
pub fn min_version(range: &str) -> Option<(u64, u64, u64)> {
    range
        .split("||")
        .map(|alt| {
            let alt = alt.trim();
            // `1.2.3 - 2.0.0` and `>=1 <2`: the first comparator is the lower bound.
            let first = alt.split_whitespace().next().unwrap_or("");
            let v = parse_version(first)?;
            // `>1.2.3` excludes the version itself.
            Some(if first.starts_with('>') && !first.starts_with(">=") {
                (v.0, v.1, v.2 + 1)
            } else {
                v
            })
        })
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .min()
}

/// Range accepting any version (`*`, `x`, `latest`, empty).
pub fn is_unbounded_range(range: &str) -> bool {
    let r = range.trim();
    r.is_empty() || r == "*" || r.eq_ignore_ascii_case("x") || r.eq_ignore_ascii_case("latest")
}

/// SonarJS' `isVue2OrEarlier` inverted: the project depends on Vue and the declared range admits
/// Vue 3 or later (an unparseable range counts as Vue 3, as in SonarJS).
pub fn is_vue3_project(ctx: &Ctx<'_, '_>) -> bool {
    let Some(range) = ctx.file.project.deps.get("vue") else {
        return false;
    };
    if is_unbounded_range(range) {
        return true;
    }
    range.split("||").any(|alt| {
        let alt = alt.trim();
        let Some(v) = alt.split_whitespace().next().and_then(parse_version) else {
            return true;
        };
        // An open upper bound (`>=2`) reaches Vue 3.
        (alt.starts_with('>') && !alt.contains('<')) || v.0 >= 3
    })
}

/// ECMAScript year of a TypeScript `lib` / `target` name (`es5` → 2009, `es6` → 2015,
/// `es2017.object` → 2017, `esnext` → max).
fn es_year(name: &str) -> Option<u32> {
    let n = name.trim().to_ascii_lowercase();
    if n.starts_with("esnext") {
        return Some(u32::MAX);
    }
    let y = n.strip_prefix("es")?.split('.').next()?;
    match y {
        "3" => Some(1999),
        "5" => Some(2009),
        "6" => Some(2015),
        y => y.parse().ok().filter(|v| *v >= 2015),
    }
}

/// The TypeScript libs of the project (tsconfig `lib`, else `target`) include ES`year`. True
/// without TypeScript configuration.
pub fn ts_lib_at_least(ctx: &Ctx<'_, '_>, year: u32) -> bool {
    let p = ctx.file.project;
    if !p.ts_lib.is_empty() {
        return p
            .ts_lib
            .iter()
            .any(|l| es_year(l).is_some_and(|y| y >= year));
    }
    p.ts_target
        .as_deref()
        .is_none_or(|t| es_year(t).is_none_or(|y| y >= year))
}

/// `-n` where `n` is a positive integer literal: returns `n`.
pub fn negative_integer(e: &Expression<'_>) -> Option<f64> {
    match strip_parens(e) {
        Expression::UnaryExpression(u) if u.operator == UnaryOperator::UnaryNegation => {
            match strip_parens(&u.argument) {
                Expression::NumericLiteral(n) if n.value > 0.0 && n.value.fract() == 0.0 => {
                    Some(n.value)
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// True if the node is an assignment / update / delete target (ESLint's "left-hand side").
pub fn is_write_target(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> bool {
    let span = node.kind().span();
    match ctx.parent_kind(node.id()) {
        AstKind::AssignmentExpression(a) => a.left.span() == span,
        AstKind::UpdateExpression(_)
        | AstKind::ArrayAssignmentTarget(_)
        | AstKind::AssignmentTargetWithDefault(_)
        | AstKind::AssignmentTargetPropertyProperty(_)
        | AstKind::AssignmentTargetRest(_) => true,
        AstKind::UnaryExpression(u) => u.operator == UnaryOperator::Delete,
        AstKind::ForInStatement(f) => f.left.span() == span,
        AstKind::ForOfStatement(f) => f.left.span() == span,
        _ => false,
    }
}

/// SonarJS' `isTestRelatedFile`: `*.{test,spec,cy,e2e,mock}.<ext>` or under `__tests__` /
/// `__mocks__`.
pub fn is_test_related_file(path: &str) -> bool {
    let path = path.replace('\\', "/");
    if path.starts_with("__tests__/")
        || path.starts_with("__mocks__/")
        || path.contains("/__tests__/")
        || path.contains("/__mocks__/")
    {
        return true;
    }
    let name = path.rsplit('/').next().unwrap_or("");
    let mut parts = name.rsplitn(3, '.');
    let (Some(ext), Some(marker), Some(_)) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    matches!(
        ext,
        "js" | "mjs" | "cjs" | "jsx" | "vue" | "ts" | "mts" | "cts" | "tsx"
    ) && matches!(marker, "test" | "spec" | "cy" | "e2e" | "mock")
}

/// ESLint's `isNullOrUndefined`: `null`, `undefined` or `void x`.
pub fn is_null_or_undefined(e: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::NullLiteral(_) => true,
        Expression::Identifier(id) => id.name == "undefined",
        Expression::UnaryExpression(u) => u.operator == UnaryOperator::Void,
        _ => false,
    }
}

/// True if `this_arg` is the `this` the applied function would get anyway.
pub fn is_valid_this_arg(
    ctx: &Ctx<'_, '_>,
    applied: &Expression<'_>,
    this_arg: &Expression<'_>,
) -> bool {
    match strip_parens(applied).as_member_expression() {
        Some(m) => ctx.same_tokens(
            strip_parens(m.object()).span(),
            strip_parens(this_arg).span(),
        ),
        None => is_null_or_undefined(this_arg),
    }
}

/// Test helper: like `test_util::check`, with project context. Each line containing
/// `Noncompliant` expects exactly one issue; `{{message}}` is checked when present.
#[cfg(test)]
pub fn check_project(key: &str, file: &str, project: &crate::ProjectInfo, code: &str) {
    use crate::{FileOptions, JsRules, analyze_in_project};
    let rules = JsRules::new(|m| (m.key == key).then(uc_core::Params::default));
    assert_eq!(rules.len(), 1, "{key} is not implemented");
    let opts = FileOptions {
        nosonar: false,
        cpd: false,
        metrics: false,
    };
    let res = analyze_in_project(file, code, false, &rules, &opts, project);
    let mut actual: Vec<(u32, String)> = res
        .issues
        .iter()
        .map(|i| (i.loc.line, i.message.clone()))
        .collect();
    actual.sort();
    let mut expected: Vec<(u32, Option<String>)> = Vec::new();
    for (i, line) in code.lines().enumerate() {
        if let Some(p) = line.find("Noncompliant") {
            let rest = &line[p..];
            let msg = rest.find("{{").and_then(|s| {
                rest[s + 2..]
                    .find("}}")
                    .map(|e| rest[s + 2..s + 2 + e].to_string())
            });
            expected.push((i as u32 + 1, msg));
        }
    }
    let lines: Vec<u32> = actual.iter().map(|a| a.0).collect();
    let want: Vec<u32> = expected.iter().map(|e| e.0).collect();
    assert_eq!(lines, want, "{key} on {file}: issues {actual:?}");
    for ((_, got), (_, msg)) in actual.iter().zip(&expected) {
        if let Some(m) = msg {
            assert_eq!(got, m);
        }
    }
}

#[cfg(test)]
pub fn project_with(deps: &[(&str, &str)]) -> crate::ProjectInfo {
    let mut p = crate::ProjectInfo::default();
    for (k, v) in deps {
        p.deps.insert(k.to_string(), v.to_string());
    }
    p
}
