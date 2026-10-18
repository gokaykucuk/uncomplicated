//! String literals should not be duplicated.
//!
//! Like sonar-go, string literals passed to logging and error functions, and route paths passed to
//! the route-defining functions of common HTTP routers, are not counted. Without type information,
//! a method call on a variable matches when the file imports the package of the matcher.
use crate::helpers_sonar::import_bindings;
use crate::prelude::*;

pub struct Rule {
    threshold: usize,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            threshold: p.u32("threshold", 3) as usize,
        }
    }
}

const MIN_LENGTH: usize = 5;

/// A function matcher: package paths, function names, and whether it also matches method calls
/// on variables (of a type of the package).
struct Matcher {
    pkgs: &'static [&'static str],
    names: &'static [&'static str],
    on_variable: bool,
}

const fn m(
    pkgs: &'static [&'static str],
    names: &'static [&'static str],
    on_variable: bool,
) -> Matcher {
    Matcher {
        pkgs,
        names,
        on_variable,
    }
}

const LOGRUS: &[&str] = &[
    "Trace",
    "Tracef",
    "Traceln",
    "Debug",
    "Debugf",
    "Debugln",
    "Info",
    "Infof",
    "Infoln",
    "Warn",
    "Warnf",
    "Warnln",
    "Warning",
    "Warningf",
    "Warningln",
    "Error",
    "Errorf",
    "Errorln",
    "Fatal",
    "Fatalf",
    "Fatalln",
    "Panic",
    "Panicf",
    "Panicln",
    "Print",
    "Printf",
    "Println",
    "WithField",
    "WithFields",
    "WithError",
];
const ZAP_SUGARED: &[&str] = &[
    "Debug", "Debugf", "Debugw", "Debugln", "Info", "Infof", "Infow", "Infoln", "Warn", "Warnf",
    "Warnw", "Warnln", "Error", "Errorf", "Errorw", "Errorln", "DPanic", "DPanicf", "DPanicw",
    "DPanicln", "Panic", "Panicf", "Panicw", "Panicln", "Fatal", "Fatalf", "Fatalw", "Fatalln",
];
const GLOG: &[&str] = &[
    "Info",
    "Infof",
    "Infoln",
    "Warning",
    "Warningf",
    "Warningln",
    "Error",
    "Errorf",
    "Errorln",
    "Fatal",
    "Fatalf",
    "Fatalln",
];

/// Functions whose string literal arguments are all ignored.
const LOG_AND_ERRORS: &[Matcher] = &[
    m(
        &["log"],
        &[
            "Print", "Printf", "Println", "Fatal", "Fatalf", "Fatalln", "Panic", "Panicf",
            "Panicln",
        ],
        false,
    ),
    m(&["fmt"], &["Errorf", "Printf", "Fprintf", "Sprintf"], false),
    m(&["errors"], &["New"], false),
    m(
        &["log/slog"],
        &["Debug", "Info", "Warn", "Error", "Log", "LogAttrs"],
        false,
    ),
    m(&["github.com/sirupsen/logrus"], LOGRUS, false),
    m(
        &["go.uber.org/zap"],
        &["Debug", "Info", "Warn", "Error", "DPanic", "Panic", "Fatal"],
        true,
    ),
    m(&["go.uber.org/zap"], ZAP_SUGARED, true),
    m(&["github.com/rs/zerolog"], &["Msg", "Msgf"], true),
    m(
        &["github.com/golang/glog", "k8s.io/klog", "k8s.io/klog/v2"],
        GLOG,
        false,
    ),
    m(
        &["gopkg.in/inconshreveable/log15.v2"],
        &["Debug", "Info", "Warn", "Error", "Crit"],
        false,
    ),
    m(&["github.com/go-kit/log"], &["Log"], true),
    m(
        &["github.com/go-kit/log/level"],
        &["Debug", "Info", "Warn", "Error"],
        false,
    ),
    m(
        &["github.com/apex/log"],
        &[
            "Debug", "Debugf", "Info", "Infof", "Warn", "Warnf", "Error", "Errorf", "Fatal",
            "Fatalf",
        ],
        false,
    ),
    m(&["golang.org/x/xerrors"], &["Errorf", "New"], false),
    m(
        &["github.com/pkg/errors"],
        &[
            "New",
            "Errorf",
            "Wrap",
            "Wrapf",
            "WithMessage",
            "WithMessagef",
        ],
        false,
    ),
];

/// A route matcher: the path argument at `arg` is ignored (when root-relative, for redirects).
struct Route {
    m: Matcher,
    arg: usize,
    root_relative: bool,
}

const fn r(
    pkgs: &'static [&'static str],
    names: &'static [&'static str],
    on_variable: bool,
    arg: usize,
    rr: bool,
) -> Route {
    Route {
        m: m(pkgs, names, on_variable),
        arg,
        root_relative: rr,
    }
}

const GIN: &[&str] = &["github.com/gin-gonic/gin"];
const HTTP: &[&str] = &["net/http"];
const ECHO: &[&str] = &["github.com/labstack/echo/v4"];
const CHI: &[&str] = &["github.com/go-chi/chi/v5"];
const MUX: &[&str] = &["github.com/gorilla/mux"];
const FIBER: &[&str] = &["github.com/gofiber/fiber/v2", "github.com/gofiber/fiber/v3"];
const FIBER2: &[&str] = &["github.com/gofiber/fiber/v2"];
const FIBER3: &[&str] = &["github.com/gofiber/fiber/v3"];
const HTTPROUTER: &[&str] = &["github.com/julienschmidt/httprouter"];
const BEEGO: &[&str] = &["github.com/beego/beego/v2/server/web"];
const GIN_ECHO_FNS: &[&str] = &[
    "GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS", "Any", "Group",
];
const HANDLE: &[&str] = &["Handle", "HandleFunc"];
const CHI_FNS: &[&str] = &[
    "Get",
    "Post",
    "Put",
    "Delete",
    "Patch",
    "Head",
    "Options",
    "Connect",
    "Trace",
    "Handle",
    "HandleFunc",
    "Route",
    "Mount",
];
const FIBER_FNS: &[&str] = &[
    "Get", "Post", "Put", "Delete", "Patch", "Head", "Options", "Connect", "Trace", "Route",
    "Mount", "All", "Group",
];
const BEEGO_FNS: &[&str] = &[
    "Get",
    "Post",
    "Put",
    "Delete",
    "Patch",
    "Head",
    "Options",
    "Any",
    "Router",
    "CtrlGet",
    "CtrlPost",
    "CtrlPut",
    "CtrlDelete",
    "CtrlPatch",
    "CtrlHead",
    "CtrlOptions",
    "CtrlAny",
];

const ROUTES: &[Route] = &[
    r(GIN, GIN_ECHO_FNS, true, 0, false),
    r(GIN, &["Handle", "Match"], true, 1, false),
    r(GIN, &["Redirect"], true, 1, true),
    r(HTTP, HANDLE, false, 0, false),
    r(HTTP, HANDLE, true, 0, false),
    r(HTTP, &["Redirect"], false, 2, true),
    r(HTTP, &["RedirectHandler"], false, 0, true),
    r(ECHO, GIN_ECHO_FNS, true, 0, false),
    r(ECHO, &["Add", "Match"], true, 1, false),
    r(ECHO, &["Redirect"], true, 1, true),
    r(CHI, CHI_FNS, true, 0, false),
    r(CHI, &["Method", "MethodFunc"], true, 1, false),
    r(
        MUX,
        &["Handle", "HandleFunc", "Path", "PathPrefix"],
        true,
        0,
        false,
    ),
    r(FIBER, FIBER_FNS, true, 0, false),
    r(FIBER, &["Add"], true, 1, false),
    r(FIBER3, &["To", "Back"], true, 0, true),
    r(FIBER2, &["Redirect", "RedirectBack"], true, 0, true),
    r(
        HTTPROUTER,
        &["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"],
        true,
        0,
        false,
    ),
    r(
        HTTPROUTER,
        &["Handle", "Handler", "HandlerFunc"],
        true,
        1,
        false,
    ),
    r(BEEGO, BEEGO_FNS, false, 0, false),
    r(BEEGO, BEEGO_FNS, true, 0, false),
];

/// How a call's function resolves: `pkg.Name` on an imported package, or `x.Name` on something else.
enum Callee<'a> {
    Package(&'a str, &'a str),
    Method(&'a str),
}

impl Matcher {
    fn matches(&self, callee: &Callee<'_>, imported: &[&str]) -> bool {
        match callee {
            Callee::Package(path, name) => self.pkgs.contains(path) && self.names.contains(name),
            Callee::Method(name) => {
                self.on_variable
                    && self.names.contains(name)
                    && self.pkgs.iter().any(|p| imported.contains(p))
            }
        }
    }
}

fn is_counted(content: &str) -> bool {
    content.encode_utf16().count() > MIN_LENGTH
        && !content
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

impl GoRule for Rule {
    fn run_once(&self, ctx: &Ctx<'_>) {
        let bindings = import_bindings(ctx);
        let imported: Vec<&str> = bindings.iter().map(|(_, p)| *p).collect();
        let dot_imports: Vec<&str> = bindings
            .iter()
            .filter(|(n, _)| *n == ".")
            .map(|(_, p)| *p)
            .collect();
        let mut excluded: FxHashSet<usize> = FxHashSet::default();
        let mut literals: Vec<Nd<'_>> = Vec::new();
        for &n in &ctx.nodes {
            match n.kind() {
                "interpreted_string_literal" => {
                    let is_tag = ctx.parent(n).is_some_and(|p| {
                        p.kind() == "field_declaration"
                            && p.child_by_field_name("tag")
                                .is_some_and(|t| t.id() == n.id())
                    });
                    if !is_tag {
                        literals.push(n);
                    }
                }
                "call_expression" if !imported.is_empty() => {
                    let Some(f) = n.child_by_field_name("function") else {
                        continue;
                    };
                    let callee = match f.kind() {
                        "selector_expression" => {
                            let (Some(op), Some(field)) = (
                                f.child_by_field_name("operand"),
                                f.child_by_field_name("field"),
                            ) else {
                                continue;
                            };
                            let name = ctx.text(field);
                            let pkg = (op.kind() == "identifier")
                                .then(|| {
                                    bindings
                                        .iter()
                                        .find(|(b, _)| *b == ctx.text(op))
                                        .map(|(_, p)| *p)
                                })
                                .flatten();
                            match pkg {
                                Some(p) => Callee::Package(p, name),
                                None => Callee::Method(name),
                            }
                        }
                        "identifier" if !dot_imports.is_empty() => {
                            let name = ctx.text(f);
                            match dot_imports.iter().find(|p| {
                                LOG_AND_ERRORS
                                    .iter()
                                    .chain(ROUTES.iter().map(|r| &r.m))
                                    .any(|m| m.pkgs.contains(p) && m.names.contains(&name))
                            }) {
                                Some(p) => Callee::Package(p, name),
                                None => continue,
                            }
                        }
                        _ => continue,
                    };
                    let args = call_args(n);
                    if LOG_AND_ERRORS.iter().any(|m| m.matches(&callee, &imported)) {
                        excluded.extend(
                            args.iter()
                                .filter(|a| a.kind() == "interpreted_string_literal")
                                .map(|a| a.id()),
                        );
                    }
                    for route in ROUTES {
                        if let Some(a) = args.get(route.arg)
                            && a.kind() == "interpreted_string_literal"
                            && route.m.matches(&callee, &imported)
                        {
                            let v = string_value(ctx, *a).unwrap_or("");
                            if !route.root_relative || (v.starts_with('/') && !v.starts_with("//"))
                            {
                                excluded.insert(a.id());
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        let mut order: Vec<&str> = Vec::new();
        let mut occurrences: FxHashMap<&str, Vec<Nd<'_>>> = FxHashMap::default();
        for lit in literals {
            if excluded.contains(&lit.id()) {
                continue;
            }
            let Some(content) = string_value(ctx, lit) else {
                continue;
            };
            if !is_counted(content) {
                continue;
            }
            let e = occurrences.entry(content).or_default();
            if e.is_empty() {
                order.push(content);
            }
            e.push(lit);
        }
        for content in order {
            let occ = &occurrences[content];
            if occ.len() >= self.threshold {
                ctx.report_issue(
                    span(occ[0]),
                    format!("Define a constant instead of duplicating this literal \"{content}\" {} times.", occ.len()),
                    occ[1..].iter().map(|o| (span(*o), Some("Duplication".to_string()))).collect(),
                    Some((occ.len() - 1) as f32),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn duplicates() {
        check(
            "S1192",
            r#"package p

import (
	"fmt"
	"log"
)

type T struct {
	A string `json:"field_a"`
	B string "tag literal"
}

func f() {
	x := "string literal1" // Noncompliant {{Define a constant instead of duplicating this literal "string literal1" 3 times.}}
	x += "string literal1" + "other"
	x = "string literal1"
	_ = "twice only"
	_ = "twice only"
	_ = "single_word"
	_ = "single_word"
	_ = "single_word"
	_ = "abcd e"[:1] // Noncompliant
	_ = "abcd e"
	_ = "abcd e"
	_ = `raw literal`
	_ = `raw literal`
	_ = `raw literal`
	log.Printf("log: operation failed")
	log.Printf("log: operation failed")
	log.Printf("log: operation failed")
	_ = fmt.Errorf("fmt: operation failed")
	_ = fmt.Errorf("fmt: operation failed")
	_ = fmt.Errorf("fmt: operation failed")
	fmt.Println("println is counted") // Noncompliant
	fmt.Println("println is counted")
	fmt.Println("println is counted")
	_ = "tag literal"
	_ = "tag literal"
}
"#,
        );
    }

    #[test]
    fn threshold_and_routes() {
        check_with(
            "S1192",
            serde_json::json!({"threshold": 2}),
            r#"package p

import (
	"net/http"

	"github.com/gin-gonic/gin"
)

func f(w http.ResponseWriter, r *http.Request) {
	router := gin.Default()
	router.GET("/users/:id", nil)
	router.POST("/users/:id", nil)
	http.HandleFunc("/http/users", nil)
	http.HandleFunc("/http/users", nil)
	http.Redirect(w, r, "/redirect/target", 302)
	http.Redirect(w, r, "/redirect/target", 302)
	http.Redirect(w, r, "https://example.com", 302) // Noncompliant
	http.Redirect(w, r, "https://example.com", 302)
}
"#,
        );
    }
}
