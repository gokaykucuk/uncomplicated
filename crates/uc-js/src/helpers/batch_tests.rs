//! Helpers owned by the `tests` rule batch: recognizing test frameworks, test structure
//! (`describe` / `it` / hooks), assertions (`expect(...)`, `assert.*`, chai `should`, Cypress,
//! Testing Library) and resolving imported names to fully qualified names.
//!
//! Sonar's checks rely on package.json dependencies to know which framework a file uses; we only
//! see the file, so "framework is active" is derived from imports/requires and from well-known
//! globals the framework injects (`jest`, `vi`, `cy`, `jasmine`).
use crate::prelude::*;

pub const TEST_FUNCTION_NAMES: &[&str] = &["it", "specify", "test"];
pub const SUITE_FUNCTION_NAMES: &[&str] = &["describe", "context", "suite"];
pub const HOOK_NAMES: &[&str] = &[
    "before",
    "after",
    "beforeAll",
    "afterAll",
    "beforeEach",
    "afterEach",
];
pub const TESTING_LIBRARY_MODULES: &[&str] = &[
    "@testing-library/dom",
    "@testing-library/react",
    "@testing-library/vue",
    "@testing-library/angular",
    "@testing-library/svelte",
];
const NODE_ASSERT_MODULES: &[&str] = &[
    "assert",
    "assert/strict",
    "node:assert",
    "node:assert/strict",
];
const MOCHA_STYLE_FRAMEWORKS: &[&str] = &["bun:test", "jest", "mocha", "test", "vitest"];
pub const SUPPORTED_TEST_FRAMEWORKS: &[&str] = &[
    "bun:test",
    "jest",
    "mocha",
    "node:test",
    "vitest",
    "@playwright/test",
];

// ---- file paths ----------------------------------------------------------------------------

const TEST_EXTENSIONS: &[&str] = &["js", "mjs", "cjs", "jsx", "vue", "ts", "mts", "cts", "tsx"];

fn has_marker_extension(path: &str, markers: &[&str]) -> bool {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let Some((rest, ext)) = file.rsplit_once('.') else {
        return false;
    };
    if !TEST_EXTENSIONS.contains(&ext) {
        return false;
    }
    let Some((_, marker)) = rest.rsplit_once('.') else {
        return false;
    };
    markers.contains(&marker)
}

/// `*.test.js`, `*.spec.ts`, `*.cy.js`, ...
pub fn is_test_file(path: &str) -> bool {
    has_marker_extension(path, &["test", "spec", "cy"])
}

/// Test files, but also `*.e2e.ts`, `*.mock.js` and files under `__tests__` / `__mocks__`.
pub fn is_test_related_file(path: &str) -> bool {
    has_marker_extension(path, &["test", "spec", "cy", "e2e", "mock"])
        || path
            .split(['/', '\\'])
            .rev()
            .skip(1)
            .any(|d| d == "__tests__" || d == "__mocks__")
}

// ---- environment ---------------------------------------------------------------------------

/// What the file tells us about the test frameworks in use.
pub struct TestEnv<'a> {
    pub imports: Vec<&'a str>,
    pub jest: bool,
    pub vitest: bool,
    pub jasmine: bool,
    pub mocha: bool,
    pub cypress: bool,
    pub playwright: bool,
    pub bun: bool,
    pub node_test: bool,
    /// Unresolved global `describe` / `it` / `test` calls exist.
    pub global_structure: bool,
    /// Unresolved global `expect` is referenced.
    pub global_expect: bool,
}

/// True if the file references an undeclared global with this name.
pub fn has_global(ctx: &Ctx<'_, '_>, name: &str) -> bool {
    ctx.scoping()
        .root_unresolved_references()
        .contains_key(&Ident::from(name))
}

impl<'a> TestEnv<'a> {
    pub fn new(ctx: &Ctx<'a, '_>) -> Self {
        let mut imports = Vec::new();
        for stmt in &ctx.nodes().program().body {
            match stmt {
                Statement::ImportDeclaration(d) => imports.push(d.source.value.as_str()),
                Statement::VariableDeclaration(v) => {
                    for d in &v.declarations {
                        if let Some(m) = d.init.as_ref().and_then(require_root) {
                            imports.push(m);
                        }
                    }
                }
                Statement::ExpressionStatement(e) => {
                    if let Some(m) = require_root(&e.expression) {
                        imports.push(m);
                    }
                }
                _ => {}
            }
        }
        let has = |m: &[&str]| imports.iter().any(|i| m.contains(i));
        let jest = has(&["jest", "@jest/globals"]) || has_global(ctx, "jest");
        let vitest = has(&["vitest"]) || has_global(ctx, "vi");
        let jasmine = has(&["jasmine"]) || has_global(ctx, "jasmine");
        let mocha = has(&["mocha"]);
        let cypress = has(&["cypress"]) || has_global(ctx, "cy") || has_global(ctx, "Cypress");
        let playwright = has(&["@playwright/test"]);
        let bun = has(&["bun:test"]);
        let node_test = has(&["node:test"]);
        let global_structure = ["describe", "it", "test", "context", "suite", "specify"]
            .iter()
            .any(|n| has_global(ctx, n));
        let global_expect = has_global(ctx, "expect");
        Self {
            imports,
            jest,
            vitest,
            jasmine,
            mocha,
            cypress,
            playwright,
            bun,
            node_test,
            global_structure,
            global_expect,
        }
    }

    pub fn imports(&self, m: &str) -> bool {
        self.imports.contains(&m)
    }

    pub fn imports_any(&self, ms: &[&str]) -> bool {
        self.imports.iter().any(|i| ms.contains(i))
    }

    pub fn imports_testing_library(&self) -> bool {
        self.imports
            .iter()
            .any(|i| is_testing_library_module(i, true))
    }

    pub fn imports_node_assert(&self) -> bool {
        self.imports_any(NODE_ASSERT_MODULES)
    }

    /// Imports one of the Mocha-style test frameworks Sonar supports, or uses global test
    /// structure (the framework then comes from the project's dependencies).
    pub fn has_supported_framework(&self) -> bool {
        self.imports_any(SUPPORTED_TEST_FRAMEWORKS)
            || self.jest
            || self.vitest
            || self.global_structure
    }

    /// Some test framework or assertion library is in play.
    pub fn has_assertion_source(&self) -> bool {
        self.imports_any(&[
            "chai",
            "sinon",
            "supertest",
            "assert",
            "assert/strict",
            "node:assert",
            "node:assert/strict",
            "uvu/assert",
            "vitest",
            "vitest/node",
            "bun:test",
            "@playwright/test",
            "aws-cdk-lib/assertions",
        ]) || self.jest
            || self.jasmine
            || self.cypress
            || self.global_expect
    }
}

/// Module name of `require('m')`, `require('m').x`, `require('m').x()`.
pub fn require_root<'a>(e: &Expression<'a>) -> Option<&'a str> {
    let mut cur = strip_ts(e);
    loop {
        match cur {
            Expression::CallExpression(c) => {
                if let Some(m) = require_arg(c) {
                    return Some(m);
                }
                cur = strip_ts(&c.callee);
            }
            Expression::StaticMemberExpression(m) => cur = strip_ts(&m.object),
            _ => return None,
        }
    }
}

/// `m` for `require('m')`.
pub fn require_arg<'a>(c: &CallExpression<'a>) -> Option<&'a str> {
    match &c.callee {
        Expression::Identifier(id) if id.name == "require" && c.arguments.len() == 1 => {
            match &c.arguments[0] {
                Argument::StringLiteral(s) => Some(s.value.as_str()),
                _ => None,
            }
        }
        _ => None,
    }
}

pub fn is_testing_library_module(m: &str, allow_subpath: bool) -> bool {
    TESTING_LIBRARY_MODULES.iter().any(|t| {
        m == *t
            || (allow_subpath
                && m.len() > t.len()
                && m.starts_with(t)
                && m.as_bytes()[t.len()] == b'/')
    })
}

// ---- fully qualified names -----------------------------------------------------------------

/// Sonar-style fully qualified name: `import { expect } from 'chai'; expect(x).to` -> `chai.expect.to`.
/// Module paths use `.` instead of `/` and the `node:` prefix is dropped. `None` for globals and
/// locally defined values.
pub fn fqn<'a>(ctx: &Ctx<'a, '_>, e: &Expression<'a>) -> Option<String> {
    let mut quals = Vec::new();
    fqn_rec(ctx, e, &mut quals, 0)
}

fn finish(module: &str, quals: &[&str]) -> String {
    let module = module.strip_prefix("node:").unwrap_or(module);
    let mut s = module.replace('/', ".");
    for q in quals.iter().rev() {
        s.push('.');
        s.push_str(q);
    }
    s
}

fn fqn_rec<'a>(
    ctx: &Ctx<'a, '_>,
    e: &Expression<'a>,
    quals: &mut Vec<&'a str>,
    depth: u8,
) -> Option<String> {
    if depth > 4 {
        return None;
    }
    let mut cur = e;
    let id = loop {
        cur = strip_ts(cur);
        match cur {
            Expression::StaticMemberExpression(m) => {
                quals.push(m.property.name.as_str());
                cur = &m.object;
            }
            Expression::ComputedMemberExpression(m) => match &m.expression {
                Expression::StringLiteral(s) => {
                    quals.push(s.value.as_str());
                    cur = &m.object;
                }
                _ => return None,
            },
            Expression::CallExpression(c) => {
                if let Some(m) = require_arg(c) {
                    return Some(finish(m, quals));
                }
                cur = &c.callee;
            }
            Expression::ChainExpression(ch) => match &ch.expression {
                ChainElement::CallExpression(c) => {
                    if let Some(m) = require_arg(c) {
                        return Some(finish(m, quals));
                    }
                    cur = &c.callee;
                }
                ChainElement::StaticMemberExpression(m) => {
                    quals.push(m.property.name.as_str());
                    cur = &m.object;
                }
                ChainElement::TSNonNullExpression(n) => cur = &n.expression,
                _ => return None,
            },
            Expression::TaggedTemplateExpression(t) => cur = &t.tag,
            Expression::NewExpression(n) => cur = &n.callee,
            Expression::Identifier(id) => break id,
            _ => return None,
        }
    };
    resolve_ident(ctx, id, quals, depth)
}

/// Fully qualified name of an identifier (see [`fqn`]).
pub fn fqn_of_ident<'a>(ctx: &Ctx<'a, '_>, id: &IdentifierReference<'a>) -> Option<String> {
    let mut quals = Vec::new();
    resolve_ident(ctx, id, &mut quals, 0)
}

fn resolve_ident<'a>(
    ctx: &Ctx<'a, '_>,
    id: &IdentifierReference<'a>,
    quals: &mut Vec<&'a str>,
    depth: u8,
) -> Option<String> {
    let sym = ctx.symbol_of(id)?;
    let decl = ctx.symbol_decl(sym);
    match decl.kind() {
        AstKind::ImportSpecifier(s) => {
            if s.import_kind.is_type() {
                return None;
            }
            let d = import_declaration(ctx, decl.id())?;
            if d.import_kind.is_type() {
                return None;
            }
            let name = s.imported.name();
            if name != "default" {
                quals.push(name.as_str());
            }
            Some(finish(d.source.value.as_str(), quals))
        }
        AstKind::ImportDefaultSpecifier(_) | AstKind::ImportNamespaceSpecifier(_) => {
            let d = import_declaration(ctx, decl.id())?;
            if d.import_kind.is_type() {
                return None;
            }
            Some(finish(d.source.value.as_str(), quals))
        }
        AstKind::VariableDeclarator(d) => {
            let init = d.init.as_ref()?;
            if ctx.scoping().symbol_is_mutated(sym) {
                return None;
            }
            match &d.id {
                BindingPattern::BindingIdentifier(_) => {}
                BindingPattern::ObjectPattern(p) => {
                    let prop = p.properties.iter().find(|p| {
                        matches!(&p.value, BindingPattern::BindingIdentifier(b) if b.symbol_id.get() == Some(sym))
                    })?;
                    quals.push(property_key_name(&prop.key)?);
                }
                _ => return None,
            }
            fqn_rec(ctx, init, quals, depth + 1)
        }
        _ => None,
    }
}

fn import_declaration<'a, 's>(ctx: &Ctx<'a, 's>, id: NodeId) -> Option<&'s ImportDeclaration<'a>> {
    ancestors(ctx, id).find_map(|n| match n.kind() {
        AstKind::ImportDeclaration(d) => Some(d),
        _ => None,
    })
}

/// True if the identifier is imported from one of `modules` (any specifier kind).
pub fn imported_from(
    ctx: &Ctx<'_, '_>,
    id: &IdentifierReference<'_>,
    pred: impl Fn(&str) -> bool,
) -> bool {
    let Some(sym) = ctx.symbol_of(id) else {
        return false;
    };
    let decl = ctx.symbol_decl(sym);
    matches!(
        decl.kind(),
        AstKind::ImportSpecifier(_)
            | AstKind::ImportDefaultSpecifier(_)
            | AstKind::ImportNamespaceSpecifier(_)
    ) && import_declaration(ctx, decl.id()).is_some_and(|d| pred(d.source.value.as_str()))
}

/// True for unresolved (global) identifiers.
pub fn is_global_ident(ctx: &Ctx<'_, '_>, id: &IdentifierReference<'_>) -> bool {
    ctx.symbol_of(id).is_none()
}

/// True if `e` is the global identifier `name`.
pub fn is_global_named(ctx: &Ctx<'_, '_>, e: &Expression<'_>, name: &str) -> bool {
    matches!(strip_ts(e), Expression::Identifier(id) if id.name == name && ctx.symbol_of(id).is_none())
}

// ---- functions -----------------------------------------------------------------------------

/// A function expression or an arrow function.
#[derive(Clone, Copy)]
pub enum Func<'b, 'a> {
    Fn(&'b Function<'a>),
    Arrow(&'b ArrowFunctionExpression<'a>),
}

impl<'b, 'a> Func<'b, 'a> {
    pub fn of(e: &'b Expression<'a>) -> Option<Self> {
        match e {
            Expression::FunctionExpression(f) => Some(Func::Fn(f)),
            Expression::ArrowFunctionExpression(a) => Some(Func::Arrow(a)),
            _ => None,
        }
    }

    pub fn of_arg(a: &'b Argument<'a>) -> Option<Self> {
        a.as_expression().and_then(Self::of)
    }

    pub fn is_async(&self) -> bool {
        match self {
            Func::Fn(f) => f.r#async,
            Func::Arrow(a) => a.r#async,
        }
    }

    pub fn is_generator(&self) -> bool {
        matches!(self, Func::Fn(f) if f.generator)
    }

    pub fn is_arrow(&self) -> bool {
        matches!(self, Func::Arrow(_))
    }

    pub fn params(&self) -> &'b FormalParameters<'a> {
        match self {
            Func::Fn(f) => &f.params,
            Func::Arrow(a) => &a.params,
        }
    }

    pub fn param_count(&self) -> usize {
        let p = self.params();
        p.items.len() + usize::from(p.rest.is_some())
    }

    /// First parameter when it's a plain identifier (no default, no pattern).
    pub fn first_ident_param(&self) -> Option<&'b BindingIdentifier<'a>> {
        let p = self.params().items.first()?;
        if p.initializer.is_some() {
            return None;
        }
        match &p.pattern {
            BindingPattern::BindingIdentifier(b) => Some(b),
            _ => None,
        }
    }

    /// Block body statements (`None` for expression-bodied arrows).
    pub fn block(&self) -> Option<&'b FunctionBody<'a>> {
        match self {
            Func::Fn(f) => f.body.as_deref(),
            Func::Arrow(a) => a.body.as_function_body(),
        }
    }

    pub fn expression_body(&self) -> Option<&'b Expression<'a>> {
        match self {
            Func::Fn(_) => None,
            Func::Arrow(a) => a.get_expression(),
        }
    }

    pub fn span(&self) -> Span {
        match self {
            Func::Fn(f) => f.span,
            Func::Arrow(a) => a.span,
        }
    }

    pub fn node_id(&self) -> NodeId {
        match self {
            Func::Fn(f) => f.node_id.get(),
            Func::Arrow(a) => a.node_id.get(),
        }
    }
}

/// First function-expression argument.
pub fn callback_arg<'b, 'a>(call: &'b CallExpression<'a>) -> Option<Func<'b, 'a>> {
    call.arguments.iter().find_map(Func::of_arg)
}

/// Local function a callee identifier refers to (declaration or `const f = () => ...`).
pub fn local_function<'a, 's>(
    ctx: &Ctx<'a, 's>,
    id: &IdentifierReference<'a>,
) -> Option<Func<'s, 'a>> {
    let sym = ctx.symbol_of(id)?;
    let decl = ctx.symbol_decl(sym);
    match decl.kind() {
        AstKind::Function(f) => Some(Func::Fn(f)),
        AstKind::VariableDeclarator(d) => {
            if ctx.scoping().symbol_is_mutated(sym)
                || !matches!(d.id, BindingPattern::BindingIdentifier(_))
            {
                return None;
            }
            Func::of(strip_parens(d.init.as_ref()?))
        }
        _ => None,
    }
}

/// Nodes strictly inside `id`'s subtree, in source order. Relies on node ids being assigned in
/// pre-order: the subtree is the contiguous run of ids whose parent is at or after `id`.
pub fn descendants<'s, 'a>(
    ctx: &Ctx<'a, 's>,
    id: NodeId,
) -> impl Iterator<Item = &'s AstNode<'a>> + 's {
    let nodes = ctx.nodes();
    let start = id.index();
    (start + 1..nodes.len())
        .map(NodeId::new)
        .take_while(move |&k| nodes.parent_id(k).index() >= start)
        .map(move |k| nodes.get_node(k))
}

/// Initializer of a never-reassigned `x = init` variable an identifier refers to.
pub fn unique_init<'a, 's>(
    ctx: &Ctx<'a, 's>,
    id: &IdentifierReference<'a>,
) -> Option<&'s Expression<'a>> {
    let sym = ctx.symbol_of(id)?;
    if ctx.scoping().symbol_is_mutated(sym) {
        return None;
    }
    match ctx.symbol_decl(sym).kind() {
        AstKind::VariableDeclarator(d) if matches!(d.id, BindingPattern::BindingIdentifier(_)) => {
            d.init.as_ref()
        }
        _ => None,
    }
}

/// Follows identifiers to their unique initializer (TS wrappers stripped).
pub fn resolve_value<'a, 's>(ctx: &Ctx<'a, 's>, e: &'s Expression<'a>) -> &'s Expression<'a> {
    let mut cur = strip_ts(e);
    for _ in 0..4 {
        match cur {
            Expression::Identifier(id) => match unique_init(ctx, id) {
                Some(init) => cur = strip_ts(init),
                None => break,
            },
            _ => break,
        }
    }
    cur
}

/// ESTree `Literal`: string, number, boolean, null, regex or bigint literal.
pub fn is_literal(e: &Expression<'_>) -> bool {
    matches!(
        strip_parens(e),
        Expression::StringLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::RegExpLiteral(_)
            | Expression::BigIntLiteral(_)
    )
}

/// Numeric literal, optionally signed: `5`, `-1`, `+3`.
pub fn signed_number(e: &Expression<'_>) -> Option<f64> {
    match strip_parens(e) {
        Expression::NumericLiteral(n) => Some(n.value),
        Expression::UnaryExpression(u) if u.operator == UnaryOperator::UnaryNegation => {
            match &u.argument {
                Expression::NumericLiteral(n) => Some(-n.value),
                _ => None,
            }
        }
        Expression::UnaryExpression(u) if u.operator == UnaryOperator::UnaryPlus => {
            match &u.argument {
                Expression::NumericLiteral(n) => Some(n.value),
                _ => None,
            }
        }
        _ => None,
    }
}

/// True if some ancestor call is a Mocha test construct (by name).
pub fn inside_named_construct(ctx: &Ctx<'_, '_>, id: NodeId, names: &[&str]) -> bool {
    ancestors(ctx, id)
        .any(|n| matches!(n.kind(), AstKind::CallExpression(c) if is_named_construct(c, names)))
}

/// Ancestors of a node, parent first. (`Ctx::ancestors` skips the direct parent.)
pub fn ancestors<'s, 'a>(
    ctx: &Ctx<'a, 's>,
    id: NodeId,
) -> impl Iterator<Item = &'s AstNode<'a>> + 's {
    let nodes = ctx.nodes();
    nodes.ancestor_ids(id).map(move |i| nodes.get_node(i))
}

/// Nearest enclosing function-like node (including declarations).
pub fn enclosing_function_id(ctx: &Ctx<'_, '_>, id: NodeId) -> Option<NodeId> {
    ancestors(ctx, id)
        .find(|n| is_function(&n.kind()))
        .map(|n| n.id())
}

// ---- test structure ------------------------------------------------------------------------

/// `describe.only.each(...)` -> base `describe`, modifiers `[only, each]` (Sonar's
/// `getMochaCalleeParts`): a bare `X.each` / `X.each(table)` factory is not a construct.
pub struct CalleeParts<'b, 'a> {
    pub base: &'b IdentifierReference<'a>,
    pub modifiers: Vec<&'a str>,
}

pub fn callee_parts<'b, 'a>(callee: &'b Expression<'a>) -> Option<CalleeParts<'b, 'a>> {
    let mut modifiers = Vec::new();
    let mut cur = callee;
    let mut applied = false;
    loop {
        match cur {
            Expression::CallExpression(c) => {
                cur = &c.callee;
                applied = true;
            }
            Expression::StaticMemberExpression(m) => {
                if m.property.name == "each" && !applied {
                    return None;
                }
                modifiers.push(m.property.name.as_str());
                cur = &m.object;
                applied = false;
            }
            Expression::Identifier(id) => {
                modifiers.reverse();
                return Some(CalleeParts {
                    base: id,
                    modifiers,
                });
            }
            _ => return None,
        }
    }
}

/// Name of the Mocha-style construct the base identifier denotes: a global name, or a name imported
/// from a supported framework (`import { it } from 'vitest'`). Locally defined names don't count.
/// Handles `import test from 'node:test'; test.describe(...)`.
pub fn construct_and_modifiers<'a>(
    ctx: &Ctx<'a, '_>,
    parts: &CalleeParts<'_, 'a>,
) -> Option<(String, Vec<&'a str>)> {
    let base = parts.base;
    let Some(sym) = ctx.symbol_of(base) else {
        return Some((base.name.to_string(), parts.modifiers.clone()));
    };
    let decl = ctx.symbol_decl(sym);
    let from_framework = match decl.kind() {
        AstKind::ImportSpecifier(_)
        | AstKind::ImportDefaultSpecifier(_)
        | AstKind::ImportNamespaceSpecifier(_) => true,
        AstKind::VariableDeclarator(d) => d
            .init
            .as_ref()
            .and_then(require_root)
            .is_some_and(|m| SUPPORTED_TEST_FRAMEWORKS.contains(&m)),
        _ => false,
    };
    if !from_framework {
        return None;
    }
    let f = fqn_of_ident(ctx, base)?;
    let mut it = f.split('.');
    let (Some(framework), Some(name), None) = (it.next(), it.next(), it.next()) else {
        // `import test from 'node:test'` -> fqn `test`
        if f == "test" && is_node_test_default(ctx, base) {
            if let Some((first, rest)) = parts.modifiers.split_first()
                && (TEST_FUNCTION_NAMES.contains(first) || SUITE_FUNCTION_NAMES.contains(first))
            {
                return Some((first.to_string(), rest.to_vec()));
            }
            return Some(("test".to_string(), parts.modifiers.clone()));
        }
        return None;
    };
    MOCHA_STYLE_FRAMEWORKS
        .contains(&framework)
        .then(|| (name.to_string(), parts.modifiers.clone()))
}

fn is_node_test_default(ctx: &Ctx<'_, '_>, id: &IdentifierReference<'_>) -> bool {
    let Some(sym) = ctx.symbol_of(id) else {
        return false;
    };
    let decl = ctx.symbol_decl(sym);
    match decl.kind() {
        AstKind::ImportDefaultSpecifier(_) => {
            import_declaration(ctx, decl.id()).is_some_and(|d| d.source.value == "node:test")
        }
        AstKind::VariableDeclarator(d) => {
            matches!(d.id, BindingPattern::BindingIdentifier(_))
                && d.init.as_ref().and_then(require_root) == Some("node:test")
        }
        _ => false,
    }
}

/// Modifiers that keep a Mocha-style construct runnable (`it.only`, `test.concurrent`, ...).
pub fn is_concrete_modifier(env: &TestEnv<'_>, m: &str) -> bool {
    matches!(m, "only" | "concurrent")
        || (m == "failing" && env.jest)
        || (m == "sequential" && env.vitest)
}

/// Sonar's `isMochaTestConstruct`: a supported framework construct among `names` whose modifiers
/// are all concrete (`each` too when `allow_parameterized`).
pub fn is_mocha_construct<'a>(
    ctx: &Ctx<'a, '_>,
    env: &TestEnv<'_>,
    call: &CallExpression<'a>,
    names: &[&str],
    allow_parameterized: bool,
) -> bool {
    let Some(parts) = callee_parts(&call.callee) else {
        return false;
    };
    let Some((name, modifiers)) = construct_and_modifiers(ctx, &parts) else {
        return false;
    };
    names.contains(&name.as_str())
        && modifiers
            .iter()
            .all(|m| (allow_parameterized && *m == "each") || is_concrete_modifier(env, m))
}

/// Classification of a Mocha-style or Playwright structural call.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Structure {
    ConcreteSuite,
    IgnoredSuite,
    ConcreteTest,
    /// `it.skip`, `test.todo`, ... or an unsupported modifier.
    OtherTest,
}

/// Classifies `describe`/`it`-like calls (Mocha-style frameworks) and Playwright `test` /
/// `test.describe` calls.
pub fn classify_structure<'a>(
    ctx: &Ctx<'a, '_>,
    env: &TestEnv<'_>,
    call: &CallExpression<'a>,
) -> Option<Structure> {
    if let Some(parts) = callee_parts(&call.callee)
        && let Some((name, modifiers)) = construct_and_modifiers(ctx, &parts)
    {
        let concrete = modifiers.iter().all(|m| is_concrete_modifier(env, m));
        if SUITE_FUNCTION_NAMES.contains(&name.as_str()) {
            return Some(if concrete {
                Structure::ConcreteSuite
            } else {
                Structure::IgnoredSuite
            });
        }
        if TEST_FUNCTION_NAMES.contains(&name.as_str()) {
            return Some(if concrete {
                Structure::ConcreteTest
            } else {
                Structure::OtherTest
            });
        }
    }
    let pw_test = playwright_test_qualifiers(ctx, &call.callee);
    let quals = pw_test
        .clone()
        .or_else(|| playwright_describe_qualifiers(&call.callee))?;
    if quals.first() == Some(&"describe") {
        let modifiers = &quals[1..];
        if modifiers.iter().any(|m| matches!(*m, "skip" | "fixme")) {
            return Some(Structure::IgnoredSuite);
        }
        let runnable = if modifiers.last() == Some(&"only") {
            &modifiers[..modifiers.len() - 1]
        } else {
            modifiers
        };
        return runnable
            .iter()
            .all(|m| matches!(*m, "parallel" | "serial"))
            .then_some(Structure::ConcreteSuite);
    }
    let quals = pw_test?;
    let concrete = quals.iter().all(|q| matches!(*q, "only" | "fail"))
        && (!quals.contains(&"fail") || callback_arg(call).is_some());
    Some(if concrete {
        Structure::ConcreteTest
    } else {
        Structure::OtherTest
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ParamFramework {
    Bun,
    Jest,
    Vitest,
}

/// Dataset of a supported parameterized declaration `test.each(dataset)(title, fn)` (Jest,
/// Vitest, Bun), with a callback that is an inline or local function.
pub fn parameterized_dataset<'b, 'a>(
    ctx: &Ctx<'a, '_>,
    env: &TestEnv<'_>,
    call: &'b CallExpression<'a>,
) -> Option<&'b Expression<'a>> {
    let Expression::CallExpression(each_call) = &call.callee else {
        return None;
    };
    let Expression::StaticMemberExpression(each) = &each_call.callee else {
        return None;
    };
    if each.property.name != "each" || call.arguments.len() < 2 {
        return None;
    }
    let dataset = each_call.arguments.first()?.as_expression()?;
    // callback: inline function, or a local function passed by name
    let has_callback = callback_arg(call).is_some()
        || matches!(&call.arguments[1], Argument::Identifier(id) if local_function(ctx, id).is_some());
    if !has_callback {
        return None;
    }
    let parts = callee_parts(&call.callee)?;
    if parts.modifiers.last() != Some(&"each") {
        return None;
    }
    let active: Vec<ParamFramework> = [
        (env.bun, ParamFramework::Bun),
        (env.jest, ParamFramework::Jest),
        (env.vitest, ParamFramework::Vitest),
    ]
    .into_iter()
    .filter_map(|(a, f)| a.then_some(f))
    .collect();
    let base_fqn = fqn_of_ident(ctx, parts.base);
    let (name, framework) = match base_fqn.as_deref() {
        Some(f) => {
            let (fw, name) = if let Some(n) = f.strip_prefix("bun:test.") {
                (ParamFramework::Bun, n)
            } else if let Some(n) = f.strip_prefix("@jest.globals.") {
                (ParamFramework::Jest, n)
            } else if let Some(n) = f.strip_prefix("jest.") {
                (ParamFramework::Jest, n)
            } else {
                (ParamFramework::Vitest, f.strip_prefix("vitest.")?)
            };
            (name.to_string(), fw)
        }
        None => {
            if ctx.symbol_of(parts.base).is_some() {
                return None;
            }
            let fw = if parts.base.name == "suite" {
                env.vitest.then_some(ParamFramework::Vitest)?
            } else if active.len() == 1 {
                active[0]
            } else {
                return None;
            };
            (parts.base.name.to_string(), fw)
        }
    };
    let is_test = matches!(name.as_str(), "test" | "it");
    let is_suite = matches!(name.as_str(), "describe" | "suite");
    if !(is_test || is_suite)
        || (name == "suite" && framework != ParamFramework::Vitest)
        || (framework == ParamFramework::Bun && !matches!(name.as_str(), "test" | "describe"))
    {
        return None;
    }
    let modifiers = &parts.modifiers[..parts.modifiers.len() - 1];
    let ok = modifiers.iter().all(|m| {
        if matches!(*m, "only" | "concurrent") {
            return framework != ParamFramework::Bun || *m == "only";
        }
        if !is_test {
            return framework == ParamFramework::Vitest && *m == "sequential";
        }
        (framework == ParamFramework::Jest && *m == "failing")
            || (framework == ParamFramework::Vitest && matches!(*m, "fails" | "sequential"))
    });
    ok.then_some(dataset)
}

/// Sonar's name-based `isTestConstruct` (helpers/testing/mocha.ts): `name(...)`,
/// `name.only(...)`, `name.skip(...)`.
pub fn is_named_construct(call: &CallExpression<'_>, names: &[&str]) -> bool {
    match &call.callee {
        Expression::Identifier(id) => names.contains(&id.name.as_str()),
        Expression::StaticMemberExpression(m) => {
            matches!(&m.object, Expression::Identifier(id) if names.contains(&id.name.as_str()))
                && matches!(m.property.name.as_str(), "only" | "skip")
        }
        _ => false,
    }
}

pub const MOCHA_TEST_CONSTRUCTS: &[&str] = &[
    "describe",
    "context",
    "it",
    "specify",
    "before",
    "after",
    "beforeAll",
    "afterAll",
    "beforeEach",
    "afterEach",
];
pub const MOCHA_TEST_CASES: &[&str] = &["it", "specify", "test"];

/// Test case `it(title, fn)` with the callback as second argument (Sonar's `extractTestCase`).
pub fn test_case_callback<'b, 'a>(call: &'b CallExpression<'a>) -> Option<Func<'b, 'a>> {
    if !is_named_construct(call, MOCHA_TEST_CASES) {
        return None;
    }
    call.arguments.get(1).and_then(Func::of_arg)
}

/// Static title of a test: string literal or template literal without expressions.
pub fn static_title<'a>(arg: &Argument<'a>) -> Option<&'a str> {
    match arg {
        Argument::StringLiteral(s) => Some(s.value.as_str()),
        Argument::TemplateLiteral(t) if t.expressions.is_empty() && t.quasis.len() == 1 => {
            let v = &t.quasis[0].value;
            Some(v.cooked.as_ref().map_or(v.raw.as_str(), |c| c.as_str()))
        }
        _ => None,
    }
}

/// Qualifiers of a static member chain rooted at an identifier: `test.describe.only` -> (`test`,
/// [describe, only]).
pub fn member_chain<'b, 'a>(
    e: &'b Expression<'a>,
) -> Option<(&'b IdentifierReference<'a>, Vec<&'a str>)> {
    let mut quals = Vec::new();
    let mut cur = e;
    loop {
        match cur {
            Expression::StaticMemberExpression(m) => {
                quals.push(m.property.name.as_str());
                cur = &m.object;
            }
            Expression::Identifier(id) => {
                quals.reverse();
                return Some((id, quals));
            }
            _ => return None,
        }
    }
}

/// Qualifiers after `test` when `test` is imported from `@playwright/test`.
pub fn playwright_test_qualifiers<'a>(
    ctx: &Ctx<'a, '_>,
    callee: &Expression<'a>,
) -> Option<Vec<&'a str>> {
    let (base, quals) = member_chain(callee)?;
    (fqn_of_ident(ctx, base).as_deref() == Some("@playwright.test.test")).then_some(quals)
}

/// Qualifiers after any identifier named `test` (Playwright extended fixtures).
pub fn playwright_describe_qualifiers<'a>(callee: &Expression<'a>) -> Option<Vec<&'a str>> {
    let (base, quals) = member_chain(callee)?;
    (base.name == "test").then_some(quals)
}

/// `test.describe(...)`, `test.describe.parallel(...)`, `test.describe.serial.only(...)`.
pub fn is_playwright_describe_member(e: &Expression<'_>) -> bool {
    match e {
        Expression::StaticMemberExpression(m) => match m.property.name.as_str() {
            "describe" => matches!(&m.object, Expression::Identifier(id) if id.name == "test"),
            "parallel" | "serial" => is_playwright_describe_member(&m.object),
            _ => false,
        },
        _ => false,
    }
}

// ---- call chains ---------------------------------------------------------------------------

/// Member names of a call/member chain, furthest from the root first: `expect(x).not.toBe(y)` ->
/// [toBe, not]. The flag is false when a computed member interrupts the chain.
pub fn call_chain<'a>(e: &Expression<'a>) -> (Vec<(&'a str, Span)>, bool) {
    let mut segs = Vec::new();
    let mut cur = e;
    loop {
        match cur {
            Expression::CallExpression(c) => cur = &c.callee,
            Expression::StaticMemberExpression(m) => {
                segs.push((m.property.name.as_str(), m.property.span));
                cur = &m.object;
            }
            Expression::ComputedMemberExpression(_) | Expression::PrivateFieldExpression(_) => {
                return (segs, false);
            }
            _ => return (segs, true),
        }
    }
}

/// Root call of a member/call chain: `expect(x).not.toBe(y)` -> `expect(x)`.
pub fn root_call<'b, 'a>(call: &'b CallExpression<'a>) -> &'b CallExpression<'a> {
    let mut current = call;
    loop {
        let mut obj = match &current.callee {
            Expression::StaticMemberExpression(m) => &m.object,
            Expression::ComputedMemberExpression(m) => &m.object,
            _ => return current,
        };
        loop {
            match obj {
                Expression::StaticMemberExpression(m) => obj = &m.object,
                Expression::ComputedMemberExpression(m) => obj = &m.object,
                Expression::CallExpression(c) => {
                    current = c;
                    break;
                }
                _ => return current,
            }
        }
    }
}

/// Root of a member/call chain (identifier, `this`, literal, ...).
pub fn chain_base<'b, 'a>(e: &'b Expression<'a>) -> &'b Expression<'a> {
    let mut cur = strip_ts(e);
    loop {
        cur = match cur {
            Expression::CallExpression(c) => strip_ts(&c.callee),
            Expression::StaticMemberExpression(m) => strip_ts(&m.object),
            Expression::ComputedMemberExpression(m) => strip_ts(&m.object),
            Expression::ChainExpression(ch) => match &ch.expression {
                ChainElement::CallExpression(c) => strip_ts(&c.callee),
                ChainElement::StaticMemberExpression(m) => strip_ts(&m.object),
                ChainElement::ComputedMemberExpression(m) => strip_ts(&m.object),
                ChainElement::TSNonNullExpression(n) => strip_ts(&n.expression),
                _ => return cur,
            },
            _ => return cur,
        }
    }
}

/// Cypress chain rooted at `cy`: `cy.get('a').find('b')`.
pub fn chain_starts_with_cy(e: &Expression<'_>) -> bool {
    matches!(chain_base(e), Expression::Identifier(id) if id.name == "cy")
}

/// `expect(...)` (by name), the inner call of an expect chain.
pub fn is_call_named<'a>(e: &Expression<'a>, name: &str) -> Option<&'a str> {
    match e {
        Expression::CallExpression(c) => match &c.callee {
            Expression::Identifier(id) if id.name == name => Some(id.name.as_str()),
            _ => None,
        },
        _ => None,
    }
}

// ---- assertions ----------------------------------------------------------------------------

/// Whether an assertion library can run without a test runner.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Exec {
    ScriptCapable,
    RunnerBound,
}

const GLOBAL_EXPECT_NAMES: &[&str] = &[
    "expect",
    "expectAsync",
    "expectObservable",
    "expectSubscriptions",
    "expectTypeOf",
];
const NON_ASSERTION_EXPECT_MEMBERS: &[&str] = &[
    "extend",
    "addEqualityTesters",
    "addSnapshotSerializer",
    "getState",
    "setState",
    "assertions",
    "hasAssertions",
];

/// Sonar's assertion catalog (chai, sinon, supertest, node assert, vitest/bun, Cypress, global
/// `expect(...).matcher()` chains), for a call or a `.should` member access.
pub fn assertion_exec<'a>(
    ctx: &Ctx<'a, '_>,
    env: &TestEnv<'_>,
    kind: &AstKind<'a>,
) -> Option<Exec> {
    match kind {
        AstKind::StaticMemberExpression(m) if m.property.name == "should" => {
            Some(Exec::ScriptCapable)
        }
        AstKind::CallExpression(c) => call_assertion_exec(ctx, env, c),
        _ => None,
    }
}

fn call_assertion_exec<'a>(
    ctx: &Ctx<'a, '_>,
    env: &TestEnv<'_>,
    c: &CallExpression<'a>,
) -> Option<Exec> {
    if let Some(f) = fqn(ctx, &c.callee) {
        let names: Vec<&str> = f.split('.').collect();
        // chai
        if (names.len() >= 2 && names[0] == "chai" && names[1] == "assert") || f == "chai.expect" {
            return Some(Exec::ScriptCapable);
        }
        // sinon
        if names.len() == 3 && names[0] == "sinon" && names[1] == "assert" {
            return Some(Exec::ScriptCapable);
        }
        // supertest
        if names.len() >= 3 && names[0] == "supertest" && names.last() == Some(&"expect") {
            return Some(Exec::ScriptCapable);
        }
        // node assert
        if names[0] == "assert" && env.imports_node_assert() {
            return Some(Exec::ScriptCapable);
        }
        // vitest / bun
        if is_vitest_assertion_fqn(&f) {
            return Some(Exec::RunnerBound);
        }
    }
    // Cypress `.should()` / `.and()`
    if let Expression::StaticMemberExpression(m) = &c.callee
        && matches!(m.property.name.as_str(), "should" | "and")
        && chain_starts_with_cy(&m.object)
    {
        return Some(Exec::RunnerBound);
    }
    // global expect chain
    if is_global_expect_assertion(ctx, c) {
        return Some(Exec::RunnerBound);
    }
    None
}

fn is_vitest_assertion_fqn(f: &str) -> bool {
    let roots = [
        "bun:test.expect",
        "vitest.expect",
        "vitest.expectTypeOf",
        "vitest.assertType",
    ];
    if !roots
        .iter()
        .any(|r| f == *r || (f.starts_with(r) && f.as_bytes()[r.len()] == b'.'))
    {
        return false;
    }
    let mut it = f.split('.');
    let (_, ns, member) = (it.next(), it.next(), it.next());
    !(ns == Some("expect") && member.is_some_and(|m| NON_ASSERTION_EXPECT_MEMBERS.contains(&m)))
}

/// `expect(x).toBe(y)`, `expect(x).not.toBe(y)`, `expectObservable(x).toBe(y)`, or a Playwright
/// `expect(...)` chain.
pub fn is_global_expect_assertion<'a>(ctx: &Ctx<'a, '_>, c: &CallExpression<'a>) -> bool {
    let Expression::StaticMemberExpression(m) = &c.callee else {
        if let Expression::ComputedMemberExpression(_) = &c.callee {
            return false;
        }
        return false;
    };
    let mut cur = &m.object;
    while let Expression::StaticMemberExpression(m) = cur {
        cur = &m.object;
    }
    let Expression::CallExpression(inner) = cur else {
        return false;
    };
    match &inner.callee {
        Expression::Identifier(id) if GLOBAL_EXPECT_NAMES.contains(&id.name.as_str()) => true,
        callee => fqn(ctx, callee).is_some_and(|f| is_playwright_expect_fqn(&f)),
    }
}

pub fn is_playwright_expect_fqn(f: &str) -> bool {
    ["@playwright.test.test.expect", "@playwright.test.expect"]
        .iter()
        .any(|r| f == *r || (f.starts_with(r) && f.as_bytes()[r.len()] == b'.'))
}

/// Type-level vitest checks (`expectTypeOf`, `assertType`) never run.
pub fn is_type_level_assertion<'a>(ctx: &Ctx<'a, '_>, c: &CallExpression<'a>) -> bool {
    fqn(ctx, &c.callee).is_some_and(|f| {
        [
            "vitest.expectTypeOf",
            "vitest.assertType",
            "expect-type.expectTypeOf",
        ]
        .iter()
        .any(|r| f == *r || (f.starts_with(r) && f.as_bytes()[r.len()] == b'.'))
    })
}

const CHAI_NON_TERMINAL: &[&str] = &[
    "all", "also", "and", "any", "at", "be", "been", "but", "deep", "does", "have", "has", "is",
    "itself", "nested", "not", "of", "ordered", "own", "same", "still", "that", "to", "which",
    "with",
];
const CHAI_TERMINAL: &[&str] = &[
    "Arguments",
    "NaN",
    "arguments",
    "empty",
    "exist",
    "extensible",
    "false",
    "finite",
    "frozen",
    "null",
    "ok",
    "sealed",
    "true",
    "undefined",
];

/// `x.should` that never reaches a call or a terminal chai property (`x.should.be`).
pub fn is_incomplete_should(ctx: &Ctx<'_, '_>, member_id: NodeId) -> bool {
    let mut cur = member_id;
    loop {
        let parent = ctx.parent(cur);
        match parent.kind() {
            AstKind::CallExpression(c) if c.callee.span() == ctx.node(cur).kind().span() => {
                return false;
            }
            AstKind::StaticMemberExpression(m) => {
                let name = m.property.name.as_str();
                let grand = ctx.parent(parent.id());
                let continues = matches!(grand.kind(), AstKind::StaticMemberExpression(_));
                let called =
                    matches!(grand.kind(), AstKind::CallExpression(c) if c.callee.span() == m.span);
                if called {
                    return false;
                }
                if CHAI_TERMINAL.contains(&name) && !continues {
                    return false;
                }
                if !CHAI_NON_TERMINAL.contains(&name)
                    && !CHAI_TERMINAL.contains(&name)
                    && !continues
                {
                    return true;
                }
                cur = parent.id();
            }
            _ => return true,
        }
    }
}

// ---- assertion structure -------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style {
    JestLike,
    Jasmine,
    Playwright,
    ChaiBdd,
    ChaiAssert,
    NodeAssert,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Predicate {
    Truthy,
    Falsy,
    True,
    False,
    Defined,
    Undefined,
    Null,
    Exists,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Comparison {
    Strict,
    Deep,
    Loose,
}

#[derive(Clone, Copy)]
pub enum AssertionKind<'b, 'a> {
    Predicate(Predicate),
    Comparison(Comparison, &'b Expression<'a>),
}

/// Cross-framework view of an assertion (Sonar's `extractTestAssertion`).
#[derive(Clone, Copy)]
pub struct Assertion<'b, 'a> {
    pub style: Style,
    pub kind: AssertionKind<'b, 'a>,
    pub actual: &'b Expression<'a>,
    pub negated: bool,
    pub report: Span,
}

/// Which assertion styles the file can use.
pub struct AssertionStyles {
    jest_like: bool,
    jasmine: bool,
    playwright: bool,
    chai: bool,
    node_assert: bool,
}

impl AssertionStyles {
    pub fn new(env: &TestEnv<'_>) -> Self {
        let chai = env.imports_any(&[
            "chai",
            "chai/register-assert",
            "chai/register-expect",
            "chai/register-should",
            "cypress",
        ]) || env.cypress;
        let jest_like = env.jest
            || env.vitest
            || env.bun
            || (env.global_expect && !env.jasmine && !chai && !env.playwright && !env.mocha);
        Self {
            jest_like,
            jasmine: env.jasmine,
            playwright: env.playwright,
            chai,
            node_assert: env.imports_node_assert(),
        }
    }

    pub fn jest_like(&self) -> bool {
        self.jest_like
    }

    pub fn any(&self) -> bool {
        self.jest_like || self.jasmine || self.playwright || self.chai || self.node_assert
    }
}

fn arg_expr<'b, 'a>(args: &'b [Argument<'a>], i: usize) -> Option<&'b Expression<'a>> {
    args.get(i).and_then(Argument::as_expression)
}

pub fn extract_assertion<'b, 'a>(
    ctx: &Ctx<'a, '_>,
    styles: &AssertionStyles,
    call: &'b CallExpression<'a>,
) -> Option<Assertion<'b, 'a>> {
    let expect_style = if styles.jest_like {
        Some(Style::JestLike)
    } else if styles.jasmine {
        Some(Style::Jasmine)
    } else if styles.playwright {
        Some(Style::Playwright)
    } else {
        None
    };
    if let Some(style) = expect_style
        && let Some(a) = extract_expect(call, style)
    {
        return Some(a);
    }
    if styles.chai
        && let Some(a) = extract_chai(ctx, call)
    {
        return Some(a);
    }
    if styles.node_assert {
        return extract_node_assert(ctx, call);
    }
    None
}

fn extract_expect<'b, 'a>(call: &'b CallExpression<'a>, style: Style) -> Option<Assertion<'b, 'a>> {
    let Expression::StaticMemberExpression(m) = &call.callee else {
        return None;
    };
    let matcher = m.property.name.as_str();
    let mut cur = &m.object;
    let mut negated = false;
    if let Expression::StaticMemberExpression(n) = cur {
        if n.property.name != "not" {
            return None;
        }
        negated = true;
        cur = &n.object;
    }
    let Expression::CallExpression(e) = cur else {
        return None;
    };
    if e.arguments.len() != 1
        || !matches!(&e.callee, Expression::Identifier(id) if id.name == "expect")
    {
        return None;
    }
    let actual = arg_expr(&e.arguments, 0)?;
    let predicate = match matcher {
        "toBeTruthy" => Some(Predicate::Truthy),
        "toBeFalsy" => Some(Predicate::Falsy),
        "toBeDefined" => Some(Predicate::Defined),
        "toBeUndefined" => Some(Predicate::Undefined),
        "toBeNull" => Some(Predicate::Null),
        _ => None,
    };
    if let Some(p) = predicate
        && call.arguments.is_empty()
    {
        return Some(Assertion {
            style,
            kind: AssertionKind::Predicate(p),
            actual,
            negated,
            report: actual.span(),
        });
    }
    let comparison = match matcher {
        "toBe" => Comparison::Strict,
        "toEqual" | "toStrictEqual" => Comparison::Deep,
        _ => return None,
    };
    if call.arguments.len() != 1 {
        return None;
    }
    let expected = arg_expr(&call.arguments, 0)?;
    Some(Assertion {
        style,
        kind: AssertionKind::Comparison(comparison, expected),
        actual,
        negated,
        report: m.property.span,
    })
}

/// Chai `expect(x).to...` chains and `assert.*` calls (common forms only).
fn extract_chai<'b, 'a>(
    ctx: &Ctx<'a, '_>,
    call: &'b CallExpression<'a>,
) -> Option<Assertion<'b, 'a>> {
    let Expression::StaticMemberExpression(m) = &call.callee else {
        return None;
    };
    let name = m.property.name.as_str();
    // assert.xxx(...)
    if let Expression::Identifier(obj) = &m.object
        && obj.name == "assert"
        && (ctx.symbol_of(obj).is_none()
            || fqn_of_ident(ctx, obj).is_some_and(|f| f.starts_with("chai")))
    {
        let actual = arg_expr(&call.arguments, 0)?;
        let pred = |p: Predicate, negated: bool| {
            Some(Assertion {
                style: Style::ChaiAssert,
                kind: AssertionKind::Predicate(p),
                actual,
                negated,
                report: actual.span(),
            })
        };
        let cmp = |c: Comparison, negated: bool| {
            let expected = arg_expr(&call.arguments, 1)?;
            Some(Assertion {
                style: Style::ChaiAssert,
                kind: AssertionKind::Comparison(c, expected),
                actual,
                negated,
                report: m.property.span,
            })
        };
        return match name {
            "isTrue" => pred(Predicate::True, false),
            "isFalse" => pred(Predicate::False, false),
            "isNull" => pred(Predicate::Null, false),
            "isNotNull" => pred(Predicate::Null, true),
            "isUndefined" => pred(Predicate::Undefined, false),
            "isDefined" => pred(Predicate::Defined, false),
            "isOk" | "ok" => pred(Predicate::Truthy, false),
            "isNotOk" | "notOk" => pred(Predicate::Falsy, false),
            "exists" => pred(Predicate::Exists, false),
            "notExists" => pred(Predicate::Exists, true),
            "equal" => cmp(Comparison::Loose, false),
            "notEqual" => cmp(Comparison::Loose, true),
            "strictEqual" => cmp(Comparison::Strict, false),
            "notStrictEqual" => cmp(Comparison::Strict, true),
            "deepEqual" | "deepStrictEqual" => cmp(Comparison::Deep, false),
            "notDeepEqual" => cmp(Comparison::Deep, true),
            _ => None,
        };
    }
    // expect(x).to.[not.]be.equal(y)
    let (segs, complete) = call_chain(&call.callee);
    if !complete {
        return None;
    }
    let root = root_call(call);
    if !matches!(&root.callee, Expression::Identifier(id) if id.name == "expect")
        || root.arguments.len() != 1
    {
        return None;
    }
    let actual = arg_expr(&root.arguments, 0)?;
    // all segments between root and matcher must be language chains
    let (matcher, rest) = segs.split_first()?;
    let mut negated = false;
    let mut deep = false;
    for (s, _) in rest {
        match *s {
            "not" => negated = !negated,
            "deep" => deep = true,
            s if CHAI_NON_TERMINAL.contains(&s) => {}
            _ => return None,
        }
    }
    let kind = match matcher.0 {
        "equal" | "equals" | "eq" if call.arguments.len() == 1 => AssertionKind::Comparison(
            if deep {
                Comparison::Deep
            } else {
                Comparison::Strict
            },
            arg_expr(&call.arguments, 0)?,
        ),
        "eql" | "eqls" if call.arguments.len() == 1 => {
            AssertionKind::Comparison(Comparison::Deep, arg_expr(&call.arguments, 0)?)
        }
        _ => return None,
    };
    Some(Assertion {
        style: Style::ChaiBdd,
        kind,
        actual,
        negated,
        report: matcher.1,
    })
}

fn extract_node_assert<'b, 'a>(
    ctx: &Ctx<'a, '_>,
    call: &'b CallExpression<'a>,
) -> Option<Assertion<'b, 'a>> {
    let f = fqn(ctx, &call.callee);
    let (method, strict, report) = match f.as_deref() {
        Some(f) if f == "assert" || f == "assert.strict" => {
            ("assert", f == "assert.strict", call.callee.span())
        }
        Some(f) if f.starts_with("assert.") => {
            let (strict, method) = match f.strip_prefix("assert.strict.") {
                Some(m) => (true, m),
                None => (false, &f["assert.".len()..]),
            };
            let method = match method {
                "ok" => "ok",
                "deepEqual" => "deepEqual",
                "notDeepEqual" => "notDeepEqual",
                "strictEqual" => "strictEqual",
                "notStrictEqual" => "notStrictEqual",
                "deepStrictEqual" => "deepStrictEqual",
                "notDeepStrictEqual" => "notDeepStrictEqual",
                _ => return None,
            };
            (method, strict, call.callee.span())
        }
        _ => {
            let Expression::StaticMemberExpression(m) = &call.callee else {
                return None;
            };
            if !matches!(&m.object, Expression::Identifier(id) if id.name == "assert") {
                return None;
            }
            let method = match m.property.name.as_str() {
                "ok" => "ok",
                "strictEqual" => "strictEqual",
                "notStrictEqual" => "notStrictEqual",
                "deepStrictEqual" => "deepStrictEqual",
                "notDeepStrictEqual" => "notDeepStrictEqual",
                _ => return None,
            };
            (method, false, m.property.span)
        }
    };
    let actual = arg_expr(&call.arguments, 0)?;
    if matches!(method, "assert" | "ok") {
        return Some(Assertion {
            style: Style::NodeAssert,
            kind: AssertionKind::Predicate(Predicate::Truthy),
            actual,
            negated: false,
            report: actual.span(),
        });
    }
    let expected = arg_expr(&call.arguments, 1)?;
    let comparison = match method {
        "deepStrictEqual" | "notDeepStrictEqual" => Comparison::Deep,
        "deepEqual" | "notDeepEqual" => {
            if strict {
                Comparison::Deep
            } else {
                Comparison::Loose
            }
        }
        _ => Comparison::Strict,
    };
    Some(Assertion {
        style: Style::NodeAssert,
        kind: AssertionKind::Comparison(comparison, expected),
        actual,
        negated: method.starts_with("not"),
        report,
    })
}

// ---- Testing Library -----------------------------------------------------------------------

/// Name imported from a Testing Library module that `e` refers to (`waitFor`, `screen`, ...),
/// through named, namespace or `require` bindings.
pub fn testing_library_name<'a>(ctx: &Ctx<'a, '_>, e: &Expression<'a>) -> Option<String> {
    let f = fqn(ctx, e)?;
    let rest = f.strip_prefix("@testing-library.")?;
    let (module, name) = rest.split_once('.')?;
    (matches!(
        module,
        "dom" | "react" | "vue" | "angular" | "svelte" | "preact"
    ) && !name.contains('.'))
    .then(|| name.to_string())
}

/// Path below a Testing Library module: `fireEvent.click` for `import { fireEvent } from
/// '@testing-library/react'; fireEvent.click`.
pub fn testing_library_path<'a>(ctx: &Ctx<'a, '_>, e: &Expression<'a>) -> Option<String> {
    let f = fqn(ctx, e)?;
    let rest = f.strip_prefix("@testing-library.")?;
    let (module, path) = rest.split_once('.')?;
    matches!(
        module,
        "dom" | "react" | "vue" | "angular" | "svelte" | "preact"
    )
    .then(|| path.to_string())
}

/// True when the expression resolves (by import) to something outside Testing Library.
pub fn is_known_non_testing_library<'a>(ctx: &Ctx<'a, '_>, e: &Expression<'a>) -> bool {
    fqn(ctx, e).is_some_and(|f| !f.starts_with("@testing-library."))
}

/// Name of a query method called by `call` (`screen.getByRole(...)`, `getByRole(...)`).
pub fn query_method<'a>(call: &CallExpression<'a>) -> Option<(&'a str, Span)> {
    match &call.callee {
        Expression::Identifier(id) => Some((id.name.as_str(), id.span)),
        Expression::StaticMemberExpression(m) => Some((m.property.name.as_str(), m.property.span)),
        _ => None,
    }
}

/// `screen` (or other name) imported directly by name from a Testing Library module.
pub fn is_direct_tl_binding(
    ctx: &Ctx<'_, '_>,
    e: &Expression<'_>,
    name: &str,
    allow_ns: bool,
    allow_subpath: bool,
) -> bool {
    match strip_ts(e) {
        Expression::Identifier(id) => {
            let Some(sym) = ctx.symbol_of(id) else {
                return false;
            };
            let decl = ctx.symbol_decl(sym);
            let AstKind::ImportSpecifier(s) = decl.kind() else {
                return false;
            };
            s.imported.name() == name
                && import_declaration(ctx, decl.id()).is_some_and(|d| {
                    is_testing_library_module(d.source.value.as_str(), allow_subpath)
                })
        }
        Expression::StaticMemberExpression(m) if allow_ns && m.property.name == name => {
            let Expression::Identifier(id) = &m.object else {
                return false;
            };
            let Some(sym) = ctx.symbol_of(id) else {
                return false;
            };
            let decl = ctx.symbol_decl(sym);
            matches!(decl.kind(), AstKind::ImportNamespaceSpecifier(_))
                && import_declaration(ctx, decl.id()).is_some_and(|d| {
                    is_testing_library_module(d.source.value.as_str(), allow_subpath)
                })
        }
        _ => false,
    }
}

/// Query method name if it matches `^(prefix)(All)?By[A-Z]`.
pub fn query_kind(name: &str) -> Option<&'static str> {
    for p in ["get", "query", "find"] {
        if let Some(rest) = name.strip_prefix(p) {
            let rest = rest.strip_prefix("All").unwrap_or(rest);
            if let Some(r) = rest.strip_prefix("By")
                && r.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
            {
                return Some(p);
            }
        }
    }
    None
}
