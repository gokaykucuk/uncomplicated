//! Helpers owned by the `smell3` rule batch.
use oxc_ast::{AstKind, ast::*};
use oxc_semantic::NodeId;
use oxc_span::GetSpan;
use rustc_hash::FxHashMap;

use crate::Ctx;
use crate::helpers::ast::strip_ts;
use crate::helpers::types::{Ty, symbol_type, type_of};

/// Ancestors of a node, nearest (the parent) first.
///
/// Note: `Ctx::ancestors` skips the parent (oxc's `ancestor_ids` already excludes the node
/// itself), so this batch uses its own walker.
pub fn ancestors<'a, 's>(
    ctx: &Ctx<'a, 's>,
    id: NodeId,
) -> impl Iterator<Item = &'s oxc_semantic::AstNode<'a>> + 's {
    let nodes = ctx.nodes();
    nodes.ancestor_ids(id).map(move |i| nodes.get_node(i))
}

/// Nearest enclosing function or arrow of a node.
pub fn enclosing_function<'a, 's>(
    ctx: &Ctx<'a, 's>,
    id: NodeId,
) -> Option<&'s oxc_semantic::AstNode<'a>> {
    ancestors(ctx, id).find(|n| {
        matches!(
            n.kind(),
            AstKind::Function(_) | AstKind::ArrowFunctionExpression(_)
        )
    })
}

/// Type of a simple assignment target (only identifiers are resolved).
pub fn simple_target_type(ctx: &Ctx<'_, '_>, t: &SimpleAssignmentTarget<'_>) -> Ty {
    match t {
        SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => match ctx.symbol_of(id) {
            Some(sym) => symbol_type(ctx, sym, 0),
            None => Ty::Unknown,
        },
        _ => Ty::Unknown,
    }
}

/// Type of an assignment target (patterns are unknown).
pub fn target_type(ctx: &Ctx<'_, '_>, t: &AssignmentTarget<'_>) -> Ty {
    t.as_simple_assignment_target()
        .map_or(Ty::Unknown, |s| simple_target_type(ctx, s))
}

fn wrapper_ann(t: &TSType<'_>) -> bool {
    matches!(t, TSType::TSTypeReference(r)
        if matches!(&r.type_name, TSTypeName::IdentifierReference(id)
            if matches!(id.name.as_str(), "Number" | "Boolean" | "String" | "Object")))
}

fn wrapper_new(e: &Expression<'_>) -> bool {
    matches!(strip_ts(e), Expression::NewExpression(n)
        if matches!(strip_ts(&n.callee), Expression::Identifier(id)
            if matches!(id.name.as_str(), "Number" | "Boolean" | "String" | "Object")))
}

/// True if `e` may be a `Number` / `Boolean` / `String` / `Object` wrapper, which our coarse
/// inference reports as a plain [`Ty::Object`].
pub fn may_be_wrapper_object(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    match e {
        Expression::TSAsExpression(x) => wrapper_ann(&x.type_annotation),
        Expression::TSTypeAssertion(x) => wrapper_ann(&x.type_annotation),
        Expression::TSSatisfiesExpression(x) => may_be_wrapper_object(ctx, &x.expression),
        Expression::ParenthesizedExpression(p) => may_be_wrapper_object(ctx, &p.expression),
        Expression::Identifier(id) => {
            let Some(sym) = ctx.symbol_of(id) else {
                return true;
            };
            match ctx.symbol_decl(sym).kind() {
                AstKind::VariableDeclarator(v) => {
                    v.type_annotation
                        .as_ref()
                        .is_some_and(|a| wrapper_ann(&a.type_annotation))
                        || v.init.as_ref().is_some_and(wrapper_new)
                }
                AstKind::FormalParameter(p) => p
                    .type_annotation
                    .as_ref()
                    .is_some_and(|a| wrapper_ann(&a.type_annotation)),
                _ => true,
            }
        }
        e => wrapper_new(e),
    }
}

/// Known to be an object type in TypeScript's sense: not a primitive, not a union, not a `Date`
/// and not a primitive wrapper.
pub fn is_definitely_object(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    match type_of(ctx, e) {
        Ty::Array
        | Ty::Tuple
        | Ty::Function
        | Ty::Class
        | Ty::Map
        | Ty::Set
        | Ty::RegExp
        | Ty::Error
        | Ty::Promise => true,
        Ty::Object => !may_be_wrapper_object(ctx, e),
        _ => false,
    }
}

/// True if `e` is known to evaluate to a plain function (function expression, arrow, function
/// declaration or a never-reassigned variable initialized with one). Classes are excluded.
pub fn is_function_value(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    match strip_ts(e) {
        Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_) => true,
        Expression::Identifier(id) => {
            let Some(sym) = ctx.symbol_of(id) else {
                return false;
            };
            match ctx.symbol_decl(sym).kind() {
                AstKind::Function(_) => !ctx.scoping().symbol_is_mutated(sym),
                AstKind::VariableDeclarator(v) => {
                    v.type_annotation.is_none()
                        && !ctx.scoping().symbol_is_mutated(sym)
                        && matches!(
                            v.init.as_ref().map(strip_ts),
                            Some(
                                Expression::FunctionExpression(_)
                                    | Expression::ArrowFunctionExpression(_)
                            )
                        )
                }
                _ => false,
            }
        }
        _ => false,
    }
}

// ---- syntactic completion analysis ---------------------------------------------------------

/// ESLint's notion of a "simple constant" truthy loop test (a literal whose value is truthy).
pub fn is_truthy_literal(e: &Expression<'_>) -> bool {
    match e {
        Expression::BooleanLiteral(b) => b.value,
        Expression::NumericLiteral(n) => n.value != 0.0 && !n.value.is_nan(),
        Expression::StringLiteral(s) => !s.value.is_empty(),
        Expression::RegExpLiteral(_) => true,
        _ => false,
    }
}

/// True if a `break` (or, with `with_continue`, a `continue`) in `s` targets the enclosing
/// construct: unlabeled jumps not nested in an inner loop/switch, or jumps to `label`.
pub fn has_jump(s: &Statement<'_>, label: Option<&str>, with_continue: bool, nested: bool) -> bool {
    let rec = |s: &Statement<'_>, nested: bool| has_jump(s, label, with_continue, nested);
    match s {
        Statement::BreakStatement(b) => match &b.label {
            Some(l) => Some(l.name.as_str()) == label,
            None => !nested,
        },
        Statement::ContinueStatement(c) if with_continue => match &c.label {
            Some(l) => Some(l.name.as_str()) == label,
            None => !nested,
        },
        Statement::BlockStatement(b) => reachable(&b.body).iter().any(|s| rec(s, nested)),
        Statement::IfStatement(i) => {
            rec(&i.consequent, nested) || i.alternate.as_ref().is_some_and(|a| rec(a, nested))
        }
        Statement::LabeledStatement(l) => rec(&l.body, nested),
        Statement::WithStatement(w) => rec(&w.body, nested),
        Statement::TryStatement(t) => {
            t.block.body.iter().any(|s| rec(s, nested))
                || t.handler
                    .as_ref()
                    .is_some_and(|h| h.body.body.iter().any(|s| rec(s, nested)))
                || t.finalizer
                    .as_ref()
                    .is_some_and(|f| f.body.iter().any(|s| rec(s, nested)))
        }
        Statement::WhileStatement(w) => rec(&w.body, true),
        Statement::DoWhileStatement(w) => rec(&w.body, true),
        Statement::ForStatement(w) => rec(&w.body, true),
        Statement::ForInStatement(w) => rec(&w.body, true),
        Statement::ForOfStatement(w) => rec(&w.body, true),
        Statement::SwitchStatement(sw) => sw
            .cases
            .iter()
            .any(|c| reachable(&c.consequent).iter().any(|s| rec(s, true))),
        _ => false,
    }
}

/// Statements up to (and including) the first unconditional `return` / `throw`; what follows
/// is dead code (e.g. a `break` after a `return` in a `switch` case).
fn reachable<'s, 'a>(stmts: &'s [Statement<'a>]) -> &'s [Statement<'a>] {
    match stmts.iter().position(|s| terminates(s, None, true)) {
        Some(i) => &stmts[..=i],
        None => stmts,
    }
}

/// True if executing the statement list can never complete normally (every path returns,
/// throws, or loops forever). Exhaustiveness of a `switch` without `default` is unknown: with
/// `exh` set, such a switch whose last case cannot complete is treated as not completing.
pub fn list_terminates(stmts: &[Statement<'_>], exh: bool) -> bool {
    stmts.iter().any(|s| terminates(s, None, exh))
}

/// See [`list_terminates`]; `label` is the label directly attached to `s`, if any.
pub fn terminates(s: &Statement<'_>, label: Option<&str>, exh: bool) -> bool {
    match s {
        Statement::ReturnStatement(_) | Statement::ThrowStatement(_) => true,
        Statement::BlockStatement(b) => {
            list_terminates(&b.body, exh)
                && (label.is_none() || !b.body.iter().any(|s| has_jump(s, label, false, true)))
        }
        Statement::IfStatement(i) => {
            i.alternate
                .as_ref()
                .is_some_and(|a| terminates(a, None, exh))
                && terminates(&i.consequent, None, exh)
        }
        Statement::LabeledStatement(l) => {
            let name = Some(l.label.name.as_str());
            terminates(&l.body, name, exh) && !has_jump(&l.body, name, false, true)
        }
        Statement::WithStatement(w) => terminates(&w.body, None, exh),
        Statement::TryStatement(t) => {
            if t.finalizer
                .as_ref()
                .is_some_and(|f| list_terminates(&f.body, exh))
            {
                return true;
            }
            list_terminates(&t.block.body, exh)
                && t.handler
                    .as_ref()
                    .is_none_or(|h| list_terminates(&h.body.body, exh))
        }
        Statement::WhileStatement(w) => {
            is_truthy_literal(&w.test) && !has_jump(&w.body, label, false, false)
        }
        Statement::ForStatement(f) => {
            f.test.as_ref().is_none_or(is_truthy_literal) && !has_jump(&f.body, label, false, false)
        }
        Statement::DoWhileStatement(d) => {
            !has_jump(&d.body, label, false, false)
                && (is_truthy_literal(&d.test)
                    || (terminates(&d.body, None, exh) && !has_jump(&d.body, label, true, false)))
        }
        Statement::SwitchStatement(sw) => {
            let has_default = sw.cases.iter().any(|c| c.test.is_none());
            if !(has_default || exh) || sw.cases.is_empty() {
                return false;
            }
            let breaks = sw.cases.iter().any(|c| {
                reachable(&c.consequent)
                    .iter()
                    .any(|s| has_jump(s, label, false, false))
            });
            !breaks
                && sw
                    .cases
                    .last()
                    .is_some_and(|c| list_terminates(&c.consequent, exh))
        }
        _ => false,
    }
}

/// The JSDoc block comment (`/** ... */`) attached to a function, if any: the comment directly
/// preceding the function or the declaration / property / method that holds it.
pub fn function_jsdoc<'a>(ctx: &Ctx<'a, '_>, func: NodeId) -> Option<&'a str> {
    let mut anchor = ctx.node(func);
    loop {
        let parent = ctx.parent(anchor.id());
        match parent.kind() {
            AstKind::VariableDeclarator(_)
            | AstKind::VariableDeclaration(_)
            | AstKind::MethodDefinition(_)
            | AstKind::ObjectProperty(_)
            | AstKind::PropertyDefinition(_)
            | AstKind::ExportNamedDeclaration(_)
            | AstKind::ExportDeclaration(_)
            | AstKind::ExportDefaultDeclaration(_)
            | AstKind::ParenthesizedExpression(_) => anchor = parent,
            AstKind::AssignmentExpression(_) | AstKind::ExpressionStatement(_) => anchor = parent,
            _ => break,
        }
    }
    let start = anchor.kind().span().start;
    let comments = ctx.comments();
    let i = comments.partition_point(|c| c.span.end <= start);
    let c = comments.get(i.checked_sub(1)?)?;
    if !c.is_jsdoc() {
        return None;
    }
    if crate::helpers::ast::token_before(ctx, start).is_some_and(|t| t.end > c.span.start) {
        return None;
    }
    Some(ctx.comment_text(c))
}

/// Return statements of each function (keyed by the function's node id), excluding those of
/// nested functions. Built in one pass over the file.
pub fn returns_by_function<'a>(
    ctx: &Ctx<'a, '_>,
) -> FxHashMap<NodeId, Vec<&'a ReturnStatement<'a>>> {
    let mut map: FxHashMap<NodeId, Vec<&'a ReturnStatement<'a>>> = FxHashMap::default();
    for node in ctx.nodes().iter() {
        if let AstKind::ReturnStatement(r) = node.kind()
            && let Some(f) = enclosing_function(ctx, node.id())
        {
            map.entry(f.id()).or_default().push(r);
        }
    }
    map
}

/// Types that are certainly neither thenables nor `void`.
pub fn certainly_not_thenable(t: &Ty) -> bool {
    t.all(|t| {
        matches!(
            t,
            Ty::String
                | Ty::Number
                | Ty::BigInt
                | Ty::Boolean
                | Ty::Null
                | Ty::Undefined
                | Ty::Symbol
                | Ty::Array
                | Ty::Tuple
                | Ty::Function
                | Ty::Class
                | Ty::RegExp
                | Ty::Date
                | Ty::Map
                | Ty::Set
                | Ty::Error
        )
    })
}

// ---- project context -------------------------------------------------------------------------

/// SonarJS's `detectedEsYear` (`esLibToYear` of the program's `lib`): `None` means no restriction
/// (`esnext`). The `lib` is the tsconfig's when set, otherwise `es<max(target, node version)>`.
pub fn detected_es_year(ctx: &Ctx<'_, '_>) -> Option<u32> {
    let p = ctx.file.project;
    if !p.ts_lib.is_empty() {
        if p.ts_lib.iter().any(|l| l.starts_with("esnext")) {
            return None;
        }
        return p.ts_lib.iter().filter_map(|l| lib_year(l)).max();
    }
    let mut years = Vec::new();
    if let Some(t) = &p.ts_target {
        match t.as_str() {
            "es3" | "es5" => years.push(2020),
            t => years.extend(lib_year(t).filter(|y| (2015..=2030).contains(y))),
        }
    }
    let node = p
        .deps
        .get("@types/node")
        .and_then(|v| max_node_major(v))
        .or_else(|| p.engines_node.as_deref().and_then(max_node_major));
    if let Some(major) = node {
        years.push(node_to_es(major));
    }
    years.into_iter().max()
}

/// `es2020`, `es2015.promise` → year.
fn lib_year(lib: &str) -> Option<u32> {
    let rest = lib.strip_prefix("es")?;
    let digits = rest.get(..4)?;
    if !digits.bytes().all(|b| b.is_ascii_digit()) || rest.len() > 4 && !rest[4..].starts_with('.')
    {
        return None;
    }
    digits.parse().ok()
}

/// Highest plausible Node.js major mentioned in a version range (SonarJS `parseMaxNodeMajor`).
fn max_node_major(v: &str) -> Option<u32> {
    let v = v.trim();
    if v.is_empty() || v == "*" || v == "latest" {
        return None;
    }
    let mut best = None;
    let mut rest = v;
    while let Some(i) = rest.find(|c: char| c.is_ascii_digit()) {
        let s = &rest[i..];
        let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
        if let Ok(n) = s[..end].parse::<u32>()
            && (8..100).contains(&n)
        {
            best = best.max(Some(n));
        }
        // Skip the rest of a dotted version (`18.2.0` counts once).
        let mut j = end;
        while s[j..].starts_with('.') && s[j + 1..].starts_with(|c: char| c.is_ascii_digit()) {
            j += 1;
            j += s[j..]
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(s.len() - j);
        }
        rest = &s[j..];
    }
    best
}

fn node_to_es(major: u32) -> u32 {
    const NODE_TO_ES: [(u32, u32); 8] = [
        (22, 2024),
        (20, 2023),
        (18, 2022),
        (16, 2021),
        (14, 2020),
        (12, 2019),
        (10, 2018),
        (8, 2017),
    ];
    NODE_TO_ES
        .iter()
        .find(|(n, _)| major >= *n)
        .map_or(2017, |(_, y)| *y)
}

/// SonarJS `filter-ecma-version`: a rule requiring ES `required` runs unless the detected ES
/// year is lower. `downlevelable` rules (syntax TypeScript compiles down) always run on TS files.
pub fn es_version_allows(ctx: &Ctx<'_, '_>, required: u32, downlevelable: bool) -> bool {
    if ctx.is_ts() && downlevelable {
        return true;
    }
    detected_es_year(ctx).is_none_or(|y| y >= required)
}

/// A `package.json` encloses the file.
pub fn has_package_json(ctx: &Ctx<'_, '_>) -> bool {
    ctx.file.project.has_package_json
}
/// Globals predefined by SonarJS (its default ESLint environments and `sonar.javascript.globals`).
/// Sorted, for binary search.
#[rustfmt::skip]
pub static SONAR_GLOBALS: &[&str] = &[
    "$", "$$", "$A", "$F", "$H", "$R", "$break", "$continue", "$w", "AI", "AICreateMonitor",
    "AITextSession", "AbortController", "AbortPaymentEvent", "AbortSignal",
    "AbsoluteOrientationSensor", "Abstract", "AbstractRange", "Accelerometer", "Accounts",
    "AccountsClient", "AccountsCommon", "AccountsServer", "AggregateError", "Ajax", "AnalyserNode",
    "Animation", "AnimationEffect", "AnimationEvent", "AnimationPlaybackEvent", "AnimationTimeline",
    "App", "Application", "Array", "ArrayBuffer", "Assets", "AsyncDisposableStack", "Atomics",
    "Attr", "Audio", "AudioBuffer", "AudioBufferSourceNode", "AudioContext", "AudioData",
    "AudioDecoder", "AudioDestinationNode", "AudioEncoder", "AudioListener", "AudioNode",
    "AudioParam", "AudioParamMap", "AudioPlaybackStats", "AudioProcessingEvent",
    "AudioScheduledSourceNode", "AudioSession", "AudioSinkInfo", "AudioWorklet", "AudioWorkletNode",
    "AuthenticatorAssertionResponse", "AuthenticatorAttestationResponse", "AuthenticatorResponse",
    "Autocompleter", "Automation", "Backbone", "BackgroundFetchEvent", "BackgroundFetchManager",
    "BackgroundFetchRecord", "BackgroundFetchRegistration", "BackgroundFetchUpdateUIEvent",
    "BarProp", "BarcodeDetector", "BaseAudioContext", "BatteryManager", "BeforeUnloadEvent",
    "BigInt", "BigInt64Array", "BigUint64Array", "BiquadFilterNode", "Blaze", "Blob", "BlobEvent",
    "Bluetooth", "BluetoothCharacteristicProperties", "BluetoothDevice",
    "BluetoothRemoteGATTCharacteristic", "BluetoothRemoteGATTDescriptor",
    "BluetoothRemoteGATTServer", "BluetoothRemoteGATTService", "BluetoothUUID", "Boolean",
    "BroadcastChannel", "BrowserCaptureMediaStreamTrack", "Buffer", "Builder", "BulkWriteResult",
    "By", "ByteLengthQueuingStrategy", "CDATASection", "CSPViolationReportBody", "CSS",
    "CSSAnimation", "CSSConditionRule", "CSSContainerRule", "CSSCounterStyleRule",
    "CSSFontFaceDescriptors", "CSSFontFaceRule", "CSSFontFeatureValuesRule",
    "CSSFontPaletteValuesRule", "CSSFunctionDeclarations", "CSSFunctionDescriptors",
    "CSSFunctionRule", "CSSGroupingRule", "CSSImageValue", "CSSImportRule", "CSSKeyframeRule",
    "CSSKeyframesRule", "CSSKeywordValue", "CSSLayerBlockRule", "CSSLayerStatementRule",
    "CSSMarginRule", "CSSMathClamp", "CSSMathInvert", "CSSMathMax", "CSSMathMin", "CSSMathNegate",
    "CSSMathProduct", "CSSMathSum", "CSSMathValue", "CSSMatrixComponent", "CSSMediaRule",
    "CSSNamespaceRule", "CSSNestedDeclarations", "CSSNumericArray", "CSSNumericValue",
    "CSSPageDescriptors", "CSSPageRule", "CSSPerspective", "CSSPositionTryDescriptors",
    "CSSPositionTryRule", "CSSPositionValue", "CSSPropertyRule", "CSSPseudoElement", "CSSRotate",
    "CSSRule", "CSSRuleList", "CSSScale", "CSSScopeRule", "CSSSkew", "CSSSkewX", "CSSSkewY",
    "CSSStartingStyleRule", "CSSStyleDeclaration", "CSSStyleProperties", "CSSStyleRule",
    "CSSStyleSheet", "CSSStyleValue", "CSSSupportsRule", "CSSTransformComponent",
    "CSSTransformValue", "CSSTransition", "CSSTranslate", "CSSUnitValue", "CSSUnparsedValue",
    "CSSVariableReferenceValue", "CSSViewTransitionRule", "Cache", "CacheStorage",
    "CanMakePaymentEvent", "CanvasCaptureMediaStream", "CanvasCaptureMediaStreamTrack",
    "CanvasGradient", "CanvasPattern", "CanvasRenderingContext2D", "CaptureController",
    "CaretPosition", "ChannelMergerNode", "ChannelSplitterNode", "ChapterInformation",
    "CharacterBoundsUpdateEvent", "CharacterData", "Class", "Client", "Clients", "Clipboard",
    "ClipboardChangeEvent", "ClipboardEvent", "ClipboardItem", "CloseEvent", "CloseWatcher",
    "CommandEvent", "Comment", "CompositionEvent", "CompressionStream", "ConstantSourceNode",
    "ContentVisibilityAutoStateChangeEvent", "Control", "ConvolverNode", "CookieChangeEvent",
    "CookieDeprecationLabel", "CookieStore", "CookieStoreManager", "Cordova",
    "CountQueuingStrategy", "CrashReportContext", "CreateMonitor", "Credential",
    "CredentialsContainer", "CropTarget", "Crypto", "CryptoKey", "CustomElementRegistry",
    "CustomEvent", "CustomStateSet", "DDP", "DDPRateLimiter", "DDPServer", "DOMError",
    "DOMException", "DOMImplementation", "DOMMatrix", "DOMMatrixReadOnly", "DOMParser", "DOMPoint",
    "DOMPointReadOnly", "DOMQuad", "DOMRect", "DOMRectList", "DOMRectReadOnly", "DOMStringList",
    "DOMStringMap", "DOMTokenList", "DartObject", "DataTransfer", "DataTransferItem",
    "DataTransferItemList", "DataView", "Date", "DecompressionStream", "DedicatedWorkerGlobalScope",
    "DelayNode", "DelegatedInkTrailPresenter", "Deps", "DeviceMotionEvent",
    "DeviceMotionEventAcceleration", "DeviceMotionEventRotationRate", "DeviceOrientationEvent",
    "DevicePosture", "DigitalCredential", "DisposableStack", "Document", "DocumentFragment",
    "DocumentPictureInPicture", "DocumentPictureInPictureEvent", "DocumentTimeline", "DocumentType",
    "DragEvent", "Draggable", "Draggables", "Droppables", "DynamicsCompressorNode", "EJSON",
    "EditContext", "Effect", "Element", "ElementInternals", "Email", "EncodedAudioChunk",
    "EncodedVideoChunk", "Enumerable", "Error", "ErrorEvent", "EvalError", "Event", "EventCounts",
    "EventSource", "EventTarget", "ExtendableCookieChangeEvent", "ExtendableEvent",
    "ExtendableMessageEvent", "External", "EyeDropper", "FeaturePolicy", "FederatedCredential",
    "Fence", "FencedFrameConfig", "FetchEvent", "FetchLaterResult", "Field", "File", "FileList",
    "FileReader", "FileReaderSync", "FileSystem", "FileSystemDirectoryEntry",
    "FileSystemDirectoryHandle", "FileSystemDirectoryReader", "FileSystemEntry",
    "FileSystemFileEntry", "FileSystemFileHandle", "FileSystemHandle", "FileSystemObserver",
    "FileSystemSyncAccessHandle", "FileSystemWritableFileStream", "FinalizationRegistry",
    "Float16Array", "Float32Array", "Float64Array", "FocusEvent", "FontData", "FontFace",
    "FontFaceSet", "FontFaceSetLoadEvent", "Form", "FormData", "FormDataEvent", "FragmentDirective",
    "Function", "GM", "GM_addElement", "GM_addStyle", "GM_addValueChangeListener", "GM_cookie",
    "GM_deleteValue", "GM_deleteValues", "GM_download", "GM_getResourceText", "GM_getResourceURL",
    "GM_getTab", "GM_getTabs", "GM_getValue", "GM_getValues", "GM_info", "GM_listValues", "GM_log",
    "GM_notification", "GM_openInTab", "GM_registerMenuCommand", "GM_removeValueChangeListener",
    "GM_saveTab", "GM_setClipboard", "GM_setValue", "GM_setValues", "GM_unregisterMenuCommand",
    "GM_xmlhttpRequest", "GPU", "GPUAdapter", "GPUAdapterInfo", "GPUBindGroup",
    "GPUBindGroupLayout", "GPUBuffer", "GPUBufferUsage", "GPUCanvasContext", "GPUColorWrite",
    "GPUCommandBuffer", "GPUCommandEncoder", "GPUCompilationInfo", "GPUCompilationMessage",
    "GPUComputePassEncoder", "GPUComputePipeline", "GPUDevice", "GPUDeviceLostInfo", "GPUError",
    "GPUExternalTexture", "GPUInternalError", "GPUMapMode", "GPUOutOfMemoryError",
    "GPUPipelineError", "GPUPipelineLayout", "GPUQuerySet", "GPUQueue", "GPURenderBundle",
    "GPURenderBundleEncoder", "GPURenderPassEncoder", "GPURenderPipeline", "GPUSampler",
    "GPUShaderModule", "GPUShaderStage", "GPUSupportedFeatures", "GPUSupportedLimits", "GPUTexture",
    "GPUTextureUsage", "GPUTextureView", "GPUUncapturedErrorEvent", "GPUValidationError",
    "GainNode", "Gamepad", "GamepadAxisMoveEvent", "GamepadButton", "GamepadButtonEvent",
    "GamepadEvent", "GamepadHapticActuator", "GamepadPose", "Geolocation", "GeolocationCoordinates",
    "GeolocationPosition", "GeolocationPositionError", "GravitySensor", "Gyroscope", "HID",
    "HIDConnectionEvent", "HIDDevice", "HIDInputReportEvent", "HTMLAllCollection",
    "HTMLAnchorElement", "HTMLAreaElement", "HTMLAudioElement", "HTMLBRElement", "HTMLBaseElement",
    "HTMLBodyElement", "HTMLButtonElement", "HTMLCameraElement", "HTMLCanvasElement",
    "HTMLCollection", "HTMLDListElement", "HTMLDataElement", "HTMLDataListElement",
    "HTMLDetailsElement", "HTMLDialogElement", "HTMLDirectoryElement", "HTMLDivElement",
    "HTMLDocument", "HTMLElement", "HTMLEmbedElement", "HTMLFencedFrameElement",
    "HTMLFieldSetElement", "HTMLFontElement", "HTMLFormControlsCollection", "HTMLFormElement",
    "HTMLFrameElement", "HTMLFrameSetElement", "HTMLGeolocationElement", "HTMLHRElement",
    "HTMLHeadElement", "HTMLHeadingElement", "HTMLHtmlElement", "HTMLIFrameElement",
    "HTMLImageElement", "HTMLInputElement", "HTMLLIElement", "HTMLLabelElement",
    "HTMLLegendElement", "HTMLLinkElement", "HTMLMapElement", "HTMLMarqueeElement",
    "HTMLMediaElement", "HTMLMenuElement", "HTMLMetaElement", "HTMLMeterElement",
    "HTMLMicrophoneElement", "HTMLModElement", "HTMLOListElement", "HTMLObjectElement",
    "HTMLOptGroupElement", "HTMLOptionElement", "HTMLOptionsCollection", "HTMLOutputElement",
    "HTMLParagraphElement", "HTMLParamElement", "HTMLPictureElement", "HTMLPreElement",
    "HTMLProgressElement", "HTMLQuoteElement", "HTMLScriptElement", "HTMLSelectElement",
    "HTMLSelectedContentElement", "HTMLSlotElement", "HTMLSourceElement", "HTMLSpanElement",
    "HTMLStyleElement", "HTMLTableCaptionElement", "HTMLTableCellElement", "HTMLTableColElement",
    "HTMLTableElement", "HTMLTableRowElement", "HTMLTableSectionElement", "HTMLTemplateElement",
    "HTMLTextAreaElement", "HTMLTimeElement", "HTMLTitleElement", "HTMLTrackElement",
    "HTMLUListElement", "HTMLUnknownElement", "HTMLUserMediaElement", "HTMLVideoElement", "HTTP",
    "Hash", "HashChangeEvent", "Headers", "Highlight", "HighlightRegistry", "History", "IDBCursor",
    "IDBCursorWithValue", "IDBDatabase", "IDBFactory", "IDBIndex", "IDBKeyRange", "IDBObjectStore",
    "IDBOpenDBRequest", "IDBRecord", "IDBRequest", "IDBTransaction", "IDBVersionChangeEvent",
    "IIRFilterNode", "ISODate", "IdentityCredential", "IdentityCredentialError", "IdentityProvider",
    "IdleDeadline", "IdleDetector", "Image", "ImageBitmap", "ImageBitmapRenderingContext",
    "ImageCapture", "ImageData", "ImageDecoder", "ImageTrack", "ImageTrackList", "Infinity", "Ink",
    "InputDeviceCapabilities", "InputDeviceInfo", "InputEvent", "Insertion", "InstallEvent",
    "Int16Array", "Int32Array", "Int8Array", "IntegrityViolationReportBody",
    "InteractionContentfulPaint", "InterestEvent", "IntersectionObserver",
    "IntersectionObserverEntry", "Intl", "Iterator", "JSAdapter", "JSON", "Java", "JavaImporter",
    "Keyboard", "KeyboardEvent", "KeyboardLayoutMap", "KeyframeEffect", "LanguageDetector",
    "LanguageModel", "LargestContentfulPaint", "LaunchParams", "LaunchQueue", "LayoutShift",
    "LayoutShiftAttribution", "Library", "LinearAccelerationSensor", "Location", "Lock",
    "LockManager", "Log", "MIDIAccess", "MIDIConnectionEvent", "MIDIInput", "MIDIInputMap",
    "MIDIMessageEvent", "MIDIOutput", "MIDIOutputMap", "MIDIPort", "Map", "Match", "Math",
    "MathMLAnchorElement", "MathMLElement", "MediaCapabilities", "MediaCapabilitiesInfo",
    "MediaDeviceInfo", "MediaDevices", "MediaElementAudioSourceNode", "MediaEncryptedEvent",
    "MediaError", "MediaKeyError", "MediaKeyMessageEvent", "MediaKeySession", "MediaKeyStatusMap",
    "MediaKeySystemAccess", "MediaKeys", "MediaList", "MediaMetadata", "MediaQueryList",
    "MediaQueryListEvent", "MediaRecorder", "MediaRecorderErrorEvent", "MediaSession",
    "MediaSource", "MediaSourceHandle", "MediaStream", "MediaStreamAudioDestinationNode",
    "MediaStreamAudioSourceNode", "MediaStreamEvent", "MediaStreamTrack",
    "MediaStreamTrackAudioSourceNode", "MediaStreamTrackAudioStats", "MediaStreamTrackEvent",
    "MediaStreamTrackGenerator", "MediaStreamTrackProcessor", "MediaStreamTrackVideoStats",
    "MessageChannel", "MessageEvent", "MessagePort", "Meteor", "MimeType", "MimeTypeArray",
    "ModelContext", "ModelGenericSession", "ModelManager", "Mongo", "MongoInternals", "MouseEvent",
    "MutationEvent", "MutationObserver", "MutationRecord", "NaN", "NamedNodeMap", "NavigateEvent",
    "Navigation", "NavigationActivation", "NavigationCurrentEntryChangeEvent",
    "NavigationDestination", "NavigationHistoryEntry", "NavigationPrecommitController",
    "NavigationPreloadManager", "NavigationTransition", "Navigator", "NavigatorLogin",
    "NavigatorManagedData", "NavigatorUAData", "NetworkInformation", "Node", "NodeFilter",
    "NodeIterator", "NodeList", "NodeRange", "NotRestoredReasonDetails", "NotRestoredReasons",
    "Notification", "NotificationEvent", "NotifyPaintEvent", "Npm", "Number", "NumberInt",
    "NumberLong", "OTPCredential", "ObjC", "Object", "ObjectId", "ObjectRange", "ObjectSpecifier",
    "Observable", "OfflineAudioCompletionEvent", "OfflineAudioContext", "OffscreenCanvas",
    "OffscreenCanvasRenderingContext2D", "OpaqueRange", "OpenLayers", "Option", "OrientationSensor",
    "Origin", "OscillatorNode", "OverconstrainedError", "PERSISTENT", "Package", "Packages",
    "PageRevealEvent", "PageSwapEvent", "PageTransitionEvent", "PannerNode", "PasswordCredential",
    "Path", "Path2D", "PaymentAddress", "PaymentManager", "PaymentMethodChangeEvent",
    "PaymentRequest", "PaymentRequestEvent", "PaymentRequestUpdateEvent", "PaymentResponse",
    "Performance", "PerformanceElementTiming", "PerformanceEntry", "PerformanceEventTiming",
    "PerformanceLongAnimationFrameTiming", "PerformanceLongTaskTiming", "PerformanceMark",
    "PerformanceMarkConditional", "PerformanceMeasure", "PerformanceNavigation",
    "PerformanceNavigationTiming", "PerformanceObserver", "PerformanceObserverEntryList",
    "PerformancePaintTiming", "PerformanceResourceTiming", "PerformanceScriptTiming",
    "PerformanceServerTiming", "PerformanceSoftNavigation", "PerformanceTiming",
    "PerformanceTimingConfidence", "PeriodicSyncEvent", "PeriodicSyncManager", "PeriodicWave",
    "PeriodicalExecuter", "PermissionStatus", "Permissions", "PermissionsPolicy",
    "PictureInPictureEvent", "PictureInPictureWindow", "PlanCache", "Plugin", "PluginArray",
    "PointerEvent", "PopStateEvent", "Position", "Presentation", "PresentationAvailability",
    "PresentationConnection", "PresentationConnectionAvailableEvent",
    "PresentationConnectionCloseEvent", "PresentationConnectionList", "PresentationReceiver",
    "PresentationRequest", "PressureObserver", "PressureRecord", "ProcessingInstruction",
    "Profiler", "Progress", "ProgressEvent", "Promise", "PromiseRejectionEvent",
    "ProtectedAudience", "Prototype", "Proxy", "PublicKeyCredential", "PushEvent", "PushManager",
    "PushMessageData", "PushSubscription", "PushSubscriptionChangeEvent", "PushSubscriptionOptions",
    "QUnit", "QuotaExceededError", "RTCCertificate", "RTCDTMFSender", "RTCDTMFToneChangeEvent",
    "RTCDataChannel", "RTCDataChannelEvent", "RTCDtlsTransport", "RTCEncodedAudioFrame",
    "RTCEncodedVideoFrame", "RTCError", "RTCErrorEvent", "RTCIceCandidate", "RTCIceCandidatePair",
    "RTCIceTransport", "RTCPeerConnection", "RTCPeerConnectionIceErrorEvent",
    "RTCPeerConnectionIceEvent", "RTCRtpReceiver", "RTCRtpScriptTransform",
    "RTCRtpScriptTransformer", "RTCRtpSender", "RTCRtpTransceiver", "RTCSctpTransport",
    "RTCSessionDescription", "RTCStatsReport", "RTCTrackEvent", "RTCTransformEvent",
    "RadioNodeList", "Random", "Range", "RangeError", "ReactiveDict", "ReactiveVar",
    "ReadableByteStreamController", "ReadableStream", "ReadableStreamBYOBReader",
    "ReadableStreamBYOBRequest", "ReadableStreamDefaultController", "ReadableStreamDefaultReader",
    "Ref", "ReferenceError", "Reflect", "RegExp", "RelativeOrientationSensor", "RemotePlayback",
    "ReportBody", "ReportingObserver", "Request", "ResizeObserver", "ResizeObserverEntry",
    "ResizeObserverSize", "Response", "RestrictionTarget", "Router", "SVGAElement", "SVGAngle",
    "SVGAnimateElement", "SVGAnimateMotionElement", "SVGAnimateTransformElement",
    "SVGAnimatedAngle", "SVGAnimatedBoolean", "SVGAnimatedEnumeration", "SVGAnimatedInteger",
    "SVGAnimatedLength", "SVGAnimatedLengthList", "SVGAnimatedNumber", "SVGAnimatedNumberList",
    "SVGAnimatedPreserveAspectRatio", "SVGAnimatedRect", "SVGAnimatedString",
    "SVGAnimatedTransformList", "SVGAnimationElement", "SVGCircleElement", "SVGClipPathElement",
    "SVGComponentTransferFunctionElement", "SVGDefsElement", "SVGDescElement", "SVGElement",
    "SVGEllipseElement", "SVGFEBlendElement", "SVGFEColorMatrixElement",
    "SVGFEComponentTransferElement", "SVGFECompositeElement", "SVGFEConvolveMatrixElement",
    "SVGFEDiffuseLightingElement", "SVGFEDisplacementMapElement", "SVGFEDistantLightElement",
    "SVGFEDropShadowElement", "SVGFEFloodElement", "SVGFEFuncAElement", "SVGFEFuncBElement",
    "SVGFEFuncGElement", "SVGFEFuncRElement", "SVGFEGaussianBlurElement", "SVGFEImageElement",
    "SVGFEMergeElement", "SVGFEMergeNodeElement", "SVGFEMorphologyElement", "SVGFEOffsetElement",
    "SVGFEPointLightElement", "SVGFESpecularLightingElement", "SVGFESpotLightElement",
    "SVGFETileElement", "SVGFETurbulenceElement", "SVGFilterElement", "SVGForeignObjectElement",
    "SVGGElement", "SVGGeometryElement", "SVGGradientElement", "SVGGraphicsElement",
    "SVGImageElement", "SVGLength", "SVGLengthList", "SVGLineElement", "SVGLinearGradientElement",
    "SVGMPathElement", "SVGMarkerElement", "SVGMaskElement", "SVGMatrix", "SVGMetadataElement",
    "SVGNumber", "SVGNumberList", "SVGPathElement", "SVGPatternElement", "SVGPoint", "SVGPointList",
    "SVGPolygonElement", "SVGPolylineElement", "SVGPreserveAspectRatio", "SVGRadialGradientElement",
    "SVGRect", "SVGRectElement", "SVGSVGElement", "SVGScriptElement", "SVGSetElement",
    "SVGStopElement", "SVGStringList", "SVGStyleElement", "SVGSwitchElement", "SVGSymbolElement",
    "SVGTSpanElement", "SVGTextContentElement", "SVGTextElement", "SVGTextPathElement",
    "SVGTextPositioningElement", "SVGTitleElement", "SVGTransform", "SVGTransformList",
    "SVGUnitTypes", "SVGUseElement", "SVGViewElement", "Sanitizer", "Scheduler", "Scheduling",
    "Screen", "ScreenDetailed", "ScreenDetails", "ScreenOrientation", "ScriptProcessorNode",
    "Scriptaculous", "ScrollTimeline", "SecurityPolicyViolationEvent", "Selection", "Selector",
    "Sensor", "SensorErrorEvent", "Serial", "SerialPort", "ServiceConfiguration", "ServiceWorker",
    "ServiceWorkerContainer", "ServiceWorkerGlobalScope", "ServiceWorkerRegistration", "Session",
    "Set", "ShadowRoot", "SharedArrayBuffer", "SharedStorage", "SharedStorageAppendMethod",
    "SharedStorageClearMethod", "SharedStorageDeleteMethod", "SharedStorageModifierMethod",
    "SharedStorageSetMethod", "SharedStorageWorklet", "SharedWorker", "ShellString", "SnapEvent",
    "Sortable", "SortableObserver", "Sound", "SourceBuffer", "SourceBufferList", "Spacebars",
    "SpeechGrammar", "SpeechGrammarList", "SpeechRecognition", "SpeechRecognitionAlternative",
    "SpeechRecognitionErrorEvent", "SpeechRecognitionEvent", "SpeechRecognitionPhrase",
    "SpeechRecognitionResult", "SpeechRecognitionResultList", "SpeechSynthesis",
    "SpeechSynthesisErrorEvent", "SpeechSynthesisEvent", "SpeechSynthesisUtterance",
    "SpeechSynthesisVoice", "StaticRange", "StereoPannerNode", "Storage", "StorageBucket",
    "StorageBucketManager", "StorageEvent", "StorageManager", "String", "StylePropertyMap",
    "StylePropertyMapReadOnly", "StyleSheet", "StyleSheetList", "SubmitEvent", "Subscriber",
    "SubtleCrypto", "Summarizer", "SuppressedError", "Symbol", "SyncEvent", "SyncManager",
    "SyntaxError", "TEMPORARY", "TaskAttributionTiming", "TaskController",
    "TaskPriorityChangeEvent", "TaskSignal", "Template", "Temporal", "Text", "TextDecoder",
    "TextDecoderStream", "TextEncoder", "TextEncoderStream", "TextEvent", "TextFormat",
    "TextFormatUpdateEvent", "TextMetrics", "TextTrack", "TextTrackCue", "TextTrackCueList",
    "TextTrackList", "TextUpdateEvent", "TimeEvent", "TimeRanges", "TimelineTrigger",
    "TimelineTriggerRange", "TimelineTriggerRangeList", "Tinytest", "Toggle", "ToggleEvent",
    "Touch", "TouchEvent", "TouchList", "TrackEvent", "Tracker", "TransformStream",
    "TransformStreamDefaultController", "TransitionEvent", "Translator", "TreeWalker",
    "TrustedHTML", "TrustedParserOptions", "TrustedScript", "TrustedScriptURL", "TrustedTypePolicy",
    "TrustedTypePolicyFactory", "Try", "TypeError", "UI", "UIEvent", "URIError", "URL",
    "URLPattern", "URLSearchParams", "USB", "USBAlternateInterface", "USBConfiguration",
    "USBConnectionEvent", "USBDevice", "USBEndpoint", "USBInTransferResult", "USBInterface",
    "USBIsochronousInTransferPacket", "USBIsochronousInTransferResult",
    "USBIsochronousOutTransferPacket", "USBIsochronousOutTransferResult", "USBOutTransferResult",
    "UUID", "Uint16Array", "Uint32Array", "Uint8Array", "Uint8ClampedArray", "UserActivation",
    "Utils", "VTTCue", "VTTRegion", "ValidityState", "VideoColorSpace", "VideoDecoder",
    "VideoEncoder", "VideoFrame", "VideoPlaybackQuality", "ViewTimeline", "ViewTransition",
    "ViewTransitionTypeSet", "Viewport", "VirtualKeyboard", "VirtualKeyboardGeometryChangeEvent",
    "VisibilityStateEntry", "VisualViewport", "WGSLLanguageFeatures", "WakeLock",
    "WakeLockSentinel", "WaveShaperNode", "WeakMap", "WeakRef", "WeakSet", "WebApp",
    "WebAppInternals", "WebAssembly", "WebGL2RenderingContext", "WebGLActiveInfo", "WebGLBuffer",
    "WebGLContextEvent", "WebGLFramebuffer", "WebGLObject", "WebGLProgram", "WebGLQuery",
    "WebGLRenderbuffer", "WebGLRenderingContext", "WebGLSampler", "WebGLShader",
    "WebGLShaderPrecisionFormat", "WebGLSync", "WebGLTexture", "WebGLTransformFeedback",
    "WebGLUniformLocation", "WebGLVertexArrayObject", "WebMCPEvent", "WebPage", "WebSocket",
    "WebSocketError", "WebSocketStream", "WebTransport", "WebTransportBidirectionalStream",
    "WebTransportDatagramDuplexStream", "WebTransportDatagramsWritable", "WebTransportError",
    "WebTransportReceiveStream", "WebTransportSendGroup", "WebTransportSendStream", "WheelEvent",
    "Window", "WindowClient", "WindowControlsOverlay", "WindowControlsOverlayGeometryChangeEvent",
    "Worker", "WorkerGlobalScope", "WorkerLocation", "WorkerNavigator", "Worklet", "WritableStream",
    "WritableStreamDefaultController", "WritableStreamDefaultWriter", "WriteResult", "XMLDocument",
    "XMLHttpRequest", "XMLHttpRequestEventTarget", "XMLHttpRequestUpload", "XMLSerializer",
    "XPathEvaluator", "XPathExpression", "XPathResult", "XRAnchor", "XRAnchorSet",
    "XRBoundedReferenceSpace", "XRCPUDepthInformation", "XRCamera", "XRCompositionLayer",
    "XRCubeLayer", "XRCylinderLayer", "XRDOMOverlayState", "XRDepthInformation", "XREquirectLayer",
    "XRFrame", "XRHand", "XRHitTestResult", "XRHitTestSource", "XRInputSource",
    "XRInputSourceArray", "XRInputSourceEvent", "XRInputSourcesChangeEvent", "XRJointPose",
    "XRJointSpace", "XRLayer", "XRLayerEvent", "XRLightEstimate", "XRLightProbe", "XRPlane",
    "XRPlaneSet", "XRPose", "XRProjectionLayer", "XRQuadLayer", "XRRay", "XRReferenceSpace",
    "XRReferenceSpaceEvent", "XRRenderState", "XRRigidTransform", "XRSession", "XRSessionEvent",
    "XRSpace", "XRSubImage", "XRSystem", "XRTransientInputHitTestResult",
    "XRTransientInputHitTestSource", "XRView", "XRViewerPose", "XRViewport",
    "XRVisibilityMaskChangeEvent", "XRWebGLBinding", "XRWebGLDepthInformation", "XRWebGLLayer",
    "XRWebGLSubImage", "XSLTProcessor", "_", "__DIR__", "__FILE__", "__LINE__", "__dirname",
    "__filename", "_isWindows", "_rand", "addEventListener", "advanceClock", "after", "afterAll",
    "afterEach", "ai", "alert", "andThen", "angular", "asyncTest", "atob", "atom", "before",
    "beforeAll", "beforeEach", "blur", "browser", "btoa", "by", "caches", "cancelAnimationFrame",
    "cancelIdleCallback", "casper", "cat", "cd", "check", "chmod", "chrome", "clearImmediate",
    "clearInterval", "clearTimeout", "click", "clientInformation", "clients", "cloneInto", "close",
    "closed", "cmd", "com", "config", "confirm", "connect", "console", "context", "cookieStore",
    "cp", "crashReport", "createImageBitmap", "createObjectIn", "credentialless",
    "crossOriginIsolated", "crypto", "currentPath", "currentRouteName", "currentURL",
    "customElements", "d3", "db", "decodeURI", "decodeURIComponent", "deepEqual", "define", "delay",
    "describe", "devicePixelRatio", "dijit", "dirs", "dispatchEvent", "document",
    "documentPictureInPicture", "dojo", "dojox", "echo", "edu", "element", "encodeURI",
    "encodeURIComponent", "env", "equal", "error", "errorCode", "escape", "eval", "event", "exec",
    "exit", "expect", "expectAsync", "exportFunction", "exports", "external", "fail",
    "fakeClearInterval", "fakeClearTimeout", "fakeSetInterval", "fakeSetTimeout", "fdescribe",
    "fence", "fetch", "fetchLater", "fillIn", "find", "findAll", "findWithAssert", "fit", "focus",
    "fonts", "frameElement", "frames", "getComputedStyle", "getHostName", "getMemInfo",
    "getScreenDetails", "getSelection", "global", "globalThis", "goog", "google", "grep", "head",
    "history", "hostname", "importScripts", "indexedDB", "innerHeight", "innerWidth", "isFinite",
    "isNaN", "isSecureContext", "it", "jQuery", "jasmine", "java", "javafx", "javax", "jest",
    "keyEvent", "launchQueue", "length", "listFiles", "ln", "load", "loadWithNewGlobal",
    "localStorage", "location", "locationbar", "ls", "matchMedia", "md5sumFile", "menubar", "mkdir",
    "mocha", "model", "module", "moment", "moveBy", "moveTo", "mv", "name", "navigation",
    "navigator", "notDeepEqual", "notEqual", "notOk", "notPropEqual", "notStrictEqual",
    "offscreenBuffering", "ok", "onabort", "onabortpayment", "onactivate", "onafterprint",
    "onanimationcancel", "onanimationend", "onanimationiteration", "onanimationstart",
    "onappinstalled", "onauxclick", "onbackgroundfetchabort", "onbackgroundfetchclick",
    "onbackgroundfetchfail", "onbackgroundfetchsuccess", "onbeforeinput", "onbeforeinstallprompt",
    "onbeforematch", "onbeforeprint", "onbeforetoggle", "onbeforeunload", "onbeforexrselect",
    "onblur", "oncancel", "oncanmakepayment", "oncanplay", "oncanplaythrough", "onchange",
    "onclick", "onclose", "oncommand", "oncontentvisibilityautostatechange", "oncontextlost",
    "oncontextmenu", "oncontextrestored", "oncookiechange", "oncopy", "oncuechange", "oncut",
    "ondblclick", "ondevicemotion", "ondeviceorientation", "ondeviceorientationabsolute", "ondrag",
    "ondragend", "ondragenter", "ondragleave", "ondragover", "ondragstart", "ondrop",
    "ondurationchange", "onemptied", "onended", "onerror", "onfetch", "onfocus", "onformdata",
    "ongamepadconnected", "ongamepaddisconnected", "ongotpointercapture", "onhashchange", "oninput",
    "oninstall", "oninvalid", "onkeydown", "onkeypress", "onkeyup", "onlanguagechange", "onload",
    "onloadeddata", "onloadedmetadata", "onloadstart", "onlostpointercapture", "onmessage",
    "onmessageerror", "onmousedown", "onmouseenter", "onmouseleave", "onmousemove", "onmouseout",
    "onmouseover", "onmouseup", "onmousewheel", "onnotificationclick", "onnotificationclose",
    "onoffline", "ononline", "onpagehide", "onpagereveal", "onpageshow", "onpageswap", "onpaste",
    "onpause", "onpaymentrequest", "onperiodicsync", "onplay", "onplaying", "onpointercancel",
    "onpointerdown", "onpointerenter", "onpointerleave", "onpointermove", "onpointerout",
    "onpointerover", "onpointerrawupdate", "onpointerup", "onpopstate", "onprogress", "onpush",
    "onpushsubscriptionchange", "onratechange", "onrejectionhandled", "onreset", "onresize",
    "onrtctransform", "onscroll", "onscrollend", "onscrollsnapchange", "onscrollsnapchanging",
    "onsearch", "onsecuritypolicyviolation", "onseeked", "onseeking", "onselect",
    "onselectionchange", "onselectstart", "onslotchange", "onstalled", "onstorage", "onsubmit",
    "onsuspend", "onsync", "ontimeupdate", "ontoggle", "ontransitioncancel", "ontransitionend",
    "ontransitionrun", "ontransitionstart", "onunhandledrejection", "onunload", "onvolumechange",
    "onwaiting", "onwheel", "open", "opener", "opr", "org", "origin", "originAgentCluster",
    "outerHeight", "outerWidth", "pageXOffset", "pageYOffset", "parent", "parseFloat", "parseInt",
    "pauseTest", "pending", "performance", "personalbar", "phantom", "popd", "postMessage", "print",
    "printjson", "process", "prompt", "propEqual", "protractor", "pushd", "pwd", "queryLocalFonts",
    "queueMicrotask", "quit", "raises", "registration", "removeEventListener", "removeFile",
    "reportError", "requestAnimationFrame", "requestIdleCallback", "requestResize", "require",
    "resetTimeouts", "resizeBy", "resizeTo", "resumeTest", "rm", "rs", "run", "runs", "sap",
    "scheduler", "screen", "screenLeft", "screenTop", "screenX", "screenY", "scroll", "scrollBy",
    "scrollTo", "scrollX", "scrollY", "scrollbars", "sed", "self", "serviceWorker",
    "sessionStorage", "set", "setImmediate", "setInterval", "setTimeout", "setup", "sh", "share",
    "sharedStorage", "showDirectoryPicker", "showOpenFilePicker", "showSaveFilePicker",
    "skipWaiting", "sort", "specify", "speechSynthesis", "spyOn", "spyOnAllFunctions",
    "spyOnProperty", "start", "status", "statusbar", "stop", "strictEqual", "structuredClone",
    "styleMedia", "suite", "suiteSetup", "suiteTeardown", "tail", "teardown", "tempdir", "test",
    "throwUnless", "throwUnlessAsync", "throws", "toolbar", "top", "touch", "triggerEvent",
    "trustedTypes", "undefined", "unescape", "uniq", "unsafeWindow", "version", "viewport", "visit",
    "visualViewport", "wait", "waits", "waitsFor", "waitsForPromise", "webkitRequestFileSystem",
    "webkitRequestFileSystemSync", "webkitResolveLocalFileSystemSyncURL",
    "webkitResolveLocalFileSystemURL", "when", "which", "window", "xcontext", "xdescribe", "xit",
    "xspecify", "xtest",
];

/// SonarJS has no TypeScript program (hence no type information) for scripts embedded in HTML,
/// and usually none for Vue single-file components; type-dependent rules stay silent there.
pub fn has_type_services(ctx: &Ctx<'_, '_>) -> bool {
    let p = ctx.file.path;
    !(p.ends_with(".html") || p.ends_with(".htm") || p.ends_with(".vue"))
}
