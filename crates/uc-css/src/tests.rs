use crate::parse::{Config, run};
use crate::{Dialect, data};

use Dialect::{Css, Less, Scss};

fn hits(src: &str, d: Dialect) -> Vec<(&'static str, u32, String)> {
    let lines = uc_core::LineIndex::new(src);
    run(src, d, &Config::all(), &lines).into_iter().map(|r| (r.key, lines.line(r.start), r.msg)).collect()
}

/// Hits of `rule`, as (line, message).
fn rule(rule: &str, src: &str, d: Dialect) -> Vec<(u32, String)> {
    hits(src, d).into_iter().filter(|h| h.0 == rule).map(|h| (h.1, h.2)).collect()
}

fn count(r: &str, src: &str, d: Dialect) -> usize {
    rule(r, src, d).len()
}

#[track_caller]
fn clean(src: &str, d: Dialect) {
    let h = hits(src, d);
    assert!(h.is_empty(), "{d:?}: unexpected issues {h:#?}\n{src}");
}

#[test]
fn sets_sorted() {
    for set in [
        data::PROPERTIES,
        data::PSEUDO_CLASSES,
        data::PSEUDO_ELEMENTS,
        data::AT_RULES,
        data::MEDIA_FEATURES,
        data::UNITS,
        data::ELEMENTS,
        data::FONT_SIZE_KEYWORDS,
        data::FONT_KEYWORDS,
    ] {
        assert!(set.windows(2).all(|w| w[0] < w[1]), "unsorted set");
    }
}

#[test]
fn implemented_in_catalog() {
    let cat = uc_core::Catalog::get();
    for k in crate::IMPLEMENTED {
        assert!(cat.rule(k, uc_core::Repo::Css).is_some(), "{k} missing from catalog/css.json");
    }
}

const MODERN_CSS: &str = r##"
@charset "UTF-8";
@import url("theme.css") layer(theme) supports(display: grid) screen;
@layer reset, base, components;
@property --angle { syntax: "<angle>"; inherits: false; initial-value: 0deg; }
@font-face {
  font-family: "Inter";
  src: url(inter.woff2) format("woff2"), url("inter.woff") format("woff");
  font-display: swap;
  unicode-range: U+0000-00FF, U+0131, U+0152-0153;
  ascent-override: 90%;
  size-adjust: 105%;
}
@counter-style thumbs { system: cyclic; symbols: "👍"; suffix: " "; }
:root { --brand: #0af; --space: calc(1rem + 2px); color-scheme: light dark; }
html { -webkit-text-size-adjust: 100%; text-size-adjust: 100%; scrollbar-gutter: stable; }
body { font: 400 1rem/1.5 "Inter", system-ui, sans-serif; margin: 0; }
.card {
  container: card / inline-size;
  inset: 0;
  aspect-ratio: 16 / 9;
  background: linear-gradient(to right in oklch, #fff 0%, rgb(0 0 0 / 50%) 100%), url(data:image/svg+xml;base64,AAA=);
  width: clamp(10rem, 50cqi - 1rem, 40rem);
  height: calc(100dvh - var(--header, 64px));
  transition: opacity .3s cubic-bezier(.25, .8, .25, 1), transform 200ms;
  text-wrap: balance;
  field-sizing: content;
  view-transition-name: card;
  anchor-name: --card;
  position-anchor: --card;
  position-area: top;
  interpolate-size: allow-keywords;
  translate: 10px 0;
  grid-template-columns: [full-start] minmax(1em, 1fr) [content-start] repeat(auto-fill, minmax(200px, 1fr));
  color: color-mix(in srgb, var(--brand) 40%, white);
  font-feature-settings: "liga" 1, "ss01";
  filter: drop-shadow(0 0 2px #000a);
  mask: url(#mask);
  &:hover { color: red; }
  & > .title { font-weight: 700; }
  .nested & { color: blue; }
}
.card:has(> img):not(:focus-visible)::before { content: "\201C"; }
:is(h1, h2):where(.a, .b) { margin-block: 1em; }
input::placeholder, ::backdrop, ::view-transition-old(root), ::part(label), ::slotted(span) { opacity: .5; }
dialog:popover-open, :state(checked), a:any-link, :user-invalid { outline: 2px solid; }
::-webkit-scrollbar-thumb:window-inactive { background: #ccc; }
svg|circle, *|a { fill: currentColor; }
my-element, x-foo::part(thing) { display: contents; }
@container card (min-width: 400px) { .title { font-size: 2cqi; } }
@media (width >= 600px) and (prefers-reduced-motion: no-preference), print and (orientation: landscape) {
  .a { animation: spin 1s linear infinite; }
}
@media (400px <= width <= 700px) { .b { color: red; } }
@media screen and (-webkit-min-device-pixel-ratio: 2), (min-resolution: 192dpi) { .c { color: red; } }
@supports (display: grid) and (not (display: inline-grid)) { .d { display: grid; } }
@scope (.card) to (.content) { img { border: 1px solid; } }
@starting-style { .e { opacity: 0; } }
@keyframes spin { from { transform: rotate(0deg); } to { transform: rotate(1turn); } }
@-webkit-keyframes spin { 0% { -webkit-transform: rotate(0); } 100% { -webkit-transform: rotate(360deg); } }
@page :first { margin: 1in; size: A4; @top-center { content: "Title"; } }
.ie { *zoom: 1; _height: 1px; filter: progid:DXImageTransform.Microsoft.gradient(startColorstr='#80000000', endColorstr='#80000000'); }
.hack { width: 100px\9; }
.md\:flex, .w-1\/2, .hover\:bg-red:hover { display: flex; }
.f { display: -webkit-box; display: -ms-flexbox; display: flex; }
.g { background: url(a.png); background: url(a.svg), none; }
.h { margin: 0 auto; padding: 1px 2px 3px 4px; }
.i { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; }
.j { font: italic small-caps bold condensed 16px/2 cursive; }
.k { font-family: inherit; }
.l { font: menu; }
.m { width: calc(-1 * var(--x)); margin: calc(1px - -2px); top: calc((100% - 10px) / 2); left: calc(1e3px * 2); }
.n { transform: translate3d(0, 0, 0) rotate(-45deg); grid-area: 1 / 2 / 3 / 4; }
.o { background-image: -webkit-linear-gradient(top, #fff, #000); background-image: linear-gradient(180deg, red, blue); }
.p { background-image: linear-gradient(0, red, blue), repeating-linear-gradient(to top left, red 0 10px, blue 10px 20px); }
"##;

#[test]
fn modern_css_is_clean() {
    clean(MODERN_CSS, Css);
}

const MODERN_SCSS: &str = r##"
@use "sass:math";
@use "config" as cfg with ($primary: #333);
@forward "src/list" hide list-reset;
@import 'variables', 'mixins';
// line comment with an apostrophe: don't
$breakpoints: (sm: 576px, md: 768px, "lg": 992px); // map
$z: (modal: 100, toast: 200) !default;
%placeholder { color: red; }
@mixin respond($bp) {
  @if map-has-key($breakpoints, $bp) {
    @media (min-width: map-get($breakpoints, $bp)) { @content; }
  } @else if $bp == print {
    @media print { @content; }
  } @else {
    @warn "Unknown breakpoint #{$bp}";
  }
}
@function rem($px, $base: 16px) { @return math.div($px, $base) * 1rem; }
.btn {
  @extend %placeholder;
  @include respond(md) { padding: rem(12px); }
  font: { family: $font; size: 12px; weight: bold; }
  margin: 0 { left: auto; }
  width: calc(100% - #{$gutter * 2});
  height: calc($a + $b);
  #{$prop}-top: 1px;
  &-primary { color: cfg.$primary; }
  &__icon, &--large { fill: currentColor; }
  &:not(&--disabled):hover { opacity: .8; }
  #{&}__x { color: red; }
  .icon-#{$name} { content: "#{$glyph}"; }
  @at-root .theme-dark & { color: white; }
  + .btn { margin-left: 4px; }
  > span { display: inline-block; }
  ~ p { color: blue; }
  @each $name, $glyph in $icons {
    .icon-#{$name}:before { content: $glyph; }
  }
  @for $i from 1 through 3 { .m-#{$i} { margin: #{$i * 4}px; } }
  @while $i > 0 { .w-#{$i} { width: 10px * $i; } }
  @debug "x";
  color: if($condition, red, blue);
  background: rgba($black, .5) url('#{$path}/img.png');
  border-radius: math.div($r, 2);
}
@media #{$mq-tablet} and (max-width: $bp-lg - 1) { .c { display: none; } }
:global(.foo) .bar, :local .baz, :deep(.x), ::v-deep .y, ::ng-deep .z { color: red; }
:export { primaryColor: $primary; spacingUnit: 8px; }
@keyframes fade { #{$from} { opacity: 0; } 100% { opacity: 1; } }
@include keyframes(pulse) { from { opacity: 1; } to { opacity: 0; } }
.a { width: 10px; } // trailing comment
.b {
  color: red // no semicolon before the brace
}
"##;

#[test]
fn modern_scss_is_clean() {
    clean(MODERN_SCSS, Scss);
}

const MODERN_LESS: &str = r##"
@import (reference) "bootstrap.less";
@plugin "my-plugin";
@primary: #428bca;
@min768: ~"(min-width: 768px)";
@selector: ~".my";
@my-ruleset: {
  .my-selector { background-color: black; }
};
@detached: { background: red; };
// a line comment
.bordered(@width: 2px; @style: solid) { border: @width @style black; }
.mixin(@a) when (lightness(@a) >= 50%) { background-color: black; }
.mixin(@a) when (lightness(@a) < 50%) { background-color: white; }
.guard when (@mode = huge) { width: 100%; }
#namespace { .m() { color: red; } }
.box {
  .bordered(4px; dashed);
  .mixin(#ddd);
  #namespace > .m();
  #namespace.m();
  @detached();
  color: darken(@primary, 10%);
  width: ~"calc(100% - @{gutter})";
  height: (@base * 2);
  @{property}: 1px;
  background+: url(1.png);
  background+_: url(2.png);
  &:extend(.foo all);
  .@{name}-suffix { color: blue; }
  @media @min768 { float: left; }
  @media (min-width: @screen-md) { float: right; }
  @r: { color: red };
  filter: ~"ms:alwaysHasItsOwnSyntax.For.Stuff()";
  .mixin() !important;
}
.e(@rules) { @rules(); }
@media screen { @media (min-width: 768px) { .x { color: red; } } }
.lazy-eval { width: @var; @var: @a; @a: 9%; }
@my-color: e(%("#%s", "fff"));
"##;

#[test]
fn modern_less_is_clean() {
    clean(MODERN_LESS, Less);
}

#[test]
fn tailwind_and_postcss() {
    clean(
        r##"
@tailwind base;
@tailwind components;
@import "tailwindcss";
@config "./tailwind.config.js";
@plugin "daisyui" { themes: light --default, dark --prefersdark; }
@plugin "daisyui/theme" { name: "light"; default: true; }
@source "../node_modules/@my-company/ui-lib";
@theme { --color-primary: oklch(0.7 0.1 200); --font-display: "Satoshi", "sans-serif"; --*: initial; }
@utility tab-4 { tab-size: 4; }
@custom-variant dark (&:where(.dark, .dark *));
@custom-variant theme-midnight { &:where([data-theme="midnight"] *) { @slot; } }
@reference "../../app.css";
@layer components { .btn { @apply px-4 py-2 rounded hover:bg-blue-500 md:w-1/2; } }
@screen md { .x { color: red; } }
@variant dark { .y { color: white; } }
@custom-media --small-viewport (max-width: 30em);
@media (--small-viewport) { .z { color: red; } }
@custom-selector :--heading h1, h2, h3;
:--heading { margin: 0; }
@define-mixin icon $name { padding-left: 16px; }
@value primary: #BF4040;
.a { composes: b from "./b.css"; composes: c; }
"##,
        Css,
    );
}

// ---- rules ---------------------------------------------------------------------------------

#[test]
fn s1116_extra_semicolons() {
    assert_eq!(count("S1116", "a { color: red;; }", Css), 1);
    assert_eq!(count("S1116", "a { ; color: red; }", Css), 1);
    assert_eq!(count("S1116", "a { color: red; };", Css), 1);
    assert_eq!(count("S1116", "@import \"a.css\";;", Css), 1);
    assert_eq!(count("S1116", ".a { .b { color: red; }; }", Scss), 1);
    assert_eq!(count("S1116", ".a { .mixin();; }", Less), 1);
    assert_eq!(count("S1116", "a { color: red; }", Css), 0);
    assert_eq!(count("S1116", "@detached: { color: red; };", Less), 0);
    assert_eq!(count("S1116", "a { --m: { color: red; }; }", Css), 0);
    assert_eq!(count("S1116", "a { background: url(data:image/png;base64,AAA); }", Css), 0);
}

#[test]
fn s1128_duplicate_imports() {
    assert_eq!(count("S1128", "@import 'a.css';\n@import \"a.css\";", Css), 1);
    assert_eq!(count("S1128", "@import url(a.css);\n@import 'a.css';", Css), 1);
    assert_eq!(count("S1128", "@import 'a.css';\n@import 'a.css' print;", Css), 0);
    assert_eq!(count("S1128", "@import 'a';\n@import 'b';", Scss), 0);
    assert_eq!(count("S1128", "@import 'a';\n@import 'a';", Scss), 1);
    assert_eq!(count("S1128", "@import (reference) 'a';\n@import (reference) 'a';", Less), 1);
    assert_eq!(
        rule("S1128", "@import 'a.css';\n@import 'a.css';", Css)[0].1,
        "Unexpected duplicate @import rule a.css"
    );
}

#[test]
fn s4647_invalid_hex() {
    assert_eq!(
        rule("S4647", "a { color: #ff00zz; }", Css),
        vec![(1, "Unexpected invalid hex color \"#ff00zz\"".into())]
    );
    assert_eq!(count("S4647", "a { color: #12345; }", Css), 1);
    assert_eq!(count("S4647", "a { border: 1px solid #fffffffff; }", Css), 1);
    assert_eq!(count("S4647", "$c: #ggg;", Scss), 1);
    assert_eq!(count("S4647", "@c: #ggg;", Less), 1);
    for ok in ["#fff", "#ffff", "#ffffff", "#ffffff80", "#AbCdEf"] {
        assert_eq!(count("S4647", &format!("a {{ color: {ok}; }}"), Css), 0, "{ok}");
    }
    assert_eq!(count("S4647", "a { fill: url(#gradient-1); clip-path: url('#x'); content: \"#zz\"; }", Css), 0);
    assert_eq!(count("S4647", "a { color: #{$c}; width: calc(#{$w}); }", Scss), 0);
    assert_eq!(count("S4647", "a:not(#main) { color: red; }", Css), 0);
}

#[test]
fn s4648_duplicate_font_names() {
    assert_eq!(
        rule("S4648", "a { font-family: Arial, sans-serif, Arial; }", Css),
        vec![(1, "Unexpected duplicate name Arial".into())]
    );
    assert_eq!(count("S4648", "a { font-family: 'Times New Roman', Times   New Roman, serif; }", Css), 1);
    assert_eq!(count("S4648", "a { font-family: serif, SERIF; }", Css), 1);
    assert_eq!(count("S4648", "a { font: 12px/1.5 Arial, Arial, sans-serif; }", Css), 1);
    assert_eq!(count("S4648", "a { font-family: \"serif\", serif; }", Css), 0);
    assert_eq!(count("S4648", "a { font-family: Arial, arial, sans-serif; }", Css), 0);
    assert_eq!(count("S4648", "a { font-family: $a, $a; }", Scss), 0);
    assert_eq!(count("S4648", "a { font-family: @a, Arial, Arial; }", Less), 0);
}

#[test]
fn s4649_generic_family() {
    assert_eq!(
        rule("S4649", "a { font-family: Arial, Helvetica; }", Css),
        vec![(1, "Unexpected missing generic font family".into())]
    );
    assert_eq!(count("S4649", "a { font-family: 'Onest', 'sans-serif'; }", Css), 1);
    assert_eq!(count("S4649", "a { font: bold 14px/1.2 \"Helvetica Neue\", Arial; }", Css), 1);
    assert_eq!(count("S4649", "a { font-family: Arial, sans-serif; }", Css), 0);
    assert_eq!(count("S4649", "a { font-family: system-ui; }", Css), 0);
    assert_eq!(count("S4649", "a { font-family: unset; font: inherit; font: caption; }", Css), 0);
    assert_eq!(count("S4649", "@font-face { font-family: MyFont; src: url(a.woff); }", Css), 0);
    assert_eq!(count("S4649", "a { font-family: var(--font); }", Css), 0);
    assert_eq!(count("S4649", "a { font-family: Arial, $fallback; }", Scss), 0);
    assert_eq!(count("S4649", "a { font-family: \"A\", @fallback; }", Less), 0);
    assert_eq!(count("S4649", "a { font: 12px $font; }", Scss), 0);
}

#[test]
fn s4650_calc_spacing() {
    assert_eq!(
        rule("S4650", "a { top: calc(1px+2px); }", Css),
        vec![(1, "Expected single space before \"+\" operator".into())]
    );
    assert_eq!(
        rule("S4650", "a { top: calc(1px +2px); }", Css),
        vec![(1, "Expected single space after \"+\" operator".into())]
    );
    assert_eq!(count("S4650", "a { top: calc(1px -2px); }", Css), 1);
    assert_eq!(count("S4650", "a { top: calc(100%-10px); }", Css), 1);
    assert_eq!(count("S4650", "a { top: calc(var(--a)- 1px); }", Css), 1);
    assert_eq!(count("S4650", "a { top: calc(1px + (2px+3px)); }", Css), 1);
    assert_eq!(count("S4650", "a { top: -webkit-calc(1px+2px); }", Css), 1);
    assert_eq!(count("S4650", "a { top: calc(1px + 2px); left: calc(-1px * 2); }", Css), 0);
    assert_eq!(count("S4650", "a { top: calc(1px - -2px); width: calc(100%/3); }", Css), 0);
    assert_eq!(count("S4650", "a { top: calc(var(--a-b) - env(safe-area-inset-top)); }", Css), 0);
    assert_eq!(count("S4650", "a { top: calc(\n  100% -\n  10px\n); }", Css), 0);
    assert_eq!(count("S4650", "a { top: calc($a+$b); }", Scss), 0);
    assert_eq!(count("S4650", "a { top: calc(@a+@b); }", Less), 0);
    assert_eq!(count("S4650", "a { top: calc(10px-5px); }", Css), 0); // tokenized as one dimension
}

#[test]
fn s4651_gradient_direction() {
    assert_eq!(
        rule("S4651", "a { background: linear-gradient(top, #fff, #000); }", Css),
        vec![(1, "Unexpected nonstandard direction".into())]
    );
    assert_eq!(count("S4651", "a { background: linear-gradient(left top, #fff, #000); }", Css), 1);
    assert_eq!(count("S4651", "a { background: linear-gradient(to top top, #fff, #000); }", Css), 1);
    assert_eq!(count("S4651", "a { background: linear-gradient(to up, #fff, #000); }", Css), 1);
    assert_eq!(count("S4651", "a { background: linear-gradient(45, #fff, #000); }", Css), 1);
    assert_eq!(count("S4651", "a { background: repeating-linear-gradient(bottom, #fff, #000); }", Scss), 1);
    assert_eq!(count("S4651", "a { background: linear-gradient(to top right, red, blue); }", Css), 0);
    assert_eq!(count("S4651", "a { background: linear-gradient(-45deg, red, blue); }", Css), 0);
    assert_eq!(count("S4651", "a { background: linear-gradient(red, blue); }", Css), 0);
    assert_eq!(count("S4651", "a { background: linear-gradient(transparent 50%, red); }", Css), 0);
    assert_eq!(count("S4651", "a { background: linear-gradient(in oklab, red, blue); }", Css), 0);
    assert_eq!(count("S4651", "a { background: -webkit-linear-gradient(top, red, blue); }", Css), 0);
    assert_eq!(count("S4651", "a { background: linear-gradient($dir, red, blue); }", Scss), 0);
    assert_eq!(count("S4651", "a { background: linear-gradient(@dir, red, blue); }", Less), 0);
}

#[test]
fn s4652_string_newline() {
    assert_eq!(
        rule("S4652", "a { content: \"first\nsecond\"; }", Css),
        vec![(1, "Unexpected newline in string".into())]
    );
    assert_eq!(count("S4652", "a[title='a\nb'] { color: red; }", Scss), 1);
    assert_eq!(count("S4652", "@x: 'a\nb';", Less), 1);
    assert_eq!(count("S4652", "a { content: \"first\\\nsecond\"; }", Css), 0);
    assert_eq!(count("S4652", "a { content: \"x\"; } // it's fine\nb { color: red; }", Scss), 0);
    assert_eq!(count("S4652", "/* it's fine\n */ b { color: red; }", Css), 0);
}

#[test]
fn s4653_unknown_unit() {
    assert_eq!(rule("S4653", "a { width: 10pixels; }", Css), vec![(1, "Unexpected unknown unit \"pixels\"".into())]);
    assert_eq!(count("S4653", "a { width: calc(10pxx + 1px); }", Css), 1);
    assert_eq!(count("S4653", "@media (min-width: 10ppx) { a { color: red; } }", Css), 1);
    assert_eq!(count("S4653", "$w: 10pxs;", Scss), 1);
    assert_eq!(count("S4653", "a { width: 10pxs; }", Less), 1);
    assert_eq!(
        count(
            "S4653",
            "a { width: 1px 1em 1rem 1vw 1dvh 1cqi 1fr 1x 2dppx 1turn 1s 1ms 1Q 50% 1e3 1.5e-2 1lh 1rlh 1vb; }",
            Css
        ),
        0
    );
    assert_eq!(
        count("S4653", "a { color: #1e90ff; content: \"10zz\"; grid-area: a1; transform: translate3d(0,0,0); }", Css),
        0
    );
    assert_eq!(count("S4653", "@font-face { unicode-range: U+0025-00FF; }", Css), 0);
    assert_eq!(count("S4653", "a { width: #{$i}px; margin: $i * 1px; }", Scss), 0);
    assert_eq!(count("S4653", "a { width: ~\"10zz\"; height: @h; }", Less), 0);
}

#[test]
fn s4654_unknown_property() {
    assert_eq!(rule("S4654", "a { colr: red; }", Css), vec![(1, "Unexpected unknown property \"colr\"".into())]);
    assert_eq!(count("S4654", "a { line-heigh: 28px; }", Less), 1);
    assert_eq!(count("S4654", ".a { .b { my-property: 1; } }", Scss), 1);
    assert_eq!(count("S4654", "a { font-smoothing: antialiased; }", Css), 1);
    assert_eq!(
        count(
            "S4654",
            "a { -webkit-font-smoothing: antialiased; --x: 1; $y: 2; COLOR: red; mso-line-height-rule: exactly; }",
            Scss
        ),
        0
    );
    assert_eq!(count("S4654", "a { #{$p}: 1; @{p}: 1; }", Scss), 0);
    assert_eq!(count("S4654", "a { font: { family: x; size: 1px; } }", Scss), 0);
    assert_eq!(count("S4654", ":export { primaryColor: red; }", Css), 0);
    assert_eq!(count("S4654", "@font-feature-values Font One { @styleset { nice-style: 12; } }", Css), 0);
    assert_eq!(count("S4654", "@counter-style x { system: cyclic; symbols: a; pad: 3 \"0\"; }", Css), 0);
    assert_eq!(count("S4654", "a { container-type: size; view-transition-name: x; text-wrap: pretty; }", Css), 0);
}

#[test]
fn s4655_keyframe_important() {
    assert_eq!(
        rule("S4655", "@keyframes a { from { opacity: 0 !important; } }", Css),
        vec![(1, "Unexpected !important".into())]
    );
    assert_eq!(count("S4655", "@-webkit-keyframes a { 50% { top: 0 ! IMPORTANT } }", Scss), 1);
    assert_eq!(count("S4655", "@keyframes a { to { opacity: 1 !important; } }", Less), 1);
    assert_eq!(count("S4655", "a { opacity: 0 !important; } @keyframes a { from { opacity: 0; } }", Css), 0);
}

#[test]
fn s4656_duplicate_properties() {
    assert_eq!(
        rule("S4656", "a { color: red; top: 0; color: blue; }", Css),
        vec![(1, "Unexpected duplicate \"color\"".into())]
    );
    assert_eq!(count("S4656", "a { color: red; color: red; }", Css), 1);
    assert_eq!(count("S4656", "a { color: red; COLOR: blue; top: 0; }", Css), 0);
    assert_eq!(count("S4656", "a { color: red; top: 0; COLOR: red; }", Css), 1);
    assert_eq!(count("S4656", "a { display: -webkit-box; display: flex; }", Css), 0);
    assert_eq!(count("S4656", "a { color: red; .b { color: blue; } }", Scss), 0);
    assert_eq!(count("S4656", "a { color: red; @include x; color: red; }", Scss), 1);
    assert_eq!(count("S4656", "a { $v: 1; $v: 2; --x: 1; --x: 2; }", Scss), 0);
    assert_eq!(count("S4656", "a { color: red; &:hover { top: 0; } top: 1px; color: red; }", Less), 1);
    assert_eq!(count("S4656", "a { composes: a; composes: b from './b.css'; composes: a; }", Css), 0);
}

#[test]
fn s4657_shorthand_overrides() {
    assert_eq!(
        rule("S4657", "a { padding-left: 10px; padding: 20px; }", Css),
        vec![(1, "Unexpected shorthand \"padding\" after \"padding-left\"".into())]
    );
    assert_eq!(count("S4657", "a { background-color: red; background: blue; }", Scss), 1);
    assert_eq!(count("S4657", "a { -webkit-transition-property: top; -webkit-transition: none; }", Css), 1);
    assert_eq!(count("S4657", "a { border-top-width: 1px; border-width: 2px; }", Less), 1);
    assert_eq!(count("S4657", "a { padding: 20px; padding-left: 10px; }", Css), 0);
    assert_eq!(count("S4657", "a { -webkit-transition-property: top; transition: none; }", Css), 0);
    assert_eq!(count("S4657", "a { padding-left: 1px; .b { padding: 0; } }", Scss), 0);
}

#[test]
fn s4658_empty_blocks() {
    assert_eq!(rule("S4658", "a {}", Css), vec![(1, "Unexpected empty block".into())]);
    assert_eq!(count("S4658", "@media print {\n}", Css), 1);
    assert_eq!(count("S4658", ".a { &:hover { } }", Scss), 1);
    assert_eq!(count("S4658", ".m() {}", Less), 1);
    assert_eq!(count("S4658", "a { /* todo */ }", Css), 0);
    assert_eq!(count("S4658", "a { // todo\n}", Scss), 0);
    assert_eq!(count("S4658", "a { color: red; }", Less), 0);
}

#[test]
fn s4659_unknown_pseudo_class() {
    assert_eq!(
        rule("S4659", "a:hovr { color: red; }", Css),
        vec![(1, "Unexpected unknown pseudo-class selector \":hovr\"".into())]
    );
    assert_eq!(count("S4659", "a:not(:focuss) { color: red; }", Css), 1);
    assert_eq!(count("S4659", ".a { &:selected { color: red; } }", Scss), 1);
    assert_eq!(count("S4659", ".a:unknown-thing { color: red; }", Less), 1);
    assert_eq!(
        count("S4659", "a:HOVER, a:before, a:-moz-focusring, :--custom, :global(.x), :host(.y) { color: red; }", Css),
        0
    );
    assert_eq!(count("S4659", "a[href^='http:'], .md\\:flex, a:#{$state} { color: red; }", Scss), 0);
    assert_eq!(count("S4659", "a { margin:0; }", Css), 0);
}

#[test]
fn s4660_unknown_pseudo_element() {
    assert_eq!(
        rule("S4660", "a::pseudo { color: red; }", Css),
        vec![(1, "Unexpected unknown pseudo-element selector \"::pseudo\"".into())]
    );
    assert_eq!(count("S4660", "a::hover { color: red; }", Css), 1);
    assert_eq!(count("S4660", ".a { &::input-placeholder { color: red; } }", Scss), 1);
    assert_eq!(count("S4660", ".a::befor { color: red; }", Less), 1);
    assert_eq!(
        count("S4660", "a::-webkit-input-placeholder, ::-moz-selection, a::BEFORE, ::ng-deep .x { color: red; }", Css),
        0
    );
}

#[test]
fn s4661_unknown_media_feature() {
    assert_eq!(
        rule("S4661", "@media screen and (max-widht: 100px) { a { color: red; } }", Css),
        vec![(1, "Unexpected unknown media feature name \"max-widht\"".into())]
    );
    assert_eq!(count("S4661", "@media (100px <= widht) { a { color: red; } }", Css), 1);
    assert_eq!(count("S4661", "@media (min-widht: $md) { a { color: red; } }", Scss), 1);
    assert_eq!(count("S4661", "@media (min-widht: @md) { a { color: red; } }", Less), 1);
    assert_eq!(
        count("S4661", "@media (min--moz-device-pixel-ratio: 1.5), (--custom), (color) { a { color: red; } }", Css),
        0
    );
    assert_eq!(count("S4661", "@media #{$query} and (hover: hover) { a { color: red; } }", Scss), 0);
    assert_eq!(count("S4661", "@media (min-width: map-get($bp, md)) { a { color: red; } }", Scss), 0);
}

#[test]
fn s4662_unknown_at_rule() {
    assert_eq!(rule("S4662", "@unknown foo;", Css), vec![(1, "Unexpected unknown at-rule \"@unknown\"".into())]);
    assert_eq!(count("S4662", "@mixn foo { }", Scss), 1);
    assert_eq!(count("S4662", "@foo { }", Less), 1);
    assert_eq!(
        count("S4662", "@-moz-document url-prefix() { a { color: red; } } @MEDIA print { a { top: 0; } }", Css),
        0
    );
    assert_eq!(count("S4662", "@var: 1px; @detached(); @{sel} { color: red; }", Less), 0);
}

#[test]
fn s4663_empty_comment() {
    assert_eq!(rule("S4663", "/* */\na { color: red; }", Css), vec![(1, "Unexpected empty comment".into())]);
    assert_eq!(count("S4663", "a { color: red; /**/ }", Css), 1);
    assert_eq!(count("S4663", "a { color: /*   */ red; }", Scss), 1);
    assert_eq!(count("S4663", "/*\n\n*/", Less), 1);
    assert_eq!(count("S4663", "/* x */ a { color: red; }\n//\n", Scss), 0);
}

#[test]
fn s4666_duplicate_selectors() {
    assert_eq!(
        rule("S4666", ".foo {}\n.bar {}\n.foo { color: red; }", Css).into_iter().map(|x| x.1).collect::<Vec<_>>(),
        vec!["Unexpected duplicate selector \".foo\", first used at line 1".to_string()]
    );
    assert_eq!(count("S4666", "a, b { top: 0; }\nb, a { top: 1px; }", Css), 1);
    assert_eq!(count("S4666", "a > b { top: 0; }\na>b { top: 1px; }", Css), 1);
    assert_eq!(count("S4666", ".a { &:hover { top: 0; } &:hover { top: 1px; } }", Scss), 1);
    assert_eq!(count("S4666", ".a { top: 0; } .a { top: 1px; }", Less), 1);
    assert_eq!(count("S4666", "a { top: 0; } @media print { a { top: 1px; } }", Css), 0);
    assert_eq!(count("S4666", "a, b { top: 0; } a { top: 1px; }", Css), 0);
    assert_eq!(count("S4666", ".a { .b { top: 0; } } .c { .b { top: 1px; } }", Scss), 0);
    assert_eq!(count("S4666", "@keyframes k { 0% { top: 0; } 0% { top: 1px; } }", Css), 0);
    assert_eq!(count("S4666", ".m(@a) { top: @a; } .m(@a) { left: @a; }", Less), 0);
    assert_eq!(count("S4666", ".x-#{$a} { top: 0; } .x-#{$a} { top: 1px; }", Scss), 0);
}

#[test]
fn s4667_empty_source() {
    assert_eq!(rule("S4667", "", Css), vec![(1, "Unexpected empty source".into())]);
    assert_eq!(count("S4667", "  \n\t\n", Scss), 1);
    assert_eq!(count("S4667", "\u{feff}", Less), 1);
    assert_eq!(count("S4667", "/* only a comment */", Css), 0);
}

#[test]
fn s4668_double_slash_comments() {
    assert_eq!(
        rule("S4668", "a {\n  // color: red;\n  top: 0;\n}", Css),
        vec![(2, "Unexpected double-slash CSS comment".into())]
    );
    assert_eq!(count("S4668", "// .old { color: red; }\na { top: 0; }", Css), 1);
    assert_eq!(count("S4668", "a { background: url(//cdn.example.com/a.png); content: \"//\"; }", Css), 0);
    assert_eq!(count("S4668", "// fine in SCSS\na { top: 0; }", Scss), 0);
    assert_eq!(count("S4668", "// fine in Less\na { top: 0; }", Less), 0);
}

#[test]
fn s4670_unknown_type_selector() {
    assert_eq!(rule("S4670", "tag { color: red; }", Css), vec![(1, "Unexpected unknown type selector \"tag\"".into())]);
    assert_eq!(count("S4670", "ul > lis, :not(spam) { color: red; }", Css), 2);
    assert_eq!(count("S4670", ".a { foo & { color: red; } }", Scss), 1);
    assert_eq!(count("S4670", ".a { bar { color: red; } }", Less), 1);
    assert_eq!(
        count(
            "S4670",
            "DIV, svg foreignObject, linearGradient, mi, custom-element, a:nth-child(2n + 1 of li), INPUT[type] { color: red; }",
            Css
        ),
        0
    );
    assert_eq!(count("S4670", "@keyframes x { from { top: 0; } to { top: 1px; } }", Css), 0);
    assert_eq!(count("S4670", ".a { &-b, &__c { top: 0; } #{$s} span, .x-#{$y} { top: 0; } }", Scss), 0);
    assert_eq!(count("S4670", ".m() when (default()) { top: 0; } .guard when (@a) { top: 0; }", Less), 0);
}

#[test]
fn s5362_invalid_calc() {
    assert_eq!(rule("S5362", "a { width: calc(100% - ); }", Css), vec![(1, "Expected a valid expression".into())]);
    assert_eq!(rule("S5362", "a { width: calc(1px / 0); }", Css), vec![(1, "Unexpected division by zero".into())]);
    assert_eq!(count("S5362", "a { width: calc(); }", Css), 1);
    assert_eq!(count("S5362", "a { width: calc(* 2px); }", Css), 1);
    assert_eq!(count("S5362", "a { width: calc(1px 2px); }", Scss), 1);
    assert_eq!(count("S5362", "a { width: calc(1px + () ); }", Less), 1);
    assert_eq!(count("S5362", "a { width: calc(100% / 3 - 2 * 1em - 2 * 1px); height: calc(1px / 0.5); }", Css), 0);
    assert_eq!(count("S5362", "a { width: calc(min(10px, 5vw) + max(1px, 2px)); top: calc(var(--x) * -1); }", Css), 0);
    assert_eq!(count("S5362", "a { width: calc($a / 0); }", Scss), 0);
}

#[test]
fn issue_positions() {
    let src = "a {\n  colr: red;\n}\n";
    let lines = uc_core::LineIndex::new(src);
    let r = run(src, Css, &Config::all(), &lines);
    let loc = lines.location(uc_core::Span::new(r[0].start, r[0].end));
    assert_eq!((loc.line, loc.col, loc.end_line, loc.end_col), (2, 2, 2, 6));
}

#[test]
fn tolerant_of_broken_input() {
    for src in [
        "a { color: red",
        "a { content: \"unterminated; }\nb { top: 0; }",
        "a { b { c { ",
        "}}}} a { top: 0; }",
        "@media (",
        "a:not( { top: 0; }",
        "/* unterminated",
        "a { background: url(",
        "@",
        "#{",
        "\"",
        "a { width: calc(1px + ; }",
    ] {
        for d in [Css, Scss, Less] {
            let _ = hits(src, d);
        }
    }
    let deep = "a {".repeat(1000);
    let _ = hits(&deep, Scss);
}

#[test]
fn metrics() {
    let (ncloc, comments, code) = super::metrics(b"/* a\n * b\n */\na {\n  color: red; // x\n}\n\n// only\n", Scss);
    assert_eq!((ncloc, comments), (3, 4));
    assert_eq!(code, vec![4, 5, 6]);
    let (ncloc, comments, _) = super::metrics(b"a { content: '/* no */'; }\n// code in css\n", Css);
    assert_eq!((ncloc, comments), (2, 0));
}

#[test]
fn analyze_api() {
    let rules = uc_core::RuleSet::all(uc_core::Repo::Css, crate::IMPLEMENTED);
    let r = crate::analyze("a.scss", "a {\n  colr: red;\n}\n", false, &rules);
    assert_eq!(r.metrics.lines, 4);
    assert_eq!(r.metrics.ncloc, 3);
    assert_eq!(r.issues.len(), 1);
    assert_eq!(r.issues[0].rule, "S4654");
    assert_eq!(r.issues[0].repo, uc_core::Repo::Css);
    let none = uc_core::RuleSet::default();
    assert!(crate::analyze("a.css", "a { colr: red }", false, &none).issues.is_empty());
}

#[test]
fn bootstrap_style_scss_is_clean() {
    clean(
        r##"
/// Doc comment
@use "sass:map";
@forward "functions" show rem;
$theme-colors: ("primary": $blue, "secondary": $gray-600) !default;
$grid-breakpoints: (xs: 0, sm: 576px, md: 768px) !default;
$enable-shadows: false !default;
@function breakpoint-min($name, $breakpoints: $grid-breakpoints) {
  $min: map.get($breakpoints, $name);
  @return if($min != 0, $min, null);
}
@mixin media-breakpoint-up($name, $breakpoints: $grid-breakpoints) {
  $min: breakpoint-min($name, $breakpoints);
  @if $min { @media (min-width: $min) { @content; } } @else { @content; }
}
@mixin button-variant($background, $border, $hover-background: darken($background, 7.5%)) {
  color: color-yiq($background);
  @include gradient-bg($background);
  &:focus, &.focus { box-shadow: 0 0 0 $btn-focus-width rgba(mix(color-yiq($background), $border, 15%), .5); }
  &:not(:disabled):not(.disabled):active, &:not(:disabled):not(.disabled).active, .show > &.dropdown-toggle {
    @if $enable-shadows and $btn-active-box-shadow != none { box-shadow: $btn-active-box-shadow; }
  }
}
@each $color, $value in $theme-colors {
  .btn-#{$color} { @include button-variant($value, $value); }
}
@each $breakpoint in map-keys($grid-breakpoints) {
  @include media-breakpoint-up($breakpoint) {
    $infix: breakpoint-infix($breakpoint, $grid-breakpoints);
    @each $prop, $abbrev in (margin: m, padding: p) {
      @each $size, $length in $spacers {
        .#{$abbrev}#{$infix}-#{$size} { #{$prop}: $length !important; }
        .#{$abbrev}t#{$infix}-#{$size}, .#{$abbrev}y#{$infix}-#{$size} { #{$prop}-top: $length !important; }
      }
    }
  }
}
.custom-select {
  background: $custom-select-bg $custom-select-background;
  width: calc(100% - (#{$custom-control-gutter} + #{$custom-control-indicator-size}));
  padding: $input-padding-y ($input-padding-x + $custom-select-indicator-padding) $input-padding-y $input-padding-x;
  @include border-radius($custom-select-border-radius, 0);
  &::-ms-expand { display: none; }
  &:-moz-focusring { text-shadow: 0 0 0 $custom-select-color; }
}
@supports (position: sticky) { .sticky-top { position: sticky; top: 0; z-index: $zindex-sticky; } }
@include foo using ($args...) { width: $args; }
.a { @include b { color: red; } }
.row { margin-right: -$gutter / 2; margin-left: - math.div($gutter, 2); }
.x { transform: translate(-50%, -50%) scale(.5); }
"##,
        Scss,
    );
}

#[test]
fn bootstrap_style_less_is_clean() {
    clean(
        r##"
@import (less) "foo.css";
@grid-columns: 12;
@screen-sm-min: 768px;
.button-variant(@color; @background; @border) {
  color: @color;
  &:focus, &.focus { color: @color; background-color: darken(@background, 10%); }
  .badge { color: @background; background-color: @color; }
}
.img-retina(@file-1x; @file-2x; @width-1x; @height-1x) {
  background-image: url("@{file-1x}");
  @media only screen and (-webkit-min-device-pixel-ratio: 2), only screen and (min-resolution: 192dpi) {
    background-image: url("@{file-2x}");
    background-size: @width-1x @height-1x;
  }
}
.make-grid-columns() {
  .col(@index) {
    @item: ~".col-xs-@{index}, .col-sm-@{index}";
    .col((@index + 1), @item);
  }
  .col(@index, @list) when (@index =< @grid-columns) {
    @item: ~".col-xs-@{index}";
    .col((@index + 1), ~"@{list}, @{item}");
  }
  .col(@index, @list) when (@index > @grid-columns) {
    @{list} { position: relative; min-height: 1px; }
  }
  .col(1);
}
.loop(@i) when (@i > 0) {
  .w-@{i} { width: (@i * 100% / @grid-columns); }
  .loop(@i - 1);
}
.btn-default { .button-variant(@btn-default-color; @btn-default-bg; @btn-default-border); }
.clearfix() { &:before, &:after { content: " "; display: table; } }
.row { &:extend(.clearfix all); margin-left: ceil((@gutter / -2)); }
.text-hide() { font: ~"0/0" a; color: transparent; }
@media (min-width: @screen-sm-min) { .navbar { border-radius: @navbar-border-radius; } }
.mixin(@color) when (iscolor(@color)) { color: @color; }
.mixin(@a; @rest...) { box-shadow: @arguments; }
button when (@my-option = true) { color: white; }
@config: { option1: true; option2: false; }
.k { color: if((@mode = dark), black, white); width: percentage((1 / 3)); }
@min768: ~"(min-width: 768px)";
.element { @media @min768 { font-size: 1.2rem; } }
.guard2 when (default()) { x: 1 }
"##,
        Less,
    );
}

#[test]
fn more_css_is_clean() {
    clean(
        r##"
@import url(foo.css) screen and (orientation:landscape);
@namespace svg url(http://www.w3.org/2000/svg);
@font-feature-values Font One { @styleset { nice-style: 12; } }
@media not all and (monochrome) { a { color: red; } }
@supports selector(:has(a)) { a { color: red; } }
@layer x { @layer y { a { color: red; } } }
@container style(--responsive: true) { a { color: red; } }
@container (inline-size > 30em) { a { color: red; } }
a{color:red}b{color:blue}@media screen{i{color:red}}
li { content: counter(item) ". "; quotes: "«" "»"; }
[class*='col-'], a[href$=".pdf" i], li:nth-child(odd), p:nth-of-type(3n+1), :lang(en), :dir(rtl) { color: red; }
::highlight(x), ::cue(b), :host-context(.dark), ::file-selector-button, ::marker, ::target-text { color: red; }
@media (prefers-color-scheme: dark) and (hover: hover) and (pointer: fine) and (forced-colors: active) { a { color: red; } }
@view-transition { navigation: auto; }
::view-transition-group(*) { animation-duration: .5s; }
@position-try --custom-left { position-area: left; width: 100px; }
.tooltip { position-try-fallbacks: --custom-left, flip-block; inset-area: top; }
.x { background-image: image-set("a.png" 1x, "a-2x.png" 2x); }
.y { transition: all 0.2s cubic-bezier(0.68, -0.55, 0.265, 1.55); }
.z { margin: -0.5em auto 0 -.25em; z-index: -1; }
.w { font: 12px/30px Georgia, serif; font: bold 1em/1.2 'Helvetica Neue', Helvetica, Arial, sans-serif; }
table td:first-child > a:not([href]):not([tabindex]) { color: inherit; }
.sr-only:not(:focus):not(:active) { clip: rect(0 0 0 0); clip-path: inset(50%); }
.v { width: -webkit-fill-available; width: -moz-available; width: stretch; }
.u { background: url(  "a.png"  ) no-repeat, url(b.png); }
.q { grid-template-areas: "header header" "sidebar main"; }
.r { will-change: transform, opacity; contain: layout paint; }
.s:is(:hover, :focus-within) > :where(svg, img) { scale: 1.05; }
"##,
        Css,
    );
}
