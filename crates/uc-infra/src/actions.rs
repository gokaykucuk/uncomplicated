//! GitHub Actions workflow rules (`GHA*`).

use std::sync::LazyLock;

use regex::Regex;

use crate::util::{Out, commands, span};
use crate::yaml::{Doc, V, Y};

pub const RULES: &[&str] = &["GHA001", "GHA002", "GHA003", "GHA004", "GHA005", "GHA006", "GHA007", "GHA008"];

/// Contexts an external contributor controls.
static UNTRUSTED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)\bgithub\s*\.\s*(?:
            head_ref
          | event\.(?:
                (?:issue|pull_request|discussion)\.(?:title|body)
              | (?:comment|review|review_comment)\.body
              | pull_request\.head\.(?:ref|label|repo\.default_branch)
              | pages(?:\[[^\]]*\]|\.\*)\.page_name
              | commits(?:\[[^\]]*\]|\.\*)\.(?:message|author\.(?:email|name))
              | head_commit\.(?:message|author\.(?:email|name)|committer\.(?:email|name))
              | workflow_run\.(?:head_branch|display_title|head_commit\.(?:message|author\.(?:email|name))
                  |pull_requests(?:\[[^\]]*\]|\.\*)\.head\.ref)
            )
        )\b",
    )
    .unwrap()
});

/// `${{ ... }}` expressions in `s`, as byte ranges (including the delimiters).
fn expressions(s: &str) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut i = 0;
    std::iter::from_fn(move || {
        let start = i + memchr::memmem::find(&s.as_bytes()[i..], b"${{")?;
        let end = memchr::memmem::find(&s.as_bytes()[start..], b"}}").map_or(s.len(), |k| start + k + 2);
        i = end;
        Some((start, end))
    })
}

fn triggers(root: &Y) -> Vec<String> {
    // `on` (a YAML 1.1 boolean to some parsers) is a plain key here.
    let Some(on) = root.get("on").or_else(|| root.get("true")) else { return Vec::new() };
    match &on.v {
        V::Scalar { text, .. } => vec![text.clone()],
        V::Seq(items) => items.iter().filter_map(|i| i.str().map(str::to_string)).collect(),
        V::Map(pairs) => pairs.iter().map(|p| p.key.clone()).collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn check(src: &str, doc: &Doc, out: &mut Out) {
    if doc.has_error || !doc.root.is_map() {
        return;
    }
    let root = &doc.root;
    let triggers = triggers(root);
    let has = |t: &str| triggers.iter().any(|x| x == t);
    permissions(root, out);
    env(root, out);
    for job in root.get("jobs").map_or(&[][..], Y::pairs) {
        let job = &job.val;
        permissions(job, out);
        env(job, out);
        if let Some(u) = job.get("uses") {
            uses(u, out);
        }
        let steps = job.get("steps").map_or(&[][..], Y::seq);
        for (i, step) in steps.iter().enumerate() {
            env(step, out);
            let action = step.get("uses");
            if let Some(u) = action {
                uses(u, out);
            }
            let action_name = action.and_then(Y::str).unwrap_or("");
            if let Some(run) = step.get("run")
                && matches!(run.v, V::Scalar { .. })
            {
                script(src, run, true, out);
            }
            if action_name.to_ascii_lowercase().starts_with("actions/github-script@")
                && let Some(s) = step.path(&["with", "script"])
            {
                script(src, s, false, out);
            }
            if out.on("GHA002")
                && action_name.to_ascii_lowercase().starts_with("actions/checkout@")
                && steps[i + 1..].iter().any(|s| runs_checked_out_code(src, s))
                && let Some(with) = step.get("with")
            {
                for key in ["ref", "repository"] {
                    let Some(v) = with.get(key) else { continue };
                    let raw = v.raw(src);
                    let pr_head = raw.contains("github.event.pull_request.head.")
                        || raw.contains("github.head_ref")
                        || raw.contains("github.event.pull_request.merge_commit_sha");
                    let run_head = raw.contains("github.event.workflow_run.head_");
                    let pull_ref = raw.contains("refs/pull/");
                    let bad = (has("pull_request_target") && (pr_head || pull_ref))
                        || (has("workflow_run") && run_head)
                        || (has("issue_comment") && pull_ref);
                    if bad {
                        let trigger = if run_head {
                            "workflow_run"
                        } else if pr_head || has("pull_request_target") {
                            "pull_request_target"
                        } else {
                            "issue_comment"
                        };
                        out.emit(
                            "GHA002",
                            v.span,
                            format!(
                                "Don't check out untrusted pull request code in a privileged \"{trigger}\" workflow."
                            ),
                        );
                        break;
                    }
                }
            }
        }
    }
}

/// Build and test tools, package managers and local scripts: commands that execute code from the
/// checked-out tree.
static EXEC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)(?:^|[\s;&|(])(?:npm|npx|yarn|pnpm|bun|make|cmake|pip3?|poetry|pipenv|tox|nox|pytest|cargo|go|mvn|gradle|ant|bundle|rake|composer|dotnet|docker|docker-compose|pre-commit|deno|mix|sbt|lerna|nx|turbo|\./[\w./-]+|python3?\s+setup\.py)(?:$|[\s;&|)])",
    )
    .unwrap()
});

/// True if a step may execute code from the checked-out tree.
fn runs_checked_out_code(src: &str, step: &Y) -> bool {
    if let Some(run) = step.get("run") {
        return EXEC.is_match(run.raw(src));
    }
    // Local actions and build actions run repository code.
    step.get("uses")
        .and_then(Y::str)
        .is_some_and(|u| u.starts_with("./") || u.starts_with("docker/build-push-action") || u.starts_with("gradle/"))
}

fn permissions(y: &Y, out: &mut Out) {
    if let Some(p) = y.get("permissions")
        && p.str() == Some("write-all")
    {
        out.emit("GHA003", p.span, "Grant only the permissions this workflow needs instead of \"write-all\".");
    }
}

fn env(y: &Y, out: &mut Out) {
    if let Some(p) = y.get("env").and_then(|e| e.pair("ACTIONS_ALLOW_UNSECURE_COMMANDS"))
        && (p.val.is_true() || p.val.str() == Some("true"))
    {
        out.emit(
            "GHA005",
            p.key_span,
            "Remove ACTIONS_ALLOW_UNSECURE_COMMANDS; it re-enables the insecure set-env and add-path commands.",
        );
    }
}

/// A `run:` shell script (`shell`) or a github-script `script:`.
fn script(src: &str, node: &Y, shell: bool, out: &mut Out) {
    let raw = node.raw(src);
    let base = node.span.start as usize;
    if out.on("GHA001") {
        for (s, e) in expressions(raw) {
            let expr = &raw[s..e];
            if UNTRUSTED.is_match(expr)
                && !["==", "!=", "contains(", "startsWith(", "endsWith("].iter().any(|op| expr.contains(op))
            {
                let what = if shell { "shell script" } else { "script" };
                out.emit(
                    "GHA001",
                    span(base + s, base + e),
                    format!("Pass this untrusted value through an environment variable instead of expanding it into the {what}."),
                );
            }
        }
    }
    if !shell {
        return;
    }
    if out.on("GHA006") {
        for cmd in ["::set-output ", "::save-state ", "::set-env ", "::add-path::"] {
            for (k, _) in raw.match_indices(cmd) {
                let name = cmd.trim_matches(|c| c == ':' || c == ' ');
                let fix = match name {
                    "set-output" => "write to $GITHUB_OUTPUT",
                    "save-state" => "write to $GITHUB_STATE",
                    "set-env" => "write to $GITHUB_ENV",
                    _ => "write to $GITHUB_PATH",
                };
                out.emit(
                    "GHA006",
                    span(base + k, base + k + cmd.trim_end().len()),
                    format!("Replace the deprecated \"{name}\" command: {fix}."),
                );
            }
        }
    }
    if out.on("GHA005")
        && let Some(k) = raw.find("ACTIONS_ALLOW_UNSECURE_COMMANDS=true")
    {
        out.emit(
            "GHA005",
            span(base + k, base + k + 31),
            "Remove ACTIONS_ALLOW_UNSECURE_COMMANDS; it re-enables the insecure set-env and add-path commands.",
        );
    }
    if out.on("GHA004") && raw.contains("secrets.") {
        let text = node.str().unwrap_or("");
        let mut from = 0;
        for c in commands(text) {
            let printed = matches!(c.name(), Some("echo" | "printf")) && !c.redirected && c.sep != "|";
            let (a, b) = (c.words[0].start, c.words.last().unwrap().end);
            let ctext = &text[a..b];
            let mut reported = false;
            for (s, e) in expressions(ctext).filter(|&(s, e)| ctext[s..e].contains("secrets.")) {
                let expr = &ctext[s..e];
                let Some(k) = raw[from..].find(expr) else { break };
                let at = from + k;
                from = at + expr.len();
                if printed && !reported {
                    reported = true;
                    out.emit(
                        "GHA004",
                        span(base + at, base + at + expr.len()),
                        "Make sure printing this secret to the build log is safe here.",
                    );
                }
            }
        }
    }
}

/// Minimum supported major versions of core actions (older ones run on removed Node.js runtimes
/// or use retired services).
const CORE_MIN: &[(&str, u32)] = &[
    ("checkout", 4),
    ("setup-node", 4),
    ("setup-python", 5),
    ("setup-go", 5),
    ("setup-java", 4),
    ("setup-dotnet", 4),
    ("cache", 4),
    ("upload-artifact", 4),
    ("download-artifact", 4),
    ("github-script", 7),
    ("upload-pages-artifact", 3),
];

fn uses(u: &Y, out: &mut Out) {
    let Some(s) = u.str() else { return };
    if s.starts_with("./") || s.starts_with("docker://") || s.contains("${{") {
        return;
    }
    let Some((name, git_ref)) = s.split_once('@') else { return };
    let owner = name.split('/').next().unwrap_or("").to_ascii_lowercase();
    let pinned = git_ref.len() == 40 && git_ref.bytes().all(|b| b.is_ascii_hexdigit());
    if owner == "actions" || owner == "github" {
        if let Some(action) = name.strip_prefix("actions/")
            && let Some(&(_, min)) = CORE_MIN.iter().find(|(a, _)| *a == action)
            && let Some(major) =
                git_ref.strip_prefix('v').and_then(|v| v.split('.').next()).and_then(|v| v.parse::<u32>().ok())
            && major < min
        {
            out.emit(
                "GHA008",
                u.span,
                format!("Upgrade {name} to v{min} or later; v{major} relies on a deprecated runtime or service."),
            );
        }
        return;
    }
    if !pinned && name.contains('/') {
        out.emit(
            "GHA007",
            u.span,
            "Make sure using this third-party action without pinning it to a full commit SHA is safe.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::run;

    fn w(src: &str) -> Vec<(String, u32)> {
        run(".github/workflows/ci.yml", src, crate::Kind::Workflow)
    }

    #[test]
    fn injection() {
        let src = "on: issues
jobs:
  a:
    runs-on: ubuntu-latest
    steps:
      - run: echo \"${{ github.event.issue.title }}\"
      - run: |
          echo ok
          echo \"${{ github.head_ref }}\"
      - uses: actions/github-script@v7
        with:
          script: console.log('${{ github.event.comment.body }}')
      - run: echo \"$TITLE\"
        env:
          TITLE: ${{ github.event.issue.title }}
      - if: contains(github.event.issue.body, 'x')
        run: echo \"${{ contains(github.event.issue.body, 'x') }} ${{ github.event.issue.number }}\"
";
        assert_eq!(w(src), vec![("GHA001".into(), 6), ("GHA001".into(), 9), ("GHA001".into(), 12)]);
    }

    #[test]
    fn pwn_request() {
        let src = "on:
  pull_request_target:
    types: [opened]
jobs:
  a:
    steps:
      - uses: actions/checkout@v4
        with:
          ref: ${{ github.event.pull_request.head.sha }}
      - run: npm test
";
        assert_eq!(w(src), vec![("GHA002".into(), 9)]);
        // Same checkout in a pull_request workflow is fine.
        assert_eq!(w(&src.replace("pull_request_target", "pull_request")), vec![]);
        // Nothing runs the checked-out code.
        assert_eq!(w(&src.replace("npm test", "gh pr edit 1 --add-label x")), vec![]);
        // Checking out the base is fine.
        assert_eq!(w(&src.replace("${{ github.event.pull_request.head.sha }}", "main")), vec![]);
    }

    #[test]
    fn permissions_env_commands() {
        let src = "on: push
permissions: write-all
env:
  ACTIONS_ALLOW_UNSECURE_COMMANDS: true
jobs:
  a:
    permissions: {contents: read}
    steps:
      - run: |
          echo \"::set-output name=x::1\"
          echo \"x=1\" >> $GITHUB_OUTPUT
          echo ${{ secrets.TOKEN }}
          echo \"${{ secrets.TOKEN }}\" | docker login --password-stdin
          echo \"T=${{ secrets.TOKEN }}\" >> $GITHUB_ENV
";
        assert_eq!(
            w(src),
            vec![("GHA003".into(), 2), ("GHA005".into(), 4), ("GHA006".into(), 10), ("GHA004".into(), 12)]
        );
    }

    #[test]
    fn action_versions() {
        let src = "on: push
jobs:
  a:
    uses: org/repo/.github/workflows/x.yml@main
  b:
    steps:
      - uses: actions/checkout@v3
      - uses: actions/checkout@v4
      - uses: actions/upload-artifact@v3.1.2
      - uses: docker/login-action@v3
      - uses: docker/login-action@0123456789abcdef0123456789abcdef01234567
      - uses: github/codeql-action/init@v3
      - uses: ./local
      - uses: docker://alpine:3
";
        assert_eq!(
            w(src),
            vec![("GHA007".into(), 4), ("GHA008".into(), 7), ("GHA008".into(), 9), ("GHA007".into(), 10)]
        );
    }

    #[test]
    fn metrics() {
        let r = crate::tests::result(".github/workflows/x.yml", "# c\non: push\njobs: {}\n", crate::Kind::Workflow);
        assert_eq!(r.lang, Some(uc_core::Lang::Yaml));
        assert_eq!((r.metrics.lines, r.metrics.ncloc, r.metrics.comment_lines), (4, 2, 1));
    }
}
