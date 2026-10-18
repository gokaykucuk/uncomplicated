//! `unc` — fast static analysis for JavaScript, TypeScript and Go, with SonarQube-compatible
//! rules, metrics and quality gates.

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use uc_core::{Catalog, Lang, Severity};
use uc_engine::config::{Condition, Config, RuleSetting};
use uc_engine::gate::Status;
use uc_engine::report::{Filter, Format};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[derive(Parser)]
#[command(name = "unc", version, about = "Fast static analysis for JS/TS and Go (SonarQube-compatible rules and quality gates)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Analyze a project (default command).
    Analyze(AnalyzeArgs),
    /// List rules.
    Rules(RulesArgs),
    /// Write a starter uncomplicated.toml.
    Init {
        #[arg(default_value = ".")]
        dir: PathBuf,
    },
}

#[derive(Args)]
struct AnalyzeArgs {
    /// Files or directories to analyze (default: the project root).
    paths: Vec<PathBuf>,
    /// Project root (where uncomplicated.toml, .gitignore and coverage paths are resolved).
    #[arg(long, short = 'C')]
    root: Option<PathBuf>,
    /// Configuration file (default: <root>/uncomplicated.toml).
    #[arg(long, short)]
    config: Option<PathBuf>,
    /// Output format: text, summary, json, sarif, gitlab, github, sonar.
    #[arg(long, short, default_value = "text")]
    format: String,
    /// Write the report to a file instead of stdout.
    #[arg(long, short)]
    output: Option<PathBuf>,
    /// Additional reports, as FORMAT=FILE (repeatable), e.g. --report sarif=out.sarif.
    #[arg(long = "report", value_name = "FORMAT=FILE")]
    reports: Vec<String>,
    /// Git ref new code is compared against (e.g. origin/main).
    #[arg(long, value_name = "REF")]
    new_code: Option<String>,
    /// LCOV coverage report(s).
    #[arg(long, value_name = "FILE")]
    lcov: Vec<String>,
    /// Go cover profile(s).
    #[arg(long, value_name = "FILE")]
    go_coverage: Vec<String>,
    /// Cobertura XML report(s).
    #[arg(long, value_name = "FILE")]
    cobertura: Vec<String>,
    /// Rule profile: sonar-way (default) or all.
    #[arg(long)]
    profile: Option<String>,
    /// Enable/disable rules: S1234=off, S1234=on, go:S1234=off (repeatable).
    #[arg(long = "rule", value_name = "KEY=on|off")]
    rule_overrides: Vec<String>,
    /// Only report issues on new code.
    #[arg(long)]
    new_only: bool,
    /// Only report issues at or above this severity (info, minor, major, critical, blocker).
    #[arg(long)]
    min_severity: Option<String>,
    /// Exit with status 1 if any reported issue has at least this severity.
    #[arg(long, value_name = "SEVERITY")]
    fail_on: Option<String>,
    /// Never fail because of the quality gate.
    #[arg(long)]
    no_gate: bool,
    /// Disable the result cache.
    #[arg(long)]
    no_cache: bool,
    /// Cache location (default: <root>/.uncomplicated/cache.bin).
    #[arg(long)]
    cache: Option<PathBuf>,
    /// Worker threads (default: all cores).
    #[arg(long, short = 'j')]
    threads: Option<usize>,
    /// Disable duplication detection.
    #[arg(long)]
    no_cpd: bool,
    /// Ignore NOSONAR comments.
    #[arg(long)]
    no_nosonar: bool,
    /// Colors: auto, always, never.
    #[arg(long, default_value = "auto")]
    color: String,
}

#[derive(Args)]
struct RulesArgs {
    /// js or go.
    #[arg(long)]
    lang: Option<String>,
    /// Show rules from the Sonar catalog that are not implemented yet.
    #[arg(long)]
    missing: bool,
    /// Only rules active in the given profile (sonar-way or all).
    #[arg(long)]
    profile: Option<String>,
    #[arg(long)]
    json: bool,
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().collect();
    let known = ["analyze", "rules", "init", "help", "--help", "-h", "--version", "-V"];
    if args.len() < 2 || !known.contains(&args[1].as_str()) {
        args.insert(1, "analyze".into());
    }
    let cli = Cli::parse_from(args);
    let res = match cli.cmd {
        Cmd::Analyze(a) => analyze(a),
        Cmd::Rules(r) => rules(r),
        Cmd::Init { dir } => init(dir),
    };
    match res {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}

fn analyze(a: AnalyzeArgs) -> Result<ExitCode, String> {
    let root = match &a.root {
        Some(r) => r.clone(),
        None => match a.paths.as_slice() {
            [single] if single.is_dir() => single.clone(),
            _ => PathBuf::from("."),
        },
    };
    let root = root.canonicalize().map_err(|e| format!("{}: {e}", root.display()))?;
    let mut cfg = Config::load(&root, a.config.as_deref())?;
    if let Some(p) = &a.profile {
        cfg.analysis.profile = p.clone();
    }
    if a.new_code.is_some() {
        cfg.new_code.reference = a.new_code.clone();
    }
    cfg.coverage.lcov.extend(a.lcov.iter().cloned());
    cfg.coverage.go.extend(a.go_coverage.iter().cloned());
    cfg.coverage.cobertura.extend(a.cobertura.iter().cloned());
    if a.no_cpd {
        cfg.cpd.enabled = false;
    }
    if a.no_nosonar {
        cfg.analysis.nosonar = false;
    }
    for o in &a.rule_overrides {
        let (k, v) = o.split_once('=').ok_or_else(|| format!("bad --rule {o:?}, expected KEY=on|off"))?;
        let setting = RuleSetting::Toggle(v.to_string());
        match k.strip_prefix("go:") {
            Some(k) => cfg.go_rules.insert(k.to_string(), setting),
            None => cfg.rules.insert(k.trim_start_matches("js:").to_string(), setting),
        };
    }
    let paths: Vec<PathBuf> = if a.paths.len() == 1 && a.paths[0].canonicalize().ok().as_ref() == Some(&root) {
        Vec::new()
    } else {
        a.paths.iter().map(|p| p.canonicalize().map_err(|e| format!("{}: {e}", p.display()))).collect::<Result<_, _>>()?
    };
    let cache = if a.no_cache { None } else { Some(a.cache.clone().unwrap_or_else(|| root.join(".uncomplicated/cache.bin"))) };
    let format = Format::parse(&a.format).ok_or_else(|| format!("unknown format {:?}", a.format))?;
    let min_severity = a.min_severity.as_deref().map(|s| Severity::parse(s).ok_or(format!("bad severity {s}"))).transpose()?;
    let fail_on = a.fail_on.as_deref().map(|s| Severity::parse(s).ok_or(format!("bad severity {s}"))).transpose()?;

    let analysis = uc_engine::run(uc_engine::Options { root, paths, config: cfg, cache, threads: a.threads })?;

    let filter = Filter { new_only: a.new_only, min_severity };
    let color = match a.color.as_str() {
        "always" => true,
        "never" => false,
        _ => a.output.is_none() && std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none(),
    };
    let out = uc_engine::report::render(&analysis, format, &filter, color);
    match &a.output {
        Some(p) => std::fs::write(p, out).map_err(|e| format!("{}: {e}", p.display()))?,
        None => {
            let mut stdout = std::io::stdout().lock();
            let _ = stdout.write_all(out.as_bytes());
        }
    }
    for r in &a.reports {
        let (f, file) = r.split_once('=').ok_or_else(|| format!("bad --report {r:?}, expected FORMAT=FILE"))?;
        let f = Format::parse(f).ok_or_else(|| format!("unknown format {f:?}"))?;
        std::fs::write(file, uc_engine::report::render(&analysis, f, &filter, false)).map_err(|e| format!("{file}: {e}"))?;
    }
    if format != Format::Text && format != Format::Summary && a.output.is_none() {
        // Machine-readable output on stdout; keep the gate status visible on stderr.
        eprintln!("Quality gate: {}", if analysis.gate.status == Status::Ok { "PASSED" } else { "FAILED" });
    }

    if std::env::var_os("UNC_PROFILE").is_some() {
        let prof = uc_engine::profile_report();
        let total: u64 = prof.iter().map(|p| p.1).sum();
        eprintln!("rule time (summed across threads): {:.0} ms", total as f64 / 1e6);
        for (k, ns) in prof.iter().take(25) {
            eprintln!("  {k:<8} {:>8.1} ms", *ns as f64 / 1e6);
        }
    }
    let gate_failed = !a.no_gate && analysis.gate.status == Status::Error;
    let issues_failed = fail_on.is_some_and(|s| uc_engine::report::items(&analysis, &filter).iter().any(|i| i.meta.severity >= s));
    Ok(if gate_failed || issues_failed { ExitCode::from(1) } else { ExitCode::SUCCESS })
}

fn rules(r: RulesArgs) -> Result<ExitCode, String> {
    let catalog = Catalog::get();
    let js: std::collections::HashSet<&str> = uc_engine::implemented_js().collect();
    let go: std::collections::HashSet<&str> = uc_engine::implemented_go().collect();
    let mut rows = Vec::new();
    for m in &catalog.rules {
        let is_go = m.langs.contains(&Lang::Go);
        if r.lang.as_deref().is_some_and(|l| (l == "go") != is_go) {
            continue;
        }
        if m.deprecated {
            continue;
        }
        if r.profile.as_deref() == Some("sonar-way") && !m.sonar_way {
            continue;
        }
        let implemented = if is_go { go.contains(m.key.as_str()) } else { js.contains(m.key.as_str()) };
        if r.missing == implemented {
            continue;
        }
        rows.push((if is_go { "go" } else { "js" }, m, implemented));
    }
    if r.json {
        let v: Vec<_> = rows
            .iter()
            .map(|(l, m, i)| {
                serde_json::json!({"lang": l, "key": m.key, "title": m.title, "type": m.kind.as_str(),
                    "severity": m.severity.as_str(), "sonarWay": m.sonar_way, "implemented": i})
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&v).unwrap());
    } else {
        for (l, m, _) in &rows {
            println!("{l}:{:<7} {:<8} {:<16} {}{}", m.key, m.severity.as_str(), m.kind.as_str(), m.title, if m.sonar_way { "" } else { "  (not in sonar-way)" });
        }
        eprintln!("{} rules", rows.len());
    }
    Ok(ExitCode::SUCCESS)
}

fn init(dir: PathBuf) -> Result<ExitCode, String> {
    let path = dir.join(uc_engine::config::CONFIG_FILE);
    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }
    let gate: Vec<Condition> = uc_engine::gate::sonar_way();
    let mut s = String::from(
        "# uncomplicated configuration. All keys are optional.\n\n[analysis]\n# profile = \"sonar-way\"   # or \"all\"\n# exclusions = [\"generated/**\"]\n# tests = [\"**/*.test.*\", \"**/*_test.go\"]\n\n[new_code]\n# reference = \"origin/main\"\n\n[coverage]\n# lcov = [\"coverage/lcov.info\"]\n# go = [\"coverage.out\"]\n\n[cpd]\n# min_tokens = 100\n# min_lines = 10\n\n[rules]\n# S3776 = { threshold = 20 }\n# S1135 = \"off\"\n\n[go_rules]\n# S1192 = \"off\"\n\n# Default quality gate (Sonar way):\n",
    );
    for c in gate {
        s.push_str(&format!("# [[gate.conditions]]\n# metric = \"{}\"\n# op = \"{}\"\n# threshold = {}\n", c.metric, c.op, c.threshold));
    }
    std::fs::write(&path, s).map_err(|e| e.to_string())?;
    println!("wrote {}", path.display());
    Ok(ExitCode::SUCCESS)
}
