//! Deprecated APIs should not be used
//!
//! Sonar relays the TypeScript checker's deprecation suggestions. Without a type checker we
//! report:
//! - references to variables and functions declared in the same file with a `/** @deprecated */`
//!   JSDoc tag;
//! - deprecated TypeScript lib / DOM APIs (table extracted from the TypeScript lib `.d.ts`
//!   files), on globals and on receivers whose lib type we can infer: DOM globals (`document`,
//!   `window`, `RegExp`, …), strings, `new XEvent()` / `document.createEvent("X")`, typed
//!   parameters and `window/document.addEventListener("type", e => …)` listener parameters.
use crate::helpers::batch_security1::{resolve_value, unique_init};
use crate::prelude::*;

pub struct Rule {
    /// Member name → indexes into `DEPRECATED`.
    by_member: FxHashMap<&'static str, Vec<usize>>,
    /// Interface names appearing in the tables.
    types: FxHashSet<&'static str>,
}

impl Rule {
    pub fn new(_: &Params) -> Self {
        let mut by_member: FxHashMap<&'static str, Vec<usize>> = FxHashMap::default();
        let mut types = FxHashSet::default();
        for (i, (t, m, _, _)) in DEPRECATED.iter().enumerate() {
            by_member.entry(*m).or_default().push(i);
            types.insert(*t);
        }
        for (_, t) in GLOBAL_TYPES.iter().chain(EVENTS) {
            types.insert(*t);
        }
        Self { by_member, types }
    }

    fn intern(&self, name: &str) -> Option<&'static str> {
        self.types.get(name).copied()
    }
}

fn global_type(name: &str) -> Option<&'static str> {
    GLOBAL_TYPES.binary_search_by(|(g, _)| g.cmp(&name)).ok().map(|i| GLOBAL_TYPES[i].1)
}

fn event_type(name: &str) -> Option<&'static str> {
    EVENTS.binary_search_by(|(e, _)| e.cmp(&name)).ok().map(|i| EVENTS[i].1)
}

const STRING_METHODS: &[&str] = &[
    "charAt", "concat", "padEnd", "padStart", "repeat", "replace", "replaceAll", "slice", "substring", "substr",
    "toLowerCase", "toUpperCase", "toLocaleLowerCase", "toLocaleUpperCase", "trim", "trimEnd", "trimStart",
    "trimLeft", "trimRight", "normalize",
];

impl Rule {
    /// Lib interface an expression is known to have.
    fn lib_type<'a>(&self, ctx: &Ctx<'a, '_>, e: &Expression<'a>, depth: u8) -> Option<&'static str> {
        if depth > 5 {
            return None;
        }
        let e = strip_ts(e);
        match e {
            Expression::StringLiteral(_) | Expression::TemplateLiteral(_) => Some("String"),
            Expression::Identifier(id) => match ctx.symbol_of(id) {
                None => global_type(id.name.as_str()),
                Some(sym) => {
                    if let Some(init) = unique_init(ctx, sym) {
                        return self.lib_type(ctx, init, depth + 1);
                    }
                    if ctx.scoping().symbol_is_mutated(sym) {
                        return None;
                    }
                    if let Some(t) = self.listener_param_type(ctx, sym, depth) {
                        return Some(t);
                    }
                    self.from_ty(type_of(ctx, e))
                }
            },
            Expression::StaticMemberExpression(m) => {
                if self.lib_type(ctx, &m.object, depth + 1) == Some("Window") {
                    return global_type(m.property.name.as_str());
                }
                None
            }
            Expression::CallExpression(c) => {
                let Expression::StaticMemberExpression(m) = strip_parens(&c.callee) else {
                    return self.from_ty(type_of(ctx, e));
                };
                let name = m.property.name.as_str();
                if name == "createEvent" && self.lib_type(ctx, &m.object, depth + 1) == Some("Document") {
                    let lit = c.arguments.first().and_then(|a| a.as_expression()).map(|a| resolve_value(ctx, a));
                    return match lit {
                        // `createEvent("MouseEvents")` returns a `MouseEvent`.
                        Some(Expression::StringLiteral(s)) => self
                            .intern(s.value.as_str())
                            .or_else(|| s.value.strip_suffix('s').and_then(|n| self.intern(n)))
                            .or(Some("Event")),
                        _ => None,
                    };
                }
                if STRING_METHODS.contains(&name) && self.lib_type(ctx, &m.object, depth + 1) == Some("String") {
                    return Some("String");
                }
                self.from_ty(type_of(ctx, e))
            }
            Expression::NewExpression(n) => match strip_parens(&n.callee) {
                Expression::Identifier(id) if ctx.symbol_of(id).is_none() && id.name.ends_with("Event") => {
                    self.intern(id.name.as_str())
                }
                _ => None,
            },
            _ => self.from_ty(type_of(ctx, e)),
        }
    }

    fn from_ty(&self, t: Ty) -> Option<&'static str> {
        match t {
            Ty::String => Some("String"),
            Ty::Named(n) => self.intern(&n),
            _ => None,
        }
    }

    /// `window.addEventListener("keydown", function (e) {...})`: `e` is a `KeyboardEvent`.
    fn listener_param_type<'a>(&self, ctx: &Ctx<'a, '_>, sym: SymbolId, depth: u8) -> Option<&'static str> {
        let decl = ctx.symbol_decl(sym);
        let AstKind::FormalParameter(p) = decl.kind() else { return None };
        if p.type_annotation.is_some() {
            return None;
        }
        let params = ctx.parent(decl.id());
        let AstKind::FormalParameters(fp) = params.kind() else { return None };
        if fp.items.first().map(|f| f.span) != Some(p.span) {
            return None;
        }
        let func = ctx.parent(params.id());
        let AstKind::CallExpression(call) = ctx.parent_kind(func.id()) else { return None };
        let Expression::StaticMemberExpression(m) = strip_parens(&call.callee) else { return None };
        if m.property.name != "addEventListener" || call.arguments.get(1).map(|a| a.span()) != Some(func.kind().span()) {
            return None;
        }
        if !matches!(self.lib_type(ctx, &m.object, depth + 1), Some("Window" | "Document")) {
            return None;
        }
        let Some(Expression::StringLiteral(ev)) = call.arguments.first().and_then(|a| a.as_expression()).map(strip_parens)
        else {
            return None;
        };
        event_type(ev.value.as_str())
    }

    fn check_member<'a>(&self, ctx: &Ctx<'a, '_>, node: &AstNode<'a>, m: &StaticMemberExpression<'a>) {
        let name = m.property.name.as_str();
        let Some(idxs) = self.by_member.get(name) else { return };
        let Some(t) = self.lib_type(ctx, &m.object, 0) else { return };
        let call = match ctx.parent_kind(node.id()) {
            AstKind::CallExpression(c) if c.callee.span() == m.span => Some(c),
            _ => None,
        };
        for &i in idxs {
            let (et, _, sig, lit) = DEPRECATED[i];
            if et != t {
                continue;
            }
            if let Some(lit) = lit {
                let first = call.and_then(|c| c.arguments.first()).and_then(|a| a.as_expression()).map(strip_parens);
                if !matches!(first, Some(Expression::StringLiteral(s)) if s.value == lit) {
                    continue;
                }
            }
            self.report(ctx, m.property.span, name, sig, call.map(|c| c.callee.span()));
            return;
        }
    }

    fn check_global<'a>(&self, ctx: &Ctx<'a, '_>, node: &AstNode<'a>, id: &IdentifierReference<'a>) {
        let name = id.name.as_str();
        let Some(idxs) = self.by_member.get(name) else { return };
        let Some(&i) = idxs.iter().find(|&&i| DEPRECATED[i].0.is_empty()) else { return };
        if ctx.symbol_of(id).is_some() {
            return;
        }
        let call = match ctx.parent_kind(node.id()) {
            AstKind::CallExpression(c) if c.callee.span() == id.span => Some(c.callee.span()),
            _ => None,
        };
        self.report(ctx, id.span, name, DEPRECATED[i].2, call);
    }

    fn report(&self, ctx: &Ctx<'_, '_>, span: Span, name: &str, sig: Option<&str>, call: Option<Span>) {
        match (sig, call) {
            (Some(""), Some(_)) => {}
            (Some(sig), Some(callee)) => ctx.report(
                span,
                format!("The signature '{sig}' of '{}' is deprecated.", ctx.text(callee)),
            ),
            _ => ctx.report(span, format!("'{name}' is deprecated.")),
        }
    }
}

fn is_function_like(e: &Expression<'_>) -> bool {
    matches!(
        strip_parens(e),
        Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_) | Expression::ClassExpression(_)
    )
}

/// Deprecated symbols declared by a statement, with whether they are functions.
fn declared<'a>(stmt: AstKind<'a>, out: &mut Vec<(SymbolId, bool)>) {
    match stmt {
        AstKind::VariableDeclaration(d) => {
            for v in &d.declarations {
                if let BindingPattern::BindingIdentifier(b) = &v.id
                    && let Some(sym) = b.symbol_id.get()
                {
                    let func = v.init.as_ref().is_some_and(is_function_like);
                    if !func {
                        out.push((sym, false));
                    }
                }
            }
        }
        AstKind::Function(f) if f.is_declaration() => {
            if let Some(sym) = f.id.as_ref().and_then(|id| id.symbol_id.get()) {
                out.push((sym, true));
            }
        }
        AstKind::ExportDeclaration(e) => match &e.declaration {
            Declaration::VariableDeclaration(d) => declared(AstKind::VariableDeclaration(d), out),
            Declaration::FunctionDeclaration(f) => declared(AstKind::Function(f), out),
            _ => {}
        },
        _ => {}
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::StaticMemberExpression, AstType::IdentifierReference]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::StaticMemberExpression(m) => self.check_member(ctx, node, m),
            AstKind::IdentifierReference(id) => self.check_global(ctx, node, id),
            _ => {}
        }
    }

    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let mut starts: FxHashSet<u32> = FxHashSet::default();
        for c in ctx.comments() {
            if !c.is_block() {
                continue;
            }
            let text = ctx.comment_text(c);
            if text.starts_with('*')
                && text.contains("@deprecated")
                && let Some(next) = token_after(ctx, c.span.end)
            {
                starts.insert(next.start);
            }
        }
        if starts.is_empty() {
            return;
        }
        let mut symbols = Vec::new();
        for node in ctx.nodes().iter() {
            let kind = node.kind();
            if !matches!(kind, AstKind::VariableDeclaration(_) | AstKind::Function(_) | AstKind::ExportDeclaration(_)) {
                continue;
            }
            if starts.contains(&kind.span().start) {
                declared(kind, &mut symbols);
            }
        }
        for (sym, is_function) in symbols {
            let name = ctx.scoping().symbol_name(sym);
            for r in ctx.scoping().get_resolved_references(sym) {
                let span = ctx.node(r.node_id()).kind().span();
                let parent = ctx.parent(r.node_id());
                let skip = match parent.kind() {
                    AstKind::ExportSpecifier(_) => true,
                    AstKind::CallExpression(c) => is_function && c.callee.span() == span,
                    AstKind::NewExpression(n) => is_function && n.callee.span() == span,
                    _ => false,
                };
                if !skip {
                    ctx.report(span, format!("'{name}' is deprecated."));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn same_file() {
        check(
            "S1874",
            "a.ts",
            r#"
/** @deprecated use NEW_LIMIT */
const OLD_LIMIT = 10;
/**
 * Old helper.
 * @deprecated
 */
export function oldHelper() { return 1; }
// @deprecated not a JSDoc
const plain = 1;
const NEW_LIMIT = 20;
use(OLD_LIMIT); // Noncompliant {{'OLD_LIMIT' is deprecated.}}
const fn = oldHelper; // Noncompliant {{'oldHelper' is deprecated.}}
oldHelper();
use(plain, NEW_LIMIT);
"#,
        );
    }

    #[test]
    fn lib() {
        check(
            "S1874",
            "a.js",
            r#"
const s = "abc".substr(1); // Noncompliant {{The signature '(from: number, length?: number): string' of '"abc".substr' is deprecated.}}
const t = "abc";
t.trimLeft(); // Noncompliant
unknown.substr(1);
const u = unescape(x); // Noncompliant {{The signature '(string: string): string' of 'unescape' is deprecated.}}
document.execCommand("copy"); // Noncompliant
document.write("<p>"); // Noncompliant
const d = document.domain; // Noncompliant {{'domain' is deprecated.}}
const r = RegExp.$1; // Noncompliant {{'$1' is deprecated.}}
const ev = document.createEvent('MouseEvents');
ev.initMouseEvent('click', true, true, window, 0, 0, 0, 0, 0, false, false, false, false, 0, null); // Noncompliant
ev.preventDefault();
const anim = document.createEvent('AnimationEvent'); // Noncompliant
const ce = new CustomEvent('x');
ce.initEvent('x', true, true); // Noncompliant
window.addEventListener('keydown', function (e) {
  if (e.keyCode === 13) {} // Noncompliant {{'keyCode' is deprecated.}}
  if (e.key === 'Enter') {}
});
el.addEventListener('keydown', function (e) { e.keyCode; });
if (window.onwebkittransitionend !== undefined) {} // Noncompliant
console.log(name); // Noncompliant {{'name' is deprecated.}}
function f(name) { return name; }
var event;
"#,
        );
    }
}

// ---- tables -------------------------------------------------------------------------------
// Generated from TypeScript 6.0 lib.es5 / es2015.core / es2019.string / dom .d.ts files.
/// (receiver type, member, call signature (`None` = property, `""` = overloaded), required literal first argument)
pub const DEPRECATED: &[(&str, &str, Option<&str>, Option<&str>)] = &[
    ("", "blur", Some("(): void"), None),
    ("", "captureEvents", Some("(): void"), None),
    ("", "clientInformation", None, None),
    ("", "escape", Some("(string: string): string"), None),
    ("", "event", None, None),
    ("", "external", None, None),
    ("", "name", None, None),
    ("", "onkeypress", None, None),
    ("", "onorientationchange", None, None),
    ("", "onunload", None, None),
    ("", "onwebkitanimationend", None, None),
    ("", "onwebkitanimationiteration", None, None),
    ("", "onwebkitanimationstart", None, None),
    ("", "onwebkittransitionend", None, None),
    ("", "orientation", None, None),
    ("", "releaseEvents", Some("(): void"), None),
    ("", "status", None, None),
    ("", "unescape", Some("(string: string): string"), None),
    ("AnimationEvent", "cancelBubble", None, None),
    ("AnimationEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("AnimationEvent", "returnValue", None, None),
    ("AnimationEvent", "srcElement", None, None),
    ("AnimationPlaybackEvent", "cancelBubble", None, None),
    ("AnimationPlaybackEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("AnimationPlaybackEvent", "returnValue", None, None),
    ("AnimationPlaybackEvent", "srcElement", None, None),
    ("AudioProcessingEvent", "cancelBubble", None, None),
    ("AudioProcessingEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("AudioProcessingEvent", "inputBuffer", None, None),
    ("AudioProcessingEvent", "outputBuffer", None, None),
    ("AudioProcessingEvent", "playbackTime", None, None),
    ("AudioProcessingEvent", "returnValue", None, None),
    ("AudioProcessingEvent", "srcElement", None, None),
    ("BeforeUnloadEvent", "cancelBubble", None, None),
    ("BeforeUnloadEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("BeforeUnloadEvent", "returnValue", None, None),
    ("BeforeUnloadEvent", "srcElement", None, None),
    ("BlobEvent", "cancelBubble", None, None),
    ("BlobEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("BlobEvent", "returnValue", None, None),
    ("BlobEvent", "srcElement", None, None),
    ("ClipboardEvent", "cancelBubble", None, None),
    ("ClipboardEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("ClipboardEvent", "returnValue", None, None),
    ("ClipboardEvent", "srcElement", None, None),
    ("CloseEvent", "cancelBubble", None, None),
    ("CloseEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("CloseEvent", "returnValue", None, None),
    ("CloseEvent", "srcElement", None, None),
    ("CommandEvent", "cancelBubble", None, None),
    ("CommandEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("CommandEvent", "returnValue", None, None),
    ("CommandEvent", "srcElement", None, None),
    ("CompositionEvent", "cancelBubble", None, None),
    ("CompositionEvent", "initCompositionEvent", Some("(typeArg: string, bubblesArg?: boolean, cancelableArg?: boolean, viewArg?: WindowProxy | null, dataArg?: string): void"), None),
    ("CompositionEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("CompositionEvent", "initUIEvent", Some("(typeArg: string, bubblesArg?: boolean, cancelableArg?: boolean, viewArg?: Window | null, detailArg?: number): void"), None),
    ("CompositionEvent", "returnValue", None, None),
    ("CompositionEvent", "srcElement", None, None),
    ("CompositionEvent", "which", None, None),
    ("ContentVisibilityAutoStateChangeEvent", "cancelBubble", None, None),
    ("ContentVisibilityAutoStateChangeEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("ContentVisibilityAutoStateChangeEvent", "returnValue", None, None),
    ("ContentVisibilityAutoStateChangeEvent", "srcElement", None, None),
    ("CookieChangeEvent", "cancelBubble", None, None),
    ("CookieChangeEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("CookieChangeEvent", "returnValue", None, None),
    ("CookieChangeEvent", "srcElement", None, None),
    ("CustomEvent", "cancelBubble", None, None),
    ("CustomEvent", "initCustomEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean, detail?: T): void"), None),
    ("CustomEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("CustomEvent", "returnValue", None, None),
    ("CustomEvent", "srcElement", None, None),
    ("DeviceMotionEvent", "cancelBubble", None, None),
    ("DeviceMotionEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("DeviceMotionEvent", "returnValue", None, None),
    ("DeviceMotionEvent", "srcElement", None, None),
    ("DeviceOrientationEvent", "cancelBubble", None, None),
    ("DeviceOrientationEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("DeviceOrientationEvent", "returnValue", None, None),
    ("DeviceOrientationEvent", "srcElement", None, None),
    ("Document", "alinkColor", None, None),
    ("Document", "all", None, None),
    ("Document", "anchors", None, None),
    ("Document", "applets", None, None),
    ("Document", "bgColor", None, None),
    ("Document", "captureEvents", Some("(): void"), None),
    ("Document", "caretRangeFromPoint", Some("(x: number, y: number): Range | null"), None),
    ("Document", "charset", None, None),
    ("Document", "clear", Some("(): void"), None),
    ("Document", "createEvent", Some("(eventInterface: \"AnimationEvent\"): AnimationEvent"), Some("AnimationEvent")),
    ("Document", "createNSResolver", Some("(nodeResolver: Node): Node"), None),
    ("Document", "domain", None, None),
    ("Document", "execCommand", Some("(commandId: string, showUI?: boolean, value?: string): boolean"), None),
    ("Document", "fgColor", None, None),
    ("Document", "fullscreen", None, None),
    ("Document", "inputEncoding", None, None),
    ("Document", "linkColor", None, None),
    ("Document", "onkeypress", None, None),
    ("Document", "onwebkitanimationend", None, None),
    ("Document", "onwebkitanimationiteration", None, None),
    ("Document", "onwebkitanimationstart", None, None),
    ("Document", "onwebkittransitionend", None, None),
    ("Document", "queryCommandEnabled", Some("(commandId: string): boolean"), None),
    ("Document", "queryCommandIndeterm", Some("(commandId: string): boolean"), None),
    ("Document", "queryCommandState", Some("(commandId: string): boolean"), None),
    ("Document", "queryCommandSupported", Some("(commandId: string): boolean"), None),
    ("Document", "queryCommandValue", Some("(commandId: string): string"), None),
    ("Document", "releaseEvents", Some("(): void"), None),
    ("Document", "rootElement", None, None),
    ("Document", "vlinkColor", None, None),
    ("Document", "write", Some("(...text: string[]): void"), None),
    ("Document", "writeln", Some("(...text: string[]): void"), None),
    ("DragEvent", "cancelBubble", None, None),
    ("DragEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("DragEvent", "initMouseEvent", Some("(typeArg: string, canBubbleArg: boolean, cancelableArg: boolean, viewArg: Window, detailArg: number, screenXArg: number, screenYArg: number, clientXArg: number, clientYArg: number, ctrlKeyArg: boolean, altKeyArg: boolean, shiftKeyArg: boolean, metaKeyArg: boolean, buttonArg: number, relatedTargetArg: EventTarget | null): void"), None),
    ("DragEvent", "initUIEvent", Some("(typeArg: string, bubblesArg?: boolean, cancelableArg?: boolean, viewArg?: Window | null, detailArg?: number): void"), None),
    ("DragEvent", "returnValue", None, None),
    ("DragEvent", "srcElement", None, None),
    ("DragEvent", "which", None, None),
    ("ErrorEvent", "cancelBubble", None, None),
    ("ErrorEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("ErrorEvent", "returnValue", None, None),
    ("ErrorEvent", "srcElement", None, None),
    ("Event", "cancelBubble", None, None),
    ("Event", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("Event", "returnValue", None, None),
    ("Event", "srcElement", None, None),
    ("External", "AddSearchProvider", Some("(): void"), None),
    ("External", "IsSearchProviderInstalled", Some("(): void"), None),
    ("FocusEvent", "cancelBubble", None, None),
    ("FocusEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("FocusEvent", "initUIEvent", Some("(typeArg: string, bubblesArg?: boolean, cancelableArg?: boolean, viewArg?: Window | null, detailArg?: number): void"), None),
    ("FocusEvent", "returnValue", None, None),
    ("FocusEvent", "srcElement", None, None),
    ("FocusEvent", "which", None, None),
    ("FontFaceSetLoadEvent", "cancelBubble", None, None),
    ("FontFaceSetLoadEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("FontFaceSetLoadEvent", "returnValue", None, None),
    ("FontFaceSetLoadEvent", "srcElement", None, None),
    ("FormDataEvent", "cancelBubble", None, None),
    ("FormDataEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("FormDataEvent", "returnValue", None, None),
    ("FormDataEvent", "srcElement", None, None),
    ("GPUUncapturedErrorEvent", "cancelBubble", None, None),
    ("GPUUncapturedErrorEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("GPUUncapturedErrorEvent", "returnValue", None, None),
    ("GPUUncapturedErrorEvent", "srcElement", None, None),
    ("GamepadEvent", "cancelBubble", None, None),
    ("GamepadEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("GamepadEvent", "returnValue", None, None),
    ("GamepadEvent", "srcElement", None, None),
    ("HashChangeEvent", "cancelBubble", None, None),
    ("HashChangeEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("HashChangeEvent", "returnValue", None, None),
    ("HashChangeEvent", "srcElement", None, None),
    ("IDBVersionChangeEvent", "cancelBubble", None, None),
    ("IDBVersionChangeEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("IDBVersionChangeEvent", "returnValue", None, None),
    ("IDBVersionChangeEvent", "srcElement", None, None),
    ("InputEvent", "cancelBubble", None, None),
    ("InputEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("InputEvent", "initUIEvent", Some("(typeArg: string, bubblesArg?: boolean, cancelableArg?: boolean, viewArg?: Window | null, detailArg?: number): void"), None),
    ("InputEvent", "returnValue", None, None),
    ("InputEvent", "srcElement", None, None),
    ("InputEvent", "which", None, None),
    ("KeyboardEvent", "cancelBubble", None, None),
    ("KeyboardEvent", "charCode", None, None),
    ("KeyboardEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("KeyboardEvent", "initKeyboardEvent", Some("(typeArg: string, bubblesArg?: boolean, cancelableArg?: boolean, viewArg?: Window | null, keyArg?: string, locationArg?: number, ctrlKey?: boolean, altKey?: boolean, shiftKey?: boolean, metaKey?: boolean): void"), None),
    ("KeyboardEvent", "initUIEvent", Some("(typeArg: string, bubblesArg?: boolean, cancelableArg?: boolean, viewArg?: Window | null, detailArg?: number): void"), None),
    ("KeyboardEvent", "keyCode", None, None),
    ("KeyboardEvent", "returnValue", None, None),
    ("KeyboardEvent", "srcElement", None, None),
    ("KeyboardEvent", "which", None, None),
    ("MIDIConnectionEvent", "cancelBubble", None, None),
    ("MIDIConnectionEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("MIDIConnectionEvent", "returnValue", None, None),
    ("MIDIConnectionEvent", "srcElement", None, None),
    ("MIDIMessageEvent", "cancelBubble", None, None),
    ("MIDIMessageEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("MIDIMessageEvent", "returnValue", None, None),
    ("MIDIMessageEvent", "srcElement", None, None),
    ("MediaEncryptedEvent", "cancelBubble", None, None),
    ("MediaEncryptedEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("MediaEncryptedEvent", "returnValue", None, None),
    ("MediaEncryptedEvent", "srcElement", None, None),
    ("MediaKeyMessageEvent", "cancelBubble", None, None),
    ("MediaKeyMessageEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("MediaKeyMessageEvent", "returnValue", None, None),
    ("MediaKeyMessageEvent", "srcElement", None, None),
    ("MediaQueryList", "addListener", Some("(callback: ((this: MediaQueryList, ev: MediaQueryListEvent) => any) | null): void"), None),
    ("MediaQueryList", "removeListener", Some("(callback: ((this: MediaQueryList, ev: MediaQueryListEvent) => any) | null): void"), None),
    ("MediaQueryListEvent", "cancelBubble", None, None),
    ("MediaQueryListEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("MediaQueryListEvent", "returnValue", None, None),
    ("MediaQueryListEvent", "srcElement", None, None),
    ("MediaStreamTrackEvent", "cancelBubble", None, None),
    ("MediaStreamTrackEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("MediaStreamTrackEvent", "returnValue", None, None),
    ("MediaStreamTrackEvent", "srcElement", None, None),
    ("MessageEvent", "cancelBubble", None, None),
    ("MessageEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("MessageEvent", "initMessageEvent", Some(""), None),
    ("MessageEvent", "returnValue", None, None),
    ("MessageEvent", "srcElement", None, None),
    ("MouseEvent", "cancelBubble", None, None),
    ("MouseEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("MouseEvent", "initMouseEvent", Some("(typeArg: string, canBubbleArg: boolean, cancelableArg: boolean, viewArg: Window, detailArg: number, screenXArg: number, screenYArg: number, clientXArg: number, clientYArg: number, ctrlKeyArg: boolean, altKeyArg: boolean, shiftKeyArg: boolean, metaKeyArg: boolean, buttonArg: number, relatedTargetArg: EventTarget | null): void"), None),
    ("MouseEvent", "initUIEvent", Some("(typeArg: string, bubblesArg?: boolean, cancelableArg?: boolean, viewArg?: Window | null, detailArg?: number): void"), None),
    ("MouseEvent", "returnValue", None, None),
    ("MouseEvent", "srcElement", None, None),
    ("MouseEvent", "which", None, None),
    ("NavigateEvent", "cancelBubble", None, None),
    ("NavigateEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("NavigateEvent", "returnValue", None, None),
    ("NavigateEvent", "srcElement", None, None),
    ("NavigationCurrentEntryChangeEvent", "cancelBubble", None, None),
    ("NavigationCurrentEntryChangeEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("NavigationCurrentEntryChangeEvent", "returnValue", None, None),
    ("NavigationCurrentEntryChangeEvent", "srcElement", None, None),
    ("OfflineAudioCompletionEvent", "cancelBubble", None, None),
    ("OfflineAudioCompletionEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("OfflineAudioCompletionEvent", "returnValue", None, None),
    ("OfflineAudioCompletionEvent", "srcElement", None, None),
    ("PageRevealEvent", "cancelBubble", None, None),
    ("PageRevealEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("PageRevealEvent", "returnValue", None, None),
    ("PageRevealEvent", "srcElement", None, None),
    ("PageSwapEvent", "cancelBubble", None, None),
    ("PageSwapEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("PageSwapEvent", "returnValue", None, None),
    ("PageSwapEvent", "srcElement", None, None),
    ("PageTransitionEvent", "cancelBubble", None, None),
    ("PageTransitionEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("PageTransitionEvent", "returnValue", None, None),
    ("PageTransitionEvent", "srcElement", None, None),
    ("PaymentMethodChangeEvent", "cancelBubble", None, None),
    ("PaymentMethodChangeEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("PaymentMethodChangeEvent", "returnValue", None, None),
    ("PaymentMethodChangeEvent", "srcElement", None, None),
    ("PaymentRequestUpdateEvent", "cancelBubble", None, None),
    ("PaymentRequestUpdateEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("PaymentRequestUpdateEvent", "returnValue", None, None),
    ("PaymentRequestUpdateEvent", "srcElement", None, None),
    ("Performance", "navigation", None, None),
    ("Performance", "timing", None, None),
    ("PictureInPictureEvent", "cancelBubble", None, None),
    ("PictureInPictureEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("PictureInPictureEvent", "returnValue", None, None),
    ("PictureInPictureEvent", "srcElement", None, None),
    ("PointerEvent", "cancelBubble", None, None),
    ("PointerEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("PointerEvent", "initMouseEvent", Some("(typeArg: string, canBubbleArg: boolean, cancelableArg: boolean, viewArg: Window, detailArg: number, screenXArg: number, screenYArg: number, clientXArg: number, clientYArg: number, ctrlKeyArg: boolean, altKeyArg: boolean, shiftKeyArg: boolean, metaKeyArg: boolean, buttonArg: number, relatedTargetArg: EventTarget | null): void"), None),
    ("PointerEvent", "initUIEvent", Some("(typeArg: string, bubblesArg?: boolean, cancelableArg?: boolean, viewArg?: Window | null, detailArg?: number): void"), None),
    ("PointerEvent", "returnValue", None, None),
    ("PointerEvent", "srcElement", None, None),
    ("PointerEvent", "which", None, None),
    ("PopStateEvent", "cancelBubble", None, None),
    ("PopStateEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("PopStateEvent", "returnValue", None, None),
    ("PopStateEvent", "srcElement", None, None),
    ("ProgressEvent", "cancelBubble", None, None),
    ("ProgressEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("ProgressEvent", "returnValue", None, None),
    ("ProgressEvent", "srcElement", None, None),
    ("PromiseRejectionEvent", "cancelBubble", None, None),
    ("PromiseRejectionEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("PromiseRejectionEvent", "returnValue", None, None),
    ("PromiseRejectionEvent", "srcElement", None, None),
    ("RTCDTMFToneChangeEvent", "cancelBubble", None, None),
    ("RTCDTMFToneChangeEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("RTCDTMFToneChangeEvent", "returnValue", None, None),
    ("RTCDTMFToneChangeEvent", "srcElement", None, None),
    ("RTCDataChannelEvent", "cancelBubble", None, None),
    ("RTCDataChannelEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("RTCDataChannelEvent", "returnValue", None, None),
    ("RTCDataChannelEvent", "srcElement", None, None),
    ("RTCErrorEvent", "cancelBubble", None, None),
    ("RTCErrorEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("RTCErrorEvent", "returnValue", None, None),
    ("RTCErrorEvent", "srcElement", None, None),
    ("RTCPeerConnectionIceErrorEvent", "cancelBubble", None, None),
    ("RTCPeerConnectionIceErrorEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("RTCPeerConnectionIceErrorEvent", "returnValue", None, None),
    ("RTCPeerConnectionIceErrorEvent", "srcElement", None, None),
    ("RTCPeerConnectionIceEvent", "cancelBubble", None, None),
    ("RTCPeerConnectionIceEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("RTCPeerConnectionIceEvent", "returnValue", None, None),
    ("RTCPeerConnectionIceEvent", "srcElement", None, None),
    ("RTCTrackEvent", "cancelBubble", None, None),
    ("RTCTrackEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("RTCTrackEvent", "returnValue", None, None),
    ("RTCTrackEvent", "srcElement", None, None),
    ("RegExp", "compile", Some("(pattern: string, flags?: string): this"), None),
    ("RegExpConstructor", "$&", None, None),
    ("RegExpConstructor", "$'", None, None),
    ("RegExpConstructor", "$+", None, None),
    ("RegExpConstructor", "$1", None, None),
    ("RegExpConstructor", "$2", None, None),
    ("RegExpConstructor", "$3", None, None),
    ("RegExpConstructor", "$4", None, None),
    ("RegExpConstructor", "$5", None, None),
    ("RegExpConstructor", "$6", None, None),
    ("RegExpConstructor", "$7", None, None),
    ("RegExpConstructor", "$8", None, None),
    ("RegExpConstructor", "$9", None, None),
    ("RegExpConstructor", "$_", None, None),
    ("RegExpConstructor", "$`", None, None),
    ("RegExpConstructor", "input", None, None),
    ("RegExpConstructor", "lastMatch", None, None),
    ("RegExpConstructor", "lastParen", None, None),
    ("RegExpConstructor", "leftContext", None, None),
    ("RegExpConstructor", "rightContext", None, None),
    ("SecurityPolicyViolationEvent", "cancelBubble", None, None),
    ("SecurityPolicyViolationEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("SecurityPolicyViolationEvent", "returnValue", None, None),
    ("SecurityPolicyViolationEvent", "srcElement", None, None),
    ("SpeechRecognitionErrorEvent", "cancelBubble", None, None),
    ("SpeechRecognitionErrorEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("SpeechRecognitionErrorEvent", "returnValue", None, None),
    ("SpeechRecognitionErrorEvent", "srcElement", None, None),
    ("SpeechRecognitionEvent", "cancelBubble", None, None),
    ("SpeechRecognitionEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("SpeechRecognitionEvent", "returnValue", None, None),
    ("SpeechRecognitionEvent", "srcElement", None, None),
    ("SpeechSynthesisErrorEvent", "cancelBubble", None, None),
    ("SpeechSynthesisErrorEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("SpeechSynthesisErrorEvent", "returnValue", None, None),
    ("SpeechSynthesisErrorEvent", "srcElement", None, None),
    ("SpeechSynthesisEvent", "cancelBubble", None, None),
    ("SpeechSynthesisEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("SpeechSynthesisEvent", "returnValue", None, None),
    ("SpeechSynthesisEvent", "srcElement", None, None),
    ("StorageEvent", "cancelBubble", None, None),
    ("StorageEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("StorageEvent", "initStorageEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean, key?: string | null, oldValue?: string | null, newValue?: string | null, url?: string | URL, storageArea?: Storage | null): void"), None),
    ("StorageEvent", "returnValue", None, None),
    ("StorageEvent", "srcElement", None, None),
    ("String", "anchor", Some("(name: string): string"), None),
    ("String", "big", Some("(): string"), None),
    ("String", "blink", Some("(): string"), None),
    ("String", "bold", Some("(): string"), None),
    ("String", "fixed", Some("(): string"), None),
    ("String", "fontcolor", Some("(color: string): string"), None),
    ("String", "fontsize", Some(""), None),
    ("String", "italics", Some("(): string"), None),
    ("String", "link", Some("(url: string): string"), None),
    ("String", "small", Some("(): string"), None),
    ("String", "strike", Some("(): string"), None),
    ("String", "sub", Some("(): string"), None),
    ("String", "substr", Some("(from: number, length?: number): string"), None),
    ("String", "sup", Some("(): string"), None),
    ("String", "trimLeft", Some("(): string"), None),
    ("String", "trimRight", Some("(): string"), None),
    ("SubmitEvent", "cancelBubble", None, None),
    ("SubmitEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("SubmitEvent", "returnValue", None, None),
    ("SubmitEvent", "srcElement", None, None),
    ("TaskPriorityChangeEvent", "cancelBubble", None, None),
    ("TaskPriorityChangeEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("TaskPriorityChangeEvent", "returnValue", None, None),
    ("TaskPriorityChangeEvent", "srcElement", None, None),
    ("TextEvent", "cancelBubble", None, None),
    ("TextEvent", "data", None, None),
    ("TextEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("TextEvent", "initTextEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean, view?: Window | null, data?: string): void"), None),
    ("TextEvent", "initUIEvent", Some("(typeArg: string, bubblesArg?: boolean, cancelableArg?: boolean, viewArg?: Window | null, detailArg?: number): void"), None),
    ("TextEvent", "returnValue", None, None),
    ("TextEvent", "srcElement", None, None),
    ("TextEvent", "which", None, None),
    ("ToggleEvent", "cancelBubble", None, None),
    ("ToggleEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("ToggleEvent", "returnValue", None, None),
    ("ToggleEvent", "srcElement", None, None),
    ("TouchEvent", "cancelBubble", None, None),
    ("TouchEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("TouchEvent", "initUIEvent", Some("(typeArg: string, bubblesArg?: boolean, cancelableArg?: boolean, viewArg?: Window | null, detailArg?: number): void"), None),
    ("TouchEvent", "returnValue", None, None),
    ("TouchEvent", "srcElement", None, None),
    ("TouchEvent", "which", None, None),
    ("TrackEvent", "cancelBubble", None, None),
    ("TrackEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("TrackEvent", "returnValue", None, None),
    ("TrackEvent", "srcElement", None, None),
    ("TransitionEvent", "cancelBubble", None, None),
    ("TransitionEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("TransitionEvent", "returnValue", None, None),
    ("TransitionEvent", "srcElement", None, None),
    ("UIEvent", "cancelBubble", None, None),
    ("UIEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("UIEvent", "initUIEvent", Some("(typeArg: string, bubblesArg?: boolean, cancelableArg?: boolean, viewArg?: Window | null, detailArg?: number): void"), None),
    ("UIEvent", "returnValue", None, None),
    ("UIEvent", "srcElement", None, None),
    ("UIEvent", "which", None, None),
    ("WebGLContextEvent", "cancelBubble", None, None),
    ("WebGLContextEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("WebGLContextEvent", "returnValue", None, None),
    ("WebGLContextEvent", "srcElement", None, None),
    ("WheelEvent", "cancelBubble", None, None),
    ("WheelEvent", "initEvent", Some("(type: string, bubbles?: boolean, cancelable?: boolean): void"), None),
    ("WheelEvent", "initMouseEvent", Some("(typeArg: string, canBubbleArg: boolean, cancelableArg: boolean, viewArg: Window, detailArg: number, screenXArg: number, screenYArg: number, clientXArg: number, clientYArg: number, ctrlKeyArg: boolean, altKeyArg: boolean, shiftKeyArg: boolean, metaKeyArg: boolean, buttonArg: number, relatedTargetArg: EventTarget | null): void"), None),
    ("WheelEvent", "initUIEvent", Some("(typeArg: string, bubblesArg?: boolean, cancelableArg?: boolean, viewArg?: Window | null, detailArg?: number): void"), None),
    ("WheelEvent", "returnValue", None, None),
    ("WheelEvent", "srcElement", None, None),
    ("WheelEvent", "which", None, None),
    ("Window", "blur", Some("(): void"), None),
    ("Window", "captureEvents", Some("(): void"), None),
    ("Window", "clientInformation", None, None),
    ("Window", "event", None, None),
    ("Window", "external", None, None),
    ("Window", "onkeypress", None, None),
    ("Window", "onorientationchange", None, None),
    ("Window", "onunload", None, None),
    ("Window", "onwebkitanimationend", None, None),
    ("Window", "onwebkitanimationiteration", None, None),
    ("Window", "onwebkitanimationstart", None, None),
    ("Window", "onwebkittransitionend", None, None),
    ("Window", "orientation", None, None),
    ("Window", "releaseEvents", Some("(): void"), None),
    ("Window", "status", None, None),
];
/// Global variables with an interface type.
pub const GLOBAL_TYPES: &[(&str, &str)] = &[
    ("Array", "ArrayConstructor"),
    ("ArrayBuffer", "ArrayBufferConstructor"),
    ("Boolean", "BooleanConstructor"),
    ("DataView", "DataViewConstructor"),
    ("Date", "DateConstructor"),
    ("Error", "ErrorConstructor"),
    ("EvalError", "EvalErrorConstructor"),
    ("Float32Array", "Float32ArrayConstructor"),
    ("Float64Array", "Float64ArrayConstructor"),
    ("Function", "FunctionConstructor"),
    ("Int16Array", "Int16ArrayConstructor"),
    ("Int32Array", "Int32ArrayConstructor"),
    ("Int8Array", "Int8ArrayConstructor"),
    ("JSON", "JSON"),
    ("Math", "Math"),
    ("Number", "NumberConstructor"),
    ("Object", "ObjectConstructor"),
    ("RangeError", "RangeErrorConstructor"),
    ("ReferenceError", "ReferenceErrorConstructor"),
    ("RegExp", "RegExpConstructor"),
    ("String", "StringConstructor"),
    ("SyntaxError", "SyntaxErrorConstructor"),
    ("TypeError", "TypeErrorConstructor"),
    ("URIError", "URIErrorConstructor"),
    ("Uint16Array", "Uint16ArrayConstructor"),
    ("Uint32Array", "Uint32ArrayConstructor"),
    ("Uint8Array", "Uint8ArrayConstructor"),
    ("Uint8ClampedArray", "Uint8ClampedArrayConstructor"),
    ("caches", "CacheStorage"),
    ("clientInformation", "Navigator"),
    ("console", "Console"),
    ("cookieStore", "CookieStore"),
    ("crypto", "Crypto"),
    ("customElements", "CustomElementRegistry"),
    ("document", "Document"),
    ("external", "External"),
    ("frames", "Window"),
    ("globalThis", "Window"),
    ("history", "History"),
    ("indexedDB", "IDBFactory"),
    ("localStorage", "Storage"),
    ("location", "Location"),
    ("locationbar", "BarProp"),
    ("menubar", "BarProp"),
    ("navigation", "Navigation"),
    ("navigator", "Navigator"),
    ("parent", "Window"),
    ("performance", "Performance"),
    ("personalbar", "BarProp"),
    ("scheduler", "Scheduler"),
    ("screen", "Screen"),
    ("scrollbars", "BarProp"),
    ("self", "Window"),
    ("sessionStorage", "Storage"),
    ("speechSynthesis", "SpeechSynthesis"),
    ("statusbar", "BarProp"),
    ("toolbar", "BarProp"),
    ("top", "Window"),
    ("window", "Window"),
];
/// Event name to event interface (DOM event maps).
pub const EVENTS: &[(&str, &str)] = &[
    ("DOMContentLoaded", "Event"),
    ("abort", "UIEvent"),
    ("animationcancel", "AnimationEvent"),
    ("animationend", "AnimationEvent"),
    ("animationiteration", "AnimationEvent"),
    ("animationstart", "AnimationEvent"),
    ("auxclick", "PointerEvent"),
    ("beforeinput", "InputEvent"),
    ("beforematch", "Event"),
    ("beforetoggle", "ToggleEvent"),
    ("blur", "FocusEvent"),
    ("cancel", "Event"),
    ("canplay", "Event"),
    ("canplaythrough", "Event"),
    ("change", "Event"),
    ("click", "PointerEvent"),
    ("close", "Event"),
    ("command", "Event"),
    ("compositionend", "CompositionEvent"),
    ("compositionstart", "CompositionEvent"),
    ("compositionupdate", "CompositionEvent"),
    ("contextlost", "Event"),
    ("contextmenu", "PointerEvent"),
    ("contextrestored", "Event"),
    ("copy", "ClipboardEvent"),
    ("cuechange", "Event"),
    ("cut", "ClipboardEvent"),
    ("dblclick", "MouseEvent"),
    ("devicemotion", "DeviceMotionEvent"),
    ("deviceorientation", "DeviceOrientationEvent"),
    ("deviceorientationabsolute", "DeviceOrientationEvent"),
    ("drag", "DragEvent"),
    ("dragend", "DragEvent"),
    ("dragenter", "DragEvent"),
    ("dragleave", "DragEvent"),
    ("dragover", "DragEvent"),
    ("dragstart", "DragEvent"),
    ("drop", "DragEvent"),
    ("durationchange", "Event"),
    ("emptied", "Event"),
    ("ended", "Event"),
    ("error", "ErrorEvent"),
    ("focus", "FocusEvent"),
    ("focusin", "FocusEvent"),
    ("focusout", "FocusEvent"),
    ("formdata", "FormDataEvent"),
    ("fullscreenchange", "Event"),
    ("fullscreenerror", "Event"),
    ("gamepadconnected", "GamepadEvent"),
    ("gamepaddisconnected", "GamepadEvent"),
    ("gotpointercapture", "PointerEvent"),
    ("input", "InputEvent"),
    ("invalid", "Event"),
    ("keydown", "KeyboardEvent"),
    ("keypress", "KeyboardEvent"),
    ("keyup", "KeyboardEvent"),
    ("load", "Event"),
    ("loadeddata", "Event"),
    ("loadedmetadata", "Event"),
    ("loadstart", "Event"),
    ("lostpointercapture", "PointerEvent"),
    ("mousedown", "MouseEvent"),
    ("mouseenter", "MouseEvent"),
    ("mouseleave", "MouseEvent"),
    ("mousemove", "MouseEvent"),
    ("mouseout", "MouseEvent"),
    ("mouseover", "MouseEvent"),
    ("mouseup", "MouseEvent"),
    ("orientationchange", "Event"),
    ("paste", "ClipboardEvent"),
    ("pause", "Event"),
    ("play", "Event"),
    ("playing", "Event"),
    ("pointercancel", "PointerEvent"),
    ("pointerdown", "PointerEvent"),
    ("pointerenter", "PointerEvent"),
    ("pointerleave", "PointerEvent"),
    ("pointerlockchange", "Event"),
    ("pointerlockerror", "Event"),
    ("pointermove", "PointerEvent"),
    ("pointerout", "PointerEvent"),
    ("pointerover", "PointerEvent"),
    ("pointerrawupdate", "Event"),
    ("pointerup", "PointerEvent"),
    ("progress", "ProgressEvent"),
    ("ratechange", "Event"),
    ("readystatechange", "Event"),
    ("reset", "Event"),
    ("resize", "UIEvent"),
    ("scroll", "Event"),
    ("scrollend", "Event"),
    ("securitypolicyviolation", "SecurityPolicyViolationEvent"),
    ("seeked", "Event"),
    ("seeking", "Event"),
    ("select", "Event"),
    ("selectionchange", "Event"),
    ("selectstart", "Event"),
    ("slotchange", "Event"),
    ("stalled", "Event"),
    ("submit", "SubmitEvent"),
    ("suspend", "Event"),
    ("timeupdate", "Event"),
    ("toggle", "ToggleEvent"),
    ("touchcancel", "TouchEvent"),
    ("touchend", "TouchEvent"),
    ("touchmove", "TouchEvent"),
    ("touchstart", "TouchEvent"),
    ("transitioncancel", "TransitionEvent"),
    ("transitionend", "TransitionEvent"),
    ("transitionrun", "TransitionEvent"),
    ("transitionstart", "TransitionEvent"),
    ("visibilitychange", "Event"),
    ("volumechange", "Event"),
    ("waiting", "Event"),
    ("webkitanimationend", "Event"),
    ("webkitanimationiteration", "Event"),
    ("webkitanimationstart", "Event"),
    ("webkittransitionend", "Event"),
    ("wheel", "WheelEvent"),
];
