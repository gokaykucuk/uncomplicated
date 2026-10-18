//! Lightweight, local type inference.
//!
//! Sonar runs the TypeScript compiler to get types, which dominates its analysis time. We don't:
//! instead we infer coarse types from literals, operators, TypeScript annotations, declarations
//! that are never reassigned and well-known built-in APIs. Rules must treat [`Ty::Unknown`] as
//! "can't tell" and stay silent, so missing type info costs recall, never precision.

use oxc_ast::{AstKind, ast::*};
use oxc_semantic::SymbolId;

use crate::Ctx;
use crate::helpers::ast::{strip_ts, string_value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    Unknown,
    Any,
    String,
    Number,
    BigInt,
    Boolean,
    Null,
    Undefined,
    Void,
    Never,
    Symbol,
    Array,
    Tuple,
    /// Plain object or object literal type.
    Object,
    Function,
    Class,
    Promise,
    RegExp,
    Date,
    Map,
    Set,
    Error,
    /// Instance of a named class / interface we know nothing else about.
    Named(String),
    Union(Vec<Ty>),
}

impl Ty {
    pub fn is_unknown(&self) -> bool {
        matches!(self, Ty::Unknown | Ty::Any)
    }

    pub fn is_string(&self) -> bool {
        self.all(|t| *t == Ty::String)
    }

    pub fn is_number(&self) -> bool {
        self.all(|t| *t == Ty::Number)
    }

    pub fn is_boolean(&self) -> bool {
        self.all(|t| *t == Ty::Boolean)
    }

    pub fn is_array(&self) -> bool {
        self.all(|t| matches!(t, Ty::Array | Ty::Tuple))
    }

    pub fn is_promise(&self) -> bool {
        self.all(|t| *t == Ty::Promise)
    }

    pub fn is_function(&self) -> bool {
        self.all(|t| matches!(t, Ty::Function | Ty::Class))
    }

    /// Primitive types (string, number, bigint, boolean, symbol, null, undefined).
    pub fn is_primitive(&self) -> bool {
        self.all(|t| {
            matches!(t, Ty::String | Ty::Number | Ty::BigInt | Ty::Boolean | Ty::Symbol | Ty::Null | Ty::Undefined)
        })
    }

    /// Known to be an object-like value (never a primitive, null or undefined).
    pub fn is_object_like(&self) -> bool {
        self.all(|t| {
            matches!(
                t,
                Ty::Array
                    | Ty::Tuple
                    | Ty::Object
                    | Ty::Function
                    | Ty::Class
                    | Ty::Promise
                    | Ty::RegExp
                    | Ty::Date
                    | Ty::Map
                    | Ty::Set
                    | Ty::Error
                    | Ty::Named(_)
            )
        })
    }

    pub fn is_nullish(&self) -> bool {
        self.all(|t| matches!(t, Ty::Null | Ty::Undefined | Ty::Void))
    }

    /// True if any member of a union may be null/undefined.
    pub fn may_be_nullish(&self) -> bool {
        match self {
            Ty::Union(ts) => ts.iter().any(Ty::may_be_nullish),
            t => matches!(t, Ty::Null | Ty::Undefined | Ty::Void | Ty::Unknown | Ty::Any),
        }
    }

    /// Predicate holds for the type (every member of a union), and the type is known.
    pub fn all(&self, f: impl Fn(&Ty) -> bool + Copy) -> bool {
        match self {
            Ty::Union(ts) => !ts.is_empty() && ts.iter().all(|t| t.all(f)),
            Ty::Unknown | Ty::Any => false,
            t => f(t),
        }
    }

    fn union(mut ts: Vec<Ty>) -> Ty {
        if ts.iter().any(Ty::is_unknown) {
            return Ty::Unknown;
        }
        ts.dedup();
        if ts.len() == 1 { ts.pop().unwrap() } else { Ty::Union(ts) }
    }
}

const MAX_DEPTH: u8 = 6;

/// Infers the type of an expression.
pub fn type_of(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> Ty {
    infer(ctx, e, 0)
}

fn infer(ctx: &Ctx<'_, '_>, e: &Expression<'_>, depth: u8) -> Ty {
    if depth > MAX_DEPTH {
        return Ty::Unknown;
    }
    let d = depth + 1;
    match e {
        Expression::ParenthesizedExpression(p) => infer(ctx, &p.expression, depth),
        Expression::TSAsExpression(x) => from_annotation(ctx, &x.type_annotation, d),
        Expression::TSSatisfiesExpression(x) => infer(ctx, &x.expression, d),
        Expression::TSTypeAssertion(x) => from_annotation(ctx, &x.type_annotation, d),
        Expression::TSNonNullExpression(x) => match infer(ctx, &x.expression, d) {
            Ty::Union(ts) => Ty::union(ts.into_iter().filter(|t| !t.is_nullish()).collect()),
            t => t,
        },
        Expression::StringLiteral(_) | Expression::TemplateLiteral(_) => Ty::String,
        Expression::NumericLiteral(_) => Ty::Number,
        Expression::BigIntLiteral(_) => Ty::BigInt,
        Expression::BooleanLiteral(_) => Ty::Boolean,
        Expression::NullLiteral(_) => Ty::Null,
        Expression::RegExpLiteral(_) => Ty::RegExp,
        Expression::ArrayExpression(_) => Ty::Array,
        Expression::ObjectExpression(_) => Ty::Object,
        Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_) => Ty::Function,
        Expression::ClassExpression(_) => Ty::Class,
        Expression::TaggedTemplateExpression(t) => match crate::helpers::ast::dotted_path(&t.tag).as_deref() {
            Some("String.raw") => Ty::String,
            _ => Ty::Unknown,
        },
        Expression::UnaryExpression(u) => match u.operator {
            UnaryOperator::Typeof => Ty::String,
            UnaryOperator::LogicalNot | UnaryOperator::Delete => Ty::Boolean,
            UnaryOperator::Void => Ty::Undefined,
            UnaryOperator::UnaryNegation | UnaryOperator::BitwiseNot => match infer(ctx, &u.argument, d) {
                Ty::BigInt => Ty::BigInt,
                t if t.is_unknown() => Ty::Unknown,
                _ => Ty::Number,
            },
            UnaryOperator::UnaryPlus => Ty::Number,
        },
        Expression::UpdateExpression(_) => Ty::Number,
        Expression::BinaryExpression(b) => {
            use BinaryOperator::*;
            match b.operator {
                Equality | Inequality | StrictEquality | StrictInequality | LessThan | LessEqualThan | GreaterThan
                | GreaterEqualThan | In | Instanceof => Ty::Boolean,
                Addition => {
                    let (l, r) = (infer(ctx, &b.left, d), infer(ctx, &b.right, d));
                    if l == Ty::String || r == Ty::String {
                        Ty::String
                    } else if l.is_number() && r.is_number() {
                        Ty::Number
                    } else if l == Ty::BigInt && r == Ty::BigInt {
                        Ty::BigInt
                    } else {
                        Ty::Unknown
                    }
                }
                _ => {
                    let (l, r) = (infer(ctx, &b.left, d), infer(ctx, &b.right, d));
                    if l == Ty::BigInt || r == Ty::BigInt { Ty::BigInt } else if l.is_unknown() && r.is_unknown() { Ty::Unknown } else { Ty::Number }
                }
            }
        }
        Expression::LogicalExpression(l) => {
            let (a, b) = (infer(ctx, &l.left, d), infer(ctx, &l.right, d));
            if a == b { a } else { Ty::Unknown }
        }
        Expression::ConditionalExpression(c) => {
            let (a, b) = (infer(ctx, &c.consequent, d), infer(ctx, &c.alternate, d));
            if a == b { a } else { Ty::union(vec![a, b]) }
        }
        Expression::AssignmentExpression(a) => infer(ctx, &a.right, d),
        Expression::SequenceExpression(s) => s.expressions.last().map_or(Ty::Unknown, |e| infer(ctx, e, d)),
        Expression::AwaitExpression(a) => match infer(ctx, &a.argument, d) {
            Ty::Promise => Ty::Unknown,
            t => t,
        },
        Expression::NewExpression(n) => match strip_ts(&n.callee) {
            Expression::Identifier(id) if ctx.symbol_of(id).is_none() => builtin_ctor(&id.name),
            Expression::Identifier(id) => Ty::Named(id.name.to_string()),
            _ => Ty::Object,
        },
        Expression::CallExpression(c) => infer_call(ctx, c, d),
        Expression::Identifier(id) => infer_ident(ctx, id, d),
        Expression::StaticMemberExpression(m) => infer_member(ctx, &m.object, &m.property.name, d),
        Expression::ChainExpression(_) => Ty::Unknown,
        _ => Ty::Unknown,
    }
}

fn builtin_ctor(name: &str) -> Ty {
    match name {
        "Array" => Ty::Array,
        "Object" => Ty::Object,
        "Map" | "WeakMap" => Ty::Map,
        "Set" | "WeakSet" => Ty::Set,
        "Date" => Ty::Date,
        "RegExp" => Ty::RegExp,
        "Promise" => Ty::Promise,
        "Function" => Ty::Function,
        "Error" | "TypeError" | "RangeError" | "SyntaxError" | "ReferenceError" | "EvalError" | "URIError"
        | "AggregateError" => Ty::Error,
        // `new String(...)` etc. are wrapper objects, not primitives.
        "String" | "Number" | "Boolean" => Ty::Object,
        other => Ty::Named(other.to_string()),
    }
}

fn infer_ident(ctx: &Ctx<'_, '_>, id: &IdentifierReference<'_>, d: u8) -> Ty {
    let Some(sym) = ctx.symbol_of(id) else {
        return match id.name.as_str() {
            "undefined" => Ty::Undefined,
            "NaN" | "Infinity" => Ty::Number,
            _ => Ty::Unknown,
        };
    };
    symbol_type(ctx, sym, d)
}

/// Type of a symbol from its declaration (annotation, or initializer when never reassigned).
pub fn symbol_type(ctx: &Ctx<'_, '_>, sym: SymbolId, d: u8) -> Ty {
    if d > MAX_DEPTH {
        return Ty::Unknown;
    }
    let decl = ctx.symbol_decl(sym);
    match decl.kind() {
        AstKind::VariableDeclarator(v) => {
            if let Some(ann) = &v.type_annotation {
                return from_annotation(ctx, &ann.type_annotation, d);
            }
            if !matches!(v.id, BindingPattern::BindingIdentifier(_)) {
                return Ty::Unknown;
            }
            let reassigned = ctx.scoping().symbol_is_mutated(sym);
            let is_const = matches!(ctx.parent_kind(decl.id()), AstKind::VariableDeclaration(vd) if vd.kind == VariableDeclarationKind::Const);
            match &v.init {
                Some(init) if !reassigned || is_const => infer(ctx, init, d),
                None if !reassigned => Ty::Undefined,
                _ => Ty::Unknown,
            }
        }
        AstKind::FormalParameter(p) => match &p.type_annotation {
            Some(ann) if matches!(p.pattern, BindingPattern::BindingIdentifier(_)) => {
                let t = from_annotation(ctx, &ann.type_annotation, d);
                if p.optional { Ty::union(vec![t, Ty::Undefined]) } else { t }
            }
            _ => Ty::Unknown,
        },
        AstKind::Function(_) => Ty::Function,
        AstKind::Class(_) => Ty::Class,
        _ => Ty::Unknown,
    }
}

fn infer_member(ctx: &Ctx<'_, '_>, obj: &Expression<'_>, prop: &str, d: u8) -> Ty {
    let ot = infer(ctx, obj, d);
    match prop {
        "length" if ot.is_string() || ot.is_array() => Ty::Number,
        "size" if matches!(ot, Ty::Map | Ty::Set) => Ty::Number,
        _ => match (strip_ts(obj), prop) {
            (Expression::Identifier(id), p) if ctx.symbol_of(id).is_none() => match (id.name.as_str(), p) {
                ("Number", "MAX_SAFE_INTEGER" | "MIN_SAFE_INTEGER" | "EPSILON" | "MAX_VALUE" | "MIN_VALUE" | "NaN"
                | "POSITIVE_INFINITY" | "NEGATIVE_INFINITY") => Ty::Number,
                ("Math", _) => Ty::Number,
                _ => Ty::Unknown,
            },
            _ => Ty::Unknown,
        },
    }
}

fn infer_call(ctx: &Ctx<'_, '_>, c: &CallExpression<'_>, d: u8) -> Ty {
    match strip_ts(&c.callee) {
        Expression::Identifier(id) => {
            if let Some(sym) = ctx.symbol_of(id) {
                return match ctx.symbol_decl(sym).kind() {
                    AstKind::Function(f) => function_return(ctx, f.r#async, f.generator, f.return_type.as_deref(), d),
                    AstKind::VariableDeclarator(v) => match v.init.as_ref().map(strip_ts) {
                        Some(Expression::ArrowFunctionExpression(a)) => {
                            function_return(ctx, a.r#async, false, a.return_type.as_deref(), d)
                        }
                        Some(Expression::FunctionExpression(f)) => {
                            function_return(ctx, f.r#async, f.generator, f.return_type.as_deref(), d)
                        }
                        _ => Ty::Unknown,
                    },
                    _ => Ty::Unknown,
                };
            }
            match id.name.as_str() {
                "String" | "encodeURIComponent" | "decodeURIComponent" | "encodeURI" | "decodeURI" | "escape"
                | "unescape" | "btoa" | "atob" => Ty::String,
                "Number" | "parseInt" | "parseFloat" => Ty::Number,
                "Boolean" | "isNaN" | "isFinite" => Ty::Boolean,
                "BigInt" => Ty::BigInt,
                "Symbol" => Ty::Symbol,
                "Array" => Ty::Array,
                "fetch" => Ty::Promise,
                "require" => Ty::Unknown,
                _ => Ty::Unknown,
            }
        }
        Expression::StaticMemberExpression(m) => {
            let name = m.property.name.as_str();
            if let Expression::Identifier(obj) = strip_ts(&m.object)
                && ctx.symbol_of(obj).is_none()
            {
                let t = match (obj.name.as_str(), name) {
                    ("JSON", "stringify") => Ty::String,
                    ("Object", "keys" | "values" | "entries" | "getOwnPropertyNames") => Ty::Array,
                    ("Object", "assign" | "create" | "freeze" | "fromEntries") => Ty::Object,
                    ("Array", "from" | "of") => Ty::Array,
                    ("Array", "isArray") | ("Number", "isInteger" | "isNaN" | "isFinite" | "isSafeInteger") => Ty::Boolean,
                    ("Number", "parseInt" | "parseFloat") | ("Math", _) | ("Date", "now" | "parse" | "UTC") => Ty::Number,
                    ("String", "fromCharCode" | "fromCodePoint" | "raw") => Ty::String,
                    ("Promise", _) => Ty::Promise,
                    _ => Ty::Unknown,
                };
                if !t.is_unknown() {
                    return t;
                }
            }
            let ot = infer(ctx, &m.object, d);
            method_return(&ot, name)
        }
        _ => Ty::Unknown,
    }
}

fn method_return(recv: &Ty, name: &str) -> Ty {
    if recv.is_string() {
        return match name {
            "charAt" | "concat" | "padEnd" | "padStart" | "repeat" | "replace" | "replaceAll" | "slice" | "substring"
            | "substr" | "toLowerCase" | "toUpperCase" | "toLocaleLowerCase" | "toLocaleUpperCase" | "trim"
            | "trimEnd" | "trimStart" | "normalize" | "at" | "toString" | "valueOf" => Ty::String,
            "charCodeAt" | "codePointAt" | "indexOf" | "lastIndexOf" | "localeCompare" | "search" => Ty::Number,
            "includes" | "startsWith" | "endsWith" => Ty::Boolean,
            "split" => Ty::Array,
            _ => Ty::Unknown,
        };
    }
    if recv.is_array() {
        return match name {
            "map" | "filter" | "slice" | "concat" | "flat" | "flatMap" | "reverse" | "sort" | "splice" | "toSorted"
            | "toReversed" | "toSpliced" | "with" | "fill" | "copyWithin" => Ty::Array,
            "join" | "toString" => Ty::String,
            "indexOf" | "lastIndexOf" | "findIndex" | "findLastIndex" | "push" | "unshift" => Ty::Number,
            "includes" | "some" | "every" => Ty::Boolean,
            _ => Ty::Unknown,
        };
    }
    if recv.is_promise() {
        return match name {
            "then" | "catch" | "finally" => Ty::Promise,
            _ => Ty::Unknown,
        };
    }
    match (recv, name) {
        (Ty::Number, "toFixed" | "toString" | "toPrecision" | "toExponential" | "toLocaleString") => Ty::String,
        (Ty::Date, "getTime" | "valueOf" | "getFullYear" | "getMonth" | "getDate" | "getDay" | "getHours") => Ty::Number,
        (Ty::Date, "toISOString" | "toString" | "toDateString" | "toLocaleString" | "toJSON") => Ty::String,
        (Ty::RegExp, "test") => Ty::Boolean,
        (Ty::Map | Ty::Set, "has" | "delete") => Ty::Boolean,
        _ => Ty::Unknown,
    }
}

fn function_return(ctx: &Ctx<'_, '_>, is_async: bool, generator: bool, ret: Option<&TSTypeAnnotation<'_>>, d: u8) -> Ty {
    if generator {
        return Ty::Object;
    }
    if is_async {
        return Ty::Promise;
    }
    ret.map_or(Ty::Unknown, |r| from_annotation(ctx, &r.type_annotation, d))
}

/// Maps a TypeScript type annotation to a coarse type.
pub fn from_annotation(ctx: &Ctx<'_, '_>, t: &TSType<'_>, d: u8) -> Ty {
    if d > MAX_DEPTH {
        return Ty::Unknown;
    }
    match t {
        TSType::TSAnyKeyword(_) => Ty::Any,
        TSType::TSUnknownKeyword(_) => Ty::Unknown,
        TSType::TSStringKeyword(_) | TSType::TSTemplateLiteralType(_) => Ty::String,
        TSType::TSNumberKeyword(_) => Ty::Number,
        TSType::TSBigIntKeyword(_) => Ty::BigInt,
        TSType::TSBooleanKeyword(_) => Ty::Boolean,
        TSType::TSNullKeyword(_) => Ty::Null,
        TSType::TSUndefinedKeyword(_) => Ty::Undefined,
        TSType::TSVoidKeyword(_) => Ty::Void,
        TSType::TSNeverKeyword(_) => Ty::Never,
        TSType::TSSymbolKeyword(_) => Ty::Symbol,
        TSType::TSObjectKeyword(_) | TSType::TSTypeLiteral(_) | TSType::TSMappedType(_) => Ty::Object,
        TSType::TSArrayType(_) => Ty::Array,
        TSType::TSTupleType(_) => Ty::Tuple,
        TSType::TSFunctionType(_) => Ty::Function,
        TSType::TSConstructorType(_) => Ty::Class,
        TSType::TSParenthesizedType(p) => from_annotation(ctx, &p.type_annotation, d),
        TSType::TSTypeOperatorType(o) if o.operator == TSTypeOperatorOperator::Readonly => {
            from_annotation(ctx, &o.type_annotation, d + 1)
        }
        TSType::TSLiteralType(l) => match &l.literal {
            TSLiteral::StringLiteral(_) | TSLiteral::TemplateLiteral(_) => Ty::String,
            TSLiteral::NumericLiteral(_) => Ty::Number,
            TSLiteral::BigIntLiteral(_) => Ty::BigInt,
            TSLiteral::BooleanLiteral(_) => Ty::Boolean,
            TSLiteral::UnaryExpression(_) => Ty::Number,
        },
        TSType::TSUnionType(u) => Ty::union(u.types.iter().map(|t| from_annotation(ctx, t, d + 1)).collect()),
        TSType::TSTypeReference(r) => match &r.type_name {
            TSTypeName::IdentifierReference(id) => match id.name.as_str() {
                "Array" | "ReadonlyArray" => Ty::Array,
                "Promise" | "PromiseLike" => Ty::Promise,
                "Map" | "WeakMap" | "ReadonlyMap" => Ty::Map,
                "Set" | "WeakSet" | "ReadonlySet" => Ty::Set,
                "Date" => Ty::Date,
                "RegExp" => Ty::RegExp,
                "Error" => Ty::Error,
                "Function" => Ty::Function,
                "Record" | "Partial" | "Required" | "Readonly" | "Pick" | "Omit" => Ty::Object,
                "String" | "Number" | "Boolean" | "Object" => Ty::Object,
                name => {
                    // Local type aliases resolve one level.
                    if let Some(sym) = ctx.scoping().get_reference(id.reference_id()).symbol_id()
                        && let AstKind::TSTypeAliasDeclaration(a) = ctx.symbol_decl(sym).kind()
                    {
                        return from_annotation(ctx, &a.type_annotation, d + 1);
                    }
                    if ctx.scoping().get_reference(id.reference_id()).symbol_id().is_some_and(|s| {
                        matches!(ctx.symbol_decl(s).kind(), AstKind::TSTypeParameter(_))
                    }) {
                        return Ty::Unknown;
                    }
                    Ty::Named(name.to_string())
                }
            },
            _ => Ty::Unknown,
        },
        _ => Ty::Unknown,
    }
}

/// Convenience: string value of a literal, otherwise whether the expression is a string.
pub fn is_string_like(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    string_value(e).is_some() || type_of(ctx, e).is_string()
}
