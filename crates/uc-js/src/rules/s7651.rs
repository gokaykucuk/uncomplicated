//! Output bindings should not be named as standard DOM events
use crate::helpers::batch_ts::angular_bindings;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Standard DOM event names (a well-known subset of the upstream list).
const NATIVE_EVENTS: &[&str] = &[
    "abort",
    "afterprint",
    "animationend",
    "animationiteration",
    "animationstart",
    "beforeprint",
    "beforeunload",
    "blur",
    "canplay",
    "canplaythrough",
    "change",
    "click",
    "close",
    "compositionend",
    "compositionstart",
    "compositionupdate",
    "contextmenu",
    "copy",
    "cut",
    "dblclick",
    "drag",
    "dragend",
    "dragenter",
    "dragleave",
    "dragover",
    "dragstart",
    "drop",
    "durationchange",
    "emptied",
    "ended",
    "error",
    "focus",
    "focusin",
    "focusout",
    "fullscreenchange",
    "fullscreenerror",
    "hashchange",
    "input",
    "invalid",
    "keydown",
    "keypress",
    "keyup",
    "load",
    "loadeddata",
    "loadedmetadata",
    "loadend",
    "loadstart",
    "message",
    "mousedown",
    "mouseenter",
    "mouseleave",
    "mousemove",
    "mouseout",
    "mouseover",
    "mouseup",
    "offline",
    "online",
    "open",
    "pagehide",
    "pageshow",
    "paste",
    "pause",
    "play",
    "playing",
    "pointercancel",
    "pointerdown",
    "pointerenter",
    "pointerleave",
    "pointermove",
    "pointerout",
    "pointerover",
    "pointerup",
    "popstate",
    "progress",
    "ratechange",
    "reset",
    "resize",
    "scroll",
    "seeked",
    "seeking",
    "select",
    "show",
    "stalled",
    "storage",
    "submit",
    "suspend",
    "timeupdate",
    "touchcancel",
    "touchend",
    "touchmove",
    "touchstart",
    "transitionend",
    "unload",
    "volumechange",
    "waiting",
    "wheel",
];

const MSG: &str = "Output bindings, including aliases, should not be named as standard DOM events";

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Class]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::Class(class) = node.kind() else {
            return;
        };
        for b in angular_bindings(ctx, class, "Output", "output") {
            if NATIVE_EVENTS.contains(&b.name) {
                ctx.report(b.key, MSG);
            }
            if let Some((span, alias)) = b.alias
                && NATIVE_EVENTS.contains(&alias)
            {
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
import { output } from '@angular/core';
@Component({ selector: 'app-a' })
class A {
  @Output() change = new EventEmitter(); // Noncompliant {{Output bindings, including aliases, should not be named as standard DOM events}}
  @Output('click') clicked = new EventEmitter(); // Noncompliant
  @Output() valueChange = new EventEmitter();
  select = output(); // Noncompliant
  focus = 1;
}
"#,
        );
    }
}
