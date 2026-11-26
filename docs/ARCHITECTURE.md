# How `unc` works

One run is a pipeline. Everything up to the cache is per file and runs on all cores; the steps
after it look across files.

```
discover ─▶ per-file analysis (parallel, cached) ─▶ duplication ─▶ save cache
        ─▶ cross-file passes ─▶ ignores / baseline ─▶ coverage, new code ─▶ measures ─▶ gate ─▶ reports
```

## Crates

| Crate | Role |
|---|---|
| `uc-core` | Rule catalog (`catalog/*.json`, embedded at build time), repositories, issue/metric/result types, line index, per-file summaries |
| `uc-js` | JS/TS frontend: oxc parser, semantic model and CFG; SonarJS rules (`rules/sNNNN.rs`), extra rules (`rules/ujNNNN.rs`), local type inference, taint engine |
| `uc-go` | Go frontend: tree-sitter-go, flat node index; sonar-go rules and extra `UG` rules |
| `uc-secrets` | Secret detection over any text: one Aho-Corasick keyword pass, then per-rule regexes around the hits |
| `uc-infra` | Dockerfile, Kubernetes, GitHub Actions and Terraform rules |
| `uc-css` | CSS/SCSS/Less tokenizer and SonarQube's CSS rules |
| `uc-engine` | Discovery, cache, duplication, cross-file passes (`gopkg`, `jsgraph`), dependency scan (`deps`), coverage, new code, measures, gate, baseline, reports |
| `uc-cli` | The `unc` binary |

## Per-file analysis

`discover.rs` walks the tree once (gitignore-aware, parallel) and classifies files by name: JS/TS,
Go, stylesheet, infrastructure (`uc_infra::kind_of`), other text (secrets only) and lockfiles.
Each file is read once and dispatched to its frontend; the secrets scanner then runs over the same
text. A frontend returns a `FileResult`: issues with resolved line/column positions, metrics,
duplication tokens, and a summary for the cross-file passes.

The JS and Go frontends parse each file once into a reusable per-thread arena. Rules subscribe to
node types; the engine walks the node list once and calls every subscribed rule, so adding a rule
costs only its own work on the nodes it cares about.

## Cache

`.uncomplicated/cache.bin` maps each file to its result. A file is unchanged when its size and
modification time match (it isn't even read), or else when its content hash matches. The key also
covers the binary, the configuration, the enabled rules and the file's project context
(`package.json`/`tsconfig.json` for JS/TS, `go.mod` for Go), so any change there re-analyzes the
files concerned.

Cross-file results are never cached. The passes rerun on every analysis from the cached
summaries, which is cheap: no file is parsed again.

## Cross-file passes

- `gopkg.rs` groups Go files by package (directory + package clause) and checks what needs every
  file of a package, such as unused unexported declarations or ignored errors of package functions.
- `jsgraph.rs` resolves the imports recorded in JS/TS summaries (relative paths, tsconfig
  `paths`/`baseUrl`) and checks the module graph: unresolved imports, missing exports, cycles,
  unused exports.
- `deps.rs` reads lockfiles and matches pinned versions against the OSV index built by
  `unc deps update`.

## Rules and repositories

A rule is identified by its repository and key (`js:S1481`, `go:UG1018`, `docker:DKR001`).
Metadata (title, type, severity, default activation, remediation effort, tags) lives in
`catalog/<repo>.json`. Implementations are discovered by file name in `uc-js` and `uc-go`
(`build.rs`), and listed in `IMPLEMENTED` constants in the other crates. `unc rules --missing`
shows catalog rules without an implementation.

Types follow SonarQube: bugs drive the reliability rating, vulnerabilities the security rating,
code smells the technical debt and maintainability rating. Security hotspots are reported for
review but don't affect ratings or the gate.

## Measures and gate

`measures.rs` computes SonarQube's metric keys from the per-file results, duplication, coverage
and the new-code line sets (`git diff` against the merge-base, plus untracked files). `gate.rs`
evaluates the conditions; reports render the same `Analysis` in every output format.

## Adding rules

- JS/TS: [RULE_AUTHORING.md](RULE_AUTHORING.md)
- Go: [GO_RULE_AUTHORING.md](GO_RULE_AUTHORING.md)
- Other repositories: add the entry to `catalog/<repo>.json`, implement it in the crate, add its key
  to the crate's `IMPLEMENTED` list, and add tests next to the rule.
