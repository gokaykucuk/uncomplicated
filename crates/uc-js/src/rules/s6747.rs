//! JSX elements should not use unknown properties and attributes
//!
//! `react/no-unknown-property` (React latest) merged with `jsx-a11y/aria-props`. Framework props
//! Sonar ignores depending on project dependencies (`css`, `sx`, `tw`, `jsx`, `global`) are always
//! ignored since dependencies are not known here.
use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule {
    ignore: Vec<String>,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            ignore: p.list("ignore", &[]),
        }
    }
}

const FRAMEWORK_PROPS: &[&str] = &["jsx", "global", "css", "sx", "tw"];

/// aria-query 5.3 attribute names, in its order.
const ARIA_QUERY: &[&str] = &[
    "aria-activedescendant",
    "aria-atomic",
    "aria-autocomplete",
    "aria-braillelabel",
    "aria-brailleroledescription",
    "aria-busy",
    "aria-checked",
    "aria-colcount",
    "aria-colindex",
    "aria-colspan",
    "aria-controls",
    "aria-current",
    "aria-describedby",
    "aria-description",
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
    "aria-modal",
    "aria-multiline",
    "aria-multiselectable",
    "aria-orientation",
    "aria-owns",
    "aria-placeholder",
    "aria-posinset",
    "aria-pressed",
    "aria-readonly",
    "aria-relevant",
    "aria-required",
    "aria-roledescription",
    "aria-rowcount",
    "aria-rowindex",
    "aria-rowspan",
    "aria-selected",
    "aria-setsize",
    "aria-sort",
    "aria-valuemax",
    "aria-valuemin",
    "aria-valuenow",
    "aria-valuetext",
];

const DOM_ATTRIBUTE_NAMES: &[(&str, &str)] = &[
    ("accept-charset", "acceptCharset"),
    ("class", "className"),
    ("http-equiv", "httpEquiv"),
    ("crossorigin", "crossOrigin"),
    ("for", "htmlFor"),
    ("nomodule", "noModule"),
];
const SVGDOM_ATTRIBUTE_NAMES: &[(&str, &str)] = &[
    ("accent-height", "accentHeight"),
    ("alignment-baseline", "alignmentBaseline"),
    ("arabic-form", "arabicForm"),
    ("baseline-shift", "baselineShift"),
    ("cap-height", "capHeight"),
    ("clip-path", "clipPath"),
    ("clip-rule", "clipRule"),
    ("color-interpolation", "colorInterpolation"),
    ("color-interpolation-filters", "colorInterpolationFilters"),
    ("color-profile", "colorProfile"),
    ("color-rendering", "colorRendering"),
    ("dominant-baseline", "dominantBaseline"),
    ("enable-background", "enableBackground"),
    ("fill-opacity", "fillOpacity"),
    ("fill-rule", "fillRule"),
    ("flood-color", "floodColor"),
    ("flood-opacity", "floodOpacity"),
    ("font-family", "fontFamily"),
    ("font-size", "fontSize"),
    ("font-size-adjust", "fontSizeAdjust"),
    ("font-stretch", "fontStretch"),
    ("font-style", "fontStyle"),
    ("font-variant", "fontVariant"),
    ("font-weight", "fontWeight"),
    ("glyph-name", "glyphName"),
    ("glyph-orientation-horizontal", "glyphOrientationHorizontal"),
    ("glyph-orientation-vertical", "glyphOrientationVertical"),
    ("horiz-adv-x", "horizAdvX"),
    ("horiz-origin-x", "horizOriginX"),
    ("image-rendering", "imageRendering"),
    ("letter-spacing", "letterSpacing"),
    ("lighting-color", "lightingColor"),
    ("marker-end", "markerEnd"),
    ("marker-mid", "markerMid"),
    ("marker-start", "markerStart"),
    ("overline-position", "overlinePosition"),
    ("overline-thickness", "overlineThickness"),
    ("paint-order", "paintOrder"),
    ("panose-1", "panose1"),
    ("pointer-events", "pointerEvents"),
    ("rendering-intent", "renderingIntent"),
    ("shape-rendering", "shapeRendering"),
    ("stop-color", "stopColor"),
    ("stop-opacity", "stopOpacity"),
    ("strikethrough-position", "strikethroughPosition"),
    ("strikethrough-thickness", "strikethroughThickness"),
    ("stroke-dasharray", "strokeDasharray"),
    ("stroke-dashoffset", "strokeDashoffset"),
    ("stroke-linecap", "strokeLinecap"),
    ("stroke-linejoin", "strokeLinejoin"),
    ("stroke-miterlimit", "strokeMiterlimit"),
    ("stroke-opacity", "strokeOpacity"),
    ("stroke-width", "strokeWidth"),
    ("text-anchor", "textAnchor"),
    ("text-decoration", "textDecoration"),
    ("text-rendering", "textRendering"),
    ("underline-position", "underlinePosition"),
    ("underline-thickness", "underlineThickness"),
    ("unicode-bidi", "unicodeBidi"),
    ("unicode-range", "unicodeRange"),
    ("units-per-em", "unitsPerEm"),
    ("v-alphabetic", "vAlphabetic"),
    ("v-hanging", "vHanging"),
    ("v-ideographic", "vIdeographic"),
    ("v-mathematical", "vMathematical"),
    ("vector-effect", "vectorEffect"),
    ("vert-adv-y", "vertAdvY"),
    ("vert-origin-x", "vertOriginX"),
    ("vert-origin-y", "vertOriginY"),
    ("word-spacing", "wordSpacing"),
    ("writing-mode", "writingMode"),
    ("x-height", "xHeight"),
    ("xlink:actuate", "xlinkActuate"),
    ("xlink:arcrole", "xlinkArcrole"),
    ("xlink:href", "xlinkHref"),
    ("xlink:role", "xlinkRole"),
    ("xlink:show", "xlinkShow"),
    ("xlink:title", "xlinkTitle"),
    ("xlink:type", "xlinkType"),
    ("xml:base", "xmlBase"),
    ("xml:lang", "xmlLang"),
    ("xml:space", "xmlSpace"),
];
const ATTRIBUTE_TAGS_MAP: &[(&str, &[&str])] = &[
    ("abbr", &["th", "td"]),
    ("charset", &["meta"]),
    ("checked", &["input"]),
    (
        "crossOrigin",
        &["script", "img", "video", "audio", "link", "image"],
    ),
    ("displaystyle", &["math"]),
    ("download", &["a", "area"]),
    (
        "fill",
        &[
            "altGlyph",
            "circle",
            "ellipse",
            "g",
            "line",
            "marker",
            "mask",
            "path",
            "polygon",
            "polyline",
            "rect",
            "svg",
            "symbol",
            "text",
            "textPath",
            "tref",
            "tspan",
            "use",
            "animate",
            "animateColor",
            "animateMotion",
            "animateTransform",
            "set",
        ],
    ),
    ("focusable", &["svg"]),
    ("imageSizes", &["link"]),
    ("imageSrcSet", &["link"]),
    ("property", &["meta"]),
    ("viewBox", &["marker", "pattern", "svg", "symbol", "view"]),
    ("as", &["link"]),
    (
        "align",
        &[
            "applet", "caption", "col", "colgroup", "hr", "iframe", "img", "table", "tbody", "td",
            "tfoot", "th", "thead", "tr",
        ],
    ),
    (
        "valign",
        &[
            "tr", "td", "th", "thead", "tbody", "tfoot", "colgroup", "col",
        ],
    ),
    ("noModule", &["script"]),
    ("onAbort", &["audio", "video"]),
    ("onCancel", &["dialog"]),
    ("onCanPlay", &["audio", "video"]),
    ("onCanPlayThrough", &["audio", "video"]),
    ("onClose", &["dialog"]),
    ("onDurationChange", &["audio", "video"]),
    ("onEmptied", &["audio", "video"]),
    ("onEncrypted", &["audio", "video"]),
    ("onEnded", &["audio", "video"]),
    (
        "onError",
        &[
            "audio", "video", "img", "link", "source", "script", "picture", "iframe",
        ],
    ),
    (
        "onLoad",
        &[
            "script", "img", "link", "picture", "iframe", "object", "source",
        ],
    ),
    ("onLoadedData", &["audio", "video"]),
    ("onLoadedMetadata", &["audio", "video"]),
    ("onLoadStart", &["audio", "video"]),
    ("onPause", &["audio", "video"]),
    ("onPlay", &["audio", "video"]),
    ("onPlaying", &["audio", "video"]),
    ("onProgress", &["audio", "video"]),
    ("onRateChange", &["audio", "video"]),
    ("onResize", &["audio", "video"]),
    ("onSeeked", &["audio", "video"]),
    ("onSeeking", &["audio", "video"]),
    ("onStalled", &["audio", "video"]),
    ("onSuspend", &["audio", "video"]),
    ("onTimeUpdate", &["audio", "video"]),
    ("onVolumeChange", &["audio", "video"]),
    ("onWaiting", &["audio", "video"]),
    ("autoPictureInPicture", &["video"]),
    ("controls", &["audio", "video"]),
    ("controlsList", &["audio", "video"]),
    ("disablePictureInPicture", &["video"]),
    ("disableRemotePlayback", &["audio", "video"]),
    ("loop", &["audio", "video"]),
    ("muted", &["audio", "video"]),
    ("playsInline", &["video"]),
    ("allowFullScreen", &["iframe", "video"]),
    ("webkitAllowFullScreen", &["iframe", "video"]),
    ("mozAllowFullScreen", &["iframe", "video"]),
    ("poster", &["video"]),
    ("preload", &["audio", "video"]),
    ("scrolling", &["iframe"]),
    ("returnValue", &["dialog"]),
    ("webkitDirectory", &["input"]),
    ("shadowrootmode", &["template"]),
    ("shadowrootclonable", &["template"]),
    ("shadowrootdelegatesfocus", &["template"]),
    ("shadowrootserializable", &["template"]),
    ("transform-origin", &["rect"]),
];
const DOM_PROPERTY_NAMES_ONE_WORD: &[&str] = &[
    "dir",
    "draggable",
    "hidden",
    "id",
    "lang",
    "nonce",
    "part",
    "slot",
    "style",
    "title",
    "translate",
    "inert",
    "accept",
    "action",
    "allow",
    "alt",
    "as",
    "async",
    "buffered",
    "capture",
    "challenge",
    "cite",
    "code",
    "cols",
    "content",
    "coords",
    "csp",
    "data",
    "decoding",
    "default",
    "defer",
    "disabled",
    "form",
    "headers",
    "height",
    "high",
    "href",
    "icon",
    "importance",
    "integrity",
    "kind",
    "label",
    "language",
    "loading",
    "list",
    "loop",
    "low",
    "manifest",
    "max",
    "media",
    "method",
    "min",
    "multiple",
    "muted",
    "name",
    "open",
    "optimum",
    "pattern",
    "ping",
    "placeholder",
    "poster",
    "preload",
    "profile",
    "rel",
    "required",
    "reversed",
    "role",
    "rows",
    "sandbox",
    "scope",
    "seamless",
    "selected",
    "shape",
    "size",
    "sizes",
    "span",
    "src",
    "start",
    "step",
    "summary",
    "target",
    "type",
    "value",
    "width",
    "wmode",
    "wrap",
    "accumulate",
    "additive",
    "alphabetic",
    "amplitude",
    "ascent",
    "azimuth",
    "bbox",
    "begin",
    "bias",
    "by",
    "clip",
    "color",
    "cursor",
    "cx",
    "cy",
    "d",
    "decelerate",
    "descent",
    "direction",
    "display",
    "divisor",
    "dur",
    "dx",
    "dy",
    "elevation",
    "end",
    "exponent",
    "fill",
    "filter",
    "format",
    "from",
    "fr",
    "fx",
    "fy",
    "g1",
    "g2",
    "hanging",
    "height",
    "hreflang",
    "ideographic",
    "in",
    "in2",
    "intercept",
    "k",
    "k1",
    "k2",
    "k3",
    "k4",
    "kerning",
    "local",
    "mask",
    "mode",
    "offset",
    "opacity",
    "operator",
    "order",
    "orient",
    "orientation",
    "origin",
    "overflow",
    "path",
    "ping",
    "points",
    "r",
    "radius",
    "rel",
    "restart",
    "result",
    "rotate",
    "rx",
    "ry",
    "scale",
    "seed",
    "slope",
    "spacing",
    "speed",
    "stemh",
    "stemv",
    "string",
    "stroke",
    "to",
    "transform",
    "u1",
    "u2",
    "unicode",
    "values",
    "version",
    "visibility",
    "widths",
    "x",
    "x1",
    "x2",
    "xmlns",
    "y",
    "y1",
    "y2",
    "z",
    "property",
    "ref",
    "key",
    "children",
    "results",
    "security",
    "controls",
    "popover",
    "popovertarget",
    "popovertargetaction",
];
const DOM_PROPERTY_NAMES_TWO_WORDS: &[&str] = &[
    "accessKey",
    "autoCapitalize",
    "autoFocus",
    "contentEditable",
    "enterKeyHint",
    "exportParts",
    "inputMode",
    "itemID",
    "itemRef",
    "itemProp",
    "itemScope",
    "itemType",
    "spellCheck",
    "tabIndex",
    "acceptCharset",
    "autoComplete",
    "autoPlay",
    "border",
    "cellPadding",
    "cellSpacing",
    "classID",
    "codeBase",
    "colSpan",
    "contextMenu",
    "dateTime",
    "encType",
    "formAction",
    "formEncType",
    "formMethod",
    "formNoValidate",
    "formTarget",
    "frameBorder",
    "hrefLang",
    "httpEquiv",
    "imageSizes",
    "imageSrcSet",
    "isMap",
    "keyParams",
    "keyType",
    "marginHeight",
    "marginWidth",
    "maxLength",
    "mediaGroup",
    "minLength",
    "noValidate",
    "onAnimationEnd",
    "onAnimationIteration",
    "onAnimationStart",
    "onBlur",
    "onChange",
    "onClick",
    "onContextMenu",
    "onCopy",
    "onCompositionEnd",
    "onCompositionStart",
    "onCompositionUpdate",
    "onCut",
    "onDoubleClick",
    "onDrag",
    "onDragEnd",
    "onDragEnter",
    "onDragExit",
    "onDragLeave",
    "onError",
    "onFocus",
    "onInput",
    "onKeyDown",
    "onKeyPress",
    "onKeyUp",
    "onLoad",
    "onWheel",
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
    "onPaste",
    "onScroll",
    "onSelect",
    "onSubmit",
    "onBeforeToggle",
    "onToggle",
    "onTransitionEnd",
    "radioGroup",
    "readOnly",
    "referrerPolicy",
    "rowSpan",
    "srcDoc",
    "srcLang",
    "srcSet",
    "useMap",
    "fetchPriority",
    "crossOrigin",
    "accentHeight",
    "alignmentBaseline",
    "arabicForm",
    "attributeName",
    "attributeType",
    "baseFrequency",
    "baselineShift",
    "baseProfile",
    "calcMode",
    "capHeight",
    "clipPathUnits",
    "clipPath",
    "clipRule",
    "colorInterpolation",
    "colorInterpolationFilters",
    "colorProfile",
    "colorRendering",
    "contentScriptType",
    "contentStyleType",
    "diffuseConstant",
    "dominantBaseline",
    "edgeMode",
    "enableBackground",
    "fillOpacity",
    "fillRule",
    "filterRes",
    "filterUnits",
    "floodColor",
    "floodOpacity",
    "fontFamily",
    "fontSize",
    "fontSizeAdjust",
    "fontStretch",
    "fontStyle",
    "fontVariant",
    "fontWeight",
    "glyphName",
    "glyphOrientationHorizontal",
    "glyphOrientationVertical",
    "glyphRef",
    "gradientTransform",
    "gradientUnits",
    "horizAdvX",
    "horizOriginX",
    "imageRendering",
    "kernelMatrix",
    "kernelUnitLength",
    "keyPoints",
    "keySplines",
    "keyTimes",
    "lengthAdjust",
    "letterSpacing",
    "lightingColor",
    "limitingConeAngle",
    "markerEnd",
    "markerMid",
    "markerStart",
    "markerHeight",
    "markerUnits",
    "markerWidth",
    "maskContentUnits",
    "maskUnits",
    "mathematical",
    "numOctaves",
    "overlinePosition",
    "overlineThickness",
    "panose1",
    "paintOrder",
    "pathLength",
    "patternContentUnits",
    "patternTransform",
    "patternUnits",
    "pointerEvents",
    "pointsAtX",
    "pointsAtY",
    "pointsAtZ",
    "preserveAlpha",
    "preserveAspectRatio",
    "primitiveUnits",
    "referrerPolicy",
    "refX",
    "refY",
    "rendering-intent",
    "repeatCount",
    "repeatDur",
    "requiredExtensions",
    "requiredFeatures",
    "shapeRendering",
    "specularConstant",
    "specularExponent",
    "spreadMethod",
    "startOffset",
    "stdDeviation",
    "stitchTiles",
    "stopColor",
    "stopOpacity",
    "strikethroughPosition",
    "strikethroughThickness",
    "strokeDasharray",
    "strokeDashoffset",
    "strokeLinecap",
    "strokeLinejoin",
    "strokeMiterlimit",
    "strokeOpacity",
    "strokeWidth",
    "surfaceScale",
    "systemLanguage",
    "tableValues",
    "targetX",
    "targetY",
    "textAnchor",
    "textDecoration",
    "textRendering",
    "textLength",
    "transformOrigin",
    "underlinePosition",
    "underlineThickness",
    "unicodeBidi",
    "unicodeRange",
    "unitsPerEm",
    "vAlphabetic",
    "vHanging",
    "vIdeographic",
    "vMathematical",
    "vectorEffect",
    "vertAdvY",
    "vertOriginX",
    "vertOriginY",
    "viewBox",
    "viewTarget",
    "wordSpacing",
    "writingMode",
    "xHeight",
    "xChannelSelector",
    "xlinkActuate",
    "xlinkArcrole",
    "xlinkHref",
    "xlinkRole",
    "xlinkShow",
    "xlinkTitle",
    "xlinkType",
    "xmlBase",
    "xmlLang",
    "xmlnsXlink",
    "xmlSpace",
    "yChannelSelector",
    "zoomAndPan",
    "autoCorrect",
    "autoSave",
    "className",
    "dangerouslySetInnerHTML",
    "defaultValue",
    "defaultChecked",
    "htmlFor",
    "onBeforeInput",
    "onChange",
    "onInvalid",
    "onReset",
    "onTouchCancel",
    "onTouchEnd",
    "onTouchMove",
    "onTouchStart",
    "suppressContentEditableWarning",
    "suppressHydrationWarning",
    "onAbort",
    "onCanPlay",
    "onCanPlayThrough",
    "onDurationChange",
    "onEmptied",
    "onEncrypted",
    "onEnded",
    "onLoadedData",
    "onLoadedMetadata",
    "onLoadStart",
    "onPause",
    "onPlay",
    "onPlaying",
    "onProgress",
    "onRateChange",
    "onResize",
    "onSeeked",
    "onSeeking",
    "onStalled",
    "onSuspend",
    "onTimeUpdate",
    "onVolumeChange",
    "onWaiting",
    "onCopyCapture",
    "onCutCapture",
    "onPasteCapture",
    "onCompositionEndCapture",
    "onCompositionStartCapture",
    "onCompositionUpdateCapture",
    "onFocusCapture",
    "onBlurCapture",
    "onChangeCapture",
    "onBeforeInputCapture",
    "onInputCapture",
    "onResetCapture",
    "onSubmitCapture",
    "onInvalidCapture",
    "onLoadCapture",
    "onErrorCapture",
    "onKeyDownCapture",
    "onKeyPressCapture",
    "onKeyUpCapture",
    "onAbortCapture",
    "onCanPlayCapture",
    "onCanPlayThroughCapture",
    "onDurationChangeCapture",
    "onEmptiedCapture",
    "onEncryptedCapture",
    "onEndedCapture",
    "onLoadedDataCapture",
    "onLoadedMetadataCapture",
    "onLoadStartCapture",
    "onPauseCapture",
    "onPlayCapture",
    "onPlayingCapture",
    "onProgressCapture",
    "onRateChangeCapture",
    "onSeekedCapture",
    "onSeekingCapture",
    "onStalledCapture",
    "onSuspendCapture",
    "onTimeUpdateCapture",
    "onVolumeChangeCapture",
    "onWaitingCapture",
    "onSelectCapture",
    "onTouchCancelCapture",
    "onTouchEndCapture",
    "onTouchMoveCapture",
    "onTouchStartCapture",
    "onScrollCapture",
    "onWheelCapture",
    "onAnimationEndCapture",
    "onAnimationIteration",
    "onAnimationStartCapture",
    "onTransitionEndCapture",
    "onAuxClick",
    "onAuxClickCapture",
    "onClickCapture",
    "onContextMenuCapture",
    "onDoubleClickCapture",
    "onDragCapture",
    "onDragEndCapture",
    "onDragEnterCapture",
    "onDragExitCapture",
    "onDragLeaveCapture",
    "onDragOverCapture",
    "onDragStartCapture",
    "onDropCapture",
    "onMouseDown",
    "onMouseDownCapture",
    "onMouseMoveCapture",
    "onMouseOutCapture",
    "onMouseOverCapture",
    "onMouseUpCapture",
    "autoPictureInPicture",
    "controlsList",
    "disablePictureInPicture",
    "disableRemotePlayback",
    "popoverTarget",
    "popoverTargetAction",
];
const DOM_PROPERTIES_IGNORE_CASE: &[&str] = &[
    "charset",
    "allowFullScreen",
    "webkitAllowFullScreen",
    "mozAllowFullScreen",
    "webkitDirectory",
    "popoverTarget",
    "popoverTargetAction",
];
const ARIA_PROPERTIES: &[&str] = &[
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
    "aria-autocomplete",
    "aria-checked",
    "aria-expanded",
    "aria-level",
    "aria-modal",
    "aria-multiline",
    "aria-multiselectable",
    "aria-orientation",
    "aria-placeholder",
    "aria-pressed",
    "aria-readonly",
    "aria-required",
    "aria-selected",
    "aria-sort",
    "aria-valuemax",
    "aria-valuemin",
    "aria-valuenow",
    "aria-valuetext",
    "aria-activedescendant",
    "aria-colcount",
    "aria-colindex",
    "aria-colindextext",
    "aria-colspan",
    "aria-posinset",
    "aria-rowcount",
    "aria-rowindex",
    "aria-rowindextext",
    "aria-rowspan",
    "aria-setsize",
];
const REACT_ON_PROPS: &[&str] = &[
    "onGotPointerCapture",
    "onGotPointerCaptureCapture",
    "onLostPointerCapture",
    "onLostPointerCapture",
    "onLostPointerCaptureCapture",
    "onPointerCancel",
    "onPointerCancelCapture",
    "onPointerDown",
    "onPointerDownCapture",
    "onPointerEnter",
    "onPointerEnterCapture",
    "onPointerLeave",
    "onPointerLeaveCapture",
    "onPointerMove",
    "onPointerMoveCapture",
    "onPointerOut",
    "onPointerOutCapture",
    "onPointerOver",
    "onPointerOverCapture",
    "onPointerUp",
    "onPointerUpCapture",
];

/// Optimal string alignment distance (what `damerau-levenshtein` computes).
fn osa(a: &[u8], b: &[u8]) -> usize {
    let (n, m) = (a.len(), b.len());
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(d[i - 2][j - 2] + cost);
            }
            d[i][j] = v;
        }
    }
    d[n][m]
}

fn aria_message(name: &str) -> String {
    let upper = name.to_ascii_uppercase();
    let mut cands: Vec<(usize, &str)> = ARIA_QUERY
        .iter()
        .map(|w| (osa(upper.as_bytes(), w.to_ascii_uppercase().as_bytes()), *w))
        .filter(|(d, _)| *d <= 2)
        .collect();
    cands.sort_by_key(|(d, _)| *d);
    let suggestions: Vec<&str> = cands.into_iter().take(2).map(|(_, w)| w).collect();
    let mut msg = format!("{name}: This attribute is an invalid ARIA attribute.");
    if !suggestions.is_empty() {
        msg.push_str(&format!(" Did you mean to use {}?", suggestions.join(",")));
    }
    msg
}

fn lookup<'s>(table: &'s [(&'s str, &'s str)], name: &str) -> Option<&'s str> {
    table.iter().find(|(k, _)| *k == name).map(|(_, v)| *v)
}

fn standard_name(name: &str) -> Option<&'static str> {
    lookup(DOM_ATTRIBUTE_NAMES, name)
        .or_else(|| lookup(SVGDOM_ATTRIBUTE_NAMES, name))
        .or_else(|| {
            DOM_PROPERTY_NAMES_TWO_WORDS
                .iter()
                .chain(DOM_PROPERTY_NAMES_ONE_WORD)
                .chain(REACT_ON_PROPS)
                .chain(std::iter::once(&"precedence"))
                .find(|e| e.eq_ignore_ascii_case(name))
                .copied()
        })
}

fn is_valid_data_attribute(name: &str) -> bool {
    name.starts_with("data-")
        && !name.contains(':')
        && !name
            .get(..8)
            .is_some_and(|p| p.eq_ignore_ascii_case("data-xml"))
}

fn report(ctx: &Ctx<'_, '_>, span: Span, msg: String) {
    ctx.report(span, msg);
}

fn uses_vue(ctx: &Ctx<'_, '_>) -> bool {
    ctx.file.path.ends_with(".vue")
        || ctx
            .nodes()
            .program()
            .body
            .iter()
            .any(|s| matches!(s, Statement::ImportDeclaration(d) if d.source.value == "vue"))
}

fn uses_stencil(ctx: &Ctx<'_, '_>) -> bool {
    ctx.nodes().program().body.iter().any(|s| {
        matches!(s, Statement::ImportDeclaration(d) if d.source.value == "@stencil/core" && !d.import_kind.is_type())
    })
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXAttribute]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::JSXAttribute(attr) = node.kind() else {
            return;
        };
        let actual = ctx.text(attr.name.span());
        // Sonar doesn't raise on Vue files (JSX in `.vue` or with Vue imports).
        if uses_stencil(ctx) || uses_vue(ctx) {
            return;
        }
        // jsx-a11y/aria-props, on every element.
        if actual.starts_with("aria-") && !ARIA_QUERY.contains(&actual) {
            report(ctx, attr.span, aria_message(actual));
            return;
        }
        if self.ignore.iter().any(|i| i == actual) || FRAMEWORK_PROPS.contains(&actual) {
            return;
        }
        let AstKind::JSXOpeningElement(el) = ctx.parent_kind(node.id()) else {
            return;
        };
        let name = DOM_PROPERTIES_IGNORE_CASE
            .iter()
            .find(|e| e.eq_ignore_ascii_case(actual))
            .copied()
            .unwrap_or(actual);
        if is_valid_data_attribute(name) || ARIA_PROPERTIES.contains(&name) {
            return;
        }
        let JSXElementName::Identifier(tag) = &el.name else {
            return;
        };
        let tag = tag.name.as_str();
        if tag == "fbt" || tag == "fbs" {
            return;
        }
        // /^[a-z][^-]*$/ and no `is` attribute (custom elements).
        if !tag.as_bytes()[0].is_ascii_lowercase()
            || tag.contains('-')
            || find_attr(el, "is").is_some()
        {
            return;
        }
        if let Some((_, allowed)) = ATTRIBUTE_TAGS_MAP.iter().find(|(k, _)| *k == name) {
            if !allowed.contains(&tag) {
                report(
                    ctx,
                    attr.span,
                    format!(
                        "Invalid property '{actual}' found on tag '{tag}', but it is only allowed on: {}",
                        allowed.join(", ")
                    ),
                );
            }
            return;
        }
        match standard_name(name) {
            Some(std) if std == name => {}
            Some(std) => report(
                ctx,
                attr.span,
                format!("Unknown property '{actual}' found, use '{std}' instead"),
            ),
            None => report(ctx, attr.span, format!("Unknown property '{actual}' found")),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn unknown_property() {
        check(
            "S6747",
            "a.jsx",
            r#"
import React from "react";
const a = <div class="x" />; // Noncompliant {{Unknown property 'class' found, use 'className' instead}}
const b = <div onclick={f} />; // Noncompliant {{Unknown property 'onclick' found, use 'onClick' instead}}
const c = <div className="x" onClick={f} data-foo="1" aria-label="x" />;
const d = <div foo="x" />; // Noncompliant {{Unknown property 'foo' found}}
const e = <Foo foo="x" class="y" />;
const f = <my-element foo="x" />;
const g = <div aria-lable="x" />; // Noncompliant {{aria-lable: This attribute is an invalid ARIA attribute. Did you mean to use aria-label?}}
const h = <Foo aria-foo="x" />; // Noncompliant
const i = <div checked />; // Noncompliant {{Invalid property 'checked' found on tag 'div', but it is only allowed on: input}}
const j = <input checked />;
const k = <svg viewBox="0 0 1 1"><path stroke-width="2" d="M0" /></svg>; // Noncompliant {{Unknown property 'stroke-width' found, use 'strokeWidth' instead}}
const l = <div css={x} />;
const m = <meta charSet="utf-8" />;
const n = <div tabindex="0" />; // Noncompliant {{Unknown property 'tabindex' found, use 'tabIndex' instead}}
"#,
        );
    }
}
