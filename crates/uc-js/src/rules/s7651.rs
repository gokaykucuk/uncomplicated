//! Output bindings should not be named as standard DOM events
use crate::helpers::batch_ts::{angular_bindings, metadata_entries};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// angular-eslint's `getNativeEventNames()`.
const NATIVE_EVENTS: &[&str] = &[
    "abort",
    "afterprint",
    "afterscriptexecute",
    "animationcancel",
    "animationend",
    "animationiteration",
    "animationstart",
    "appinstalled",
    "auxclick",
    "beforeinput",
    "beforeinstallprompt",
    "beforematch",
    "beforeprint",
    "beforescriptexecute",
    "beforetoggle",
    "beforeunload",
    "beforexrselect",
    "beginEvent",
    "blur",
    "cancel",
    "canplay",
    "canplaythrough",
    "change",
    "click",
    "close",
    "command",
    "compositionend",
    "compositionstart",
    "compositionupdate",
    "contentvisibilityautostatechange",
    "contextlost",
    "contextmenu",
    "contextrestored",
    "copy",
    "cuechange",
    "cut",
    "dblclick",
    "devicemotion",
    "deviceorientation",
    "deviceorientationabsolute",
    "DOMActivate",
    "DOMContentLoaded",
    "DOMMouseScroll",
    "drag",
    "dragend",
    "dragenter",
    "dragexit",
    "dragleave",
    "dragover",
    "dragstart",
    "drop",
    "durationchange",
    "emptied",
    "encrypted",
    "ended",
    "endEvent",
    "enterpictureinpicture",
    "error",
    "focus",
    "focusin",
    "focusout",
    "formdata",
    "freeze",
    "fullscreenchange",
    "fullscreenerror",
    "gamepadconnected",
    "gamepaddisconnected",
    "gesturechange",
    "gestureend",
    "gesturestart",
    "gotpointercapture",
    "hashchange",
    "input",
    "interest",
    "invalid",
    "keydown",
    "keypress",
    "keyup",
    "languagechange",
    "leavepictureinpicture",
    "load",
    "loadeddata",
    "loadedmetadata",
    "loadstart",
    "location",
    "loseinterest",
    "lostpointercapture",
    "message",
    "messageerror",
    "mousedown",
    "mouseenter",
    "mouseleave",
    "mousemove",
    "mouseout",
    "mouseover",
    "mouseup",
    "mousewheel",
    "MozMousePixelScroll",
    "offline",
    "online",
    "orientationchange",
    "pagehide",
    "pagereveal",
    "pageshow",
    "pageswap",
    "paste",
    "pause",
    "play",
    "playing",
    "pointercancel",
    "pointerdown",
    "pointerenter",
    "pointerleave",
    "pointerlockchange",
    "pointerlockerror",
    "pointermove",
    "pointerout",
    "pointerover",
    "pointerrawupdate",
    "pointerup",
    "popstate",
    "prerenderingchange",
    "progress",
    "promptaction",
    "promptdismiss",
    "ratechange",
    "readystatechange",
    "rejectionhandled",
    "repeatEvent",
    "reset",
    "resize",
    "resume",
    "scroll",
    "scrollend",
    "scrollsnapchange",
    "scrollsnapchanging",
    "search",
    "securitypolicyviolation",
    "seeked",
    "seeking",
    "select",
    "selectionchange",
    "selectstart",
    "slotchange",
    "stalled",
    "storage",
    "submit",
    "suspend",
    "timeupdate",
    "toggle",
    "touchcancel",
    "touchend",
    "touchmove",
    "touchstart",
    "transitioncancel",
    "transitionend",
    "transitionrun",
    "transitionstart",
    "unhandledrejection",
    "unload",
    "validationstatuschange",
    "visibilitychange",
    "volumechange",
    "vrdisplayactivate",
    "vrdisplayconnect",
    "vrdisplaydeactivate",
    "vrdisplaydisconnect",
    "vrdisplaypresentchange",
    "waiting",
    "waitingforkey",
    "webglcontextcreationerror",
    "webglcontextlost",
    "webglcontextrestored",
    "webkitmouseforcechanged",
    "webkitmouseforcedown",
    "webkitmouseforceup",
    "webkitmouseforcewillbegin",
    "wheel",
];

const MSG: &str = "Output bindings, including aliases, should not be named as standard DOM events";

/// angular-eslint checks `name` and `name: alias` texts (whitespace removed).
fn is_native(text: &str) -> bool {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let mut parts = compact.split(':');
    let name = parts.next().unwrap_or("");
    let alias = parts.next();
    NATIVE_EVENTS.contains(&name) || alias.is_some_and(|a| NATIVE_EVENTS.contains(&a))
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Class]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::Class(class) = node.kind() else {
            return;
        };
        for b in angular_bindings(ctx, class, "Output", "output") {
            if is_native(b.name) {
                ctx.report(b.key, MSG);
            }
            if let Some((span, alias)) = b.alias
                && is_native(alias)
            {
                ctx.report(span, MSG);
            }
        }
        for (span, text) in metadata_entries(class, "outputs") {
            if is_native(text) {
                ctx.report(span, MSG);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn output_native() {
        check(
            "S7651",
            "a.ts",
            r#"
@Component({ selector: 'app-a', outputs: ['other: blur', 'fine'] }) // Noncompliant
class A {
  @Output() change = new EventEmitter(); // Noncompliant {{Output bindings, including aliases, should not be named as standard DOM events}}
  @Output('click') clicked = new EventEmitter(); // Noncompliant
  @Output() valueChange = new EventEmitter();
  @Output() show = new EventEmitter();
  select = output(); // Noncompliant
  focus = 1;
}
"#,
        );
    }
}
