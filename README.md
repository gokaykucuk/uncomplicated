# uncomplicated (`unc`)

Fast static analysis for **JavaScript, TypeScript and Go**, built for CI. It implements
SonarQube's rule set, metrics, duplication detection, coverage import, new-code detection and
quality gate, as a single native binary with no server, no JVM and no Node.js.

- **Rules:** SonarJS's JS/TS rules and sonar-go's Go rules (same keys, defaults, messages and
  locations), plus extra Go rules (`UG*`) modeled on go vet, staticcheck and gosec.
- **Metrics:** lines, ncloc, comment lines, functions, classes, statements, cyclomatic and
  cognitive complexity, technical debt, and A–E reliability, security and maintainability ratings.
- **Duplication:** token-based, cross-file, with SonarQube's defaults (100 tokens, 10 lines).
- **Coverage:** LCOV, Go cover profiles and Cobertura XML.
- **New code:** lines changed since the merge-base with a git ref, including uncommitted and
  untracked files.
- **Quality gate:** SonarQube's "Sonar way" gate by default, configurable.
- **Reports:** text, JSON, SARIF 2.1.0, GitLab Code Quality, GitHub annotations, and SonarQube's
  generic issue format (so results can still be imported into a SonarQube server).

## Usage

```bash
unc                                # analyze the current directory
unc path/to/project --new-code origin/main --lcov coverage/lcov.info
unc -f sarif -o unc.sarif          # machine-readable report
unc --report gitlab=gl-code-quality.json --report sarif=unc.sarif
unc rules                          # list implemented rules
unc rules --missing                # catalog rules not implemented yet
unc init                           # write a starter uncomplicated.toml
```

Exit status: `0` gate passed, `1` gate failed (or `--fail-on <severity>` matched), `2` error.

### GitHub Actions

```yaml
- run: unc --new-code origin/${{ github.base_ref }} --lcov coverage/lcov.info -f github
```

### GitLab CI

```yaml
code_quality:
  script: unc --new-code origin/$CI_MERGE_REQUEST_TARGET_BRANCH_NAME --report gitlab=gl-code-quality.json
  artifacts:
    reports:
      codequality: gl-code-quality.json
```

Results are cached by content hash in `.uncomplicated/cache.bin`. Cache that directory between CI
runs and unchanged files are not re-analyzed.

## Configuration

Projects already set up for SonarQube work as-is. Without an `uncomplicated.toml`, `unc` reads
`sonar-project.properties`: sources, tests, inclusions and exclusions, coverage report paths, CPD
exclusions and `sonar.issue.ignore.multicriteria` rules.

To get exactly what a specific SonarQube server reports, pin its rule set:
`--profile sonarqube-26.9` uses the Sonar way profile and rule behaviour of SonarQube 26.9
(SonarJS 13.8.0, sonar-go 1.43.0).

Otherwise, use `uncomplicated.toml` at the project root. Every key is optional:

```toml
[analysis]
profile = "sonar-way"          # "sonar-way" (default), "all", or "none"
exclusions = ["generated/**"]  # root-relative globs (use **/ to match anywhere), added to the defaults
tests = ["test/**"]            # test code globs (default: Sonar's file-name heuristics + *_test.go)

[new_code]
reference = "origin/main"

[coverage]
lcov = ["coverage/lcov.info"]
go = ["coverage.out"]

[cpd]
min_tokens = 100
min_lines = 10

[rules]                        # JS/TS rules
S3776 = { threshold = 20 }
S1135 = "off"

[go_rules]
S1192 = "off"

[[gate.conditions]]
metric = "new_coverage"
op = "lt"
threshold = 80
```

Like SonarQube, `unc` honors `// NOSONAR`, and for JS/TS also `eslint-disable` directives and
inline `/* eslint rule: "off" */` comments. Rules SonarJS only enables for some projects follow the
same conditions: dependencies declared in `package.json` (React, Vue, Angular, AWS CDK, testing
libraries...) and the ECMAScript version detected from `tsconfig.json` and the Node.js version. It skips
bundles, minified files and generated Go code, and also analyzes the scripts embedded in `.vue` and
`.html` files.

## Results

Measured on a 12-core laptop with the release build, cold cache, and every rule of the default
profile enabled:

| Corpus | Files | Lines of code | Wall time | Peak memory |
|---|---|---|---|---|
| SonarJS ruling corpus (JS/TS, 53 projects) | 20,417 | 2.35M | 1.4–1.7 s | 0.9 GB |
| Kubernetes (Go) | 9,724 | 2.23M | 3.2 s | 0.55 GB |
| Prometheus (Go) | 998 | 345k | 0.5 s | 0.2 GB |

With a warm cache (nothing changed), the JS/TS corpus takes 0.4 s.

On a real monorepo (polybase: 7,300 JS/TS/Go files, 1.5M lines), `unc` takes 0.9–1.5 s. The
project's own SonarQube pipeline takes 633 s in the scanner (695 s with server startup). With
`--profile sonarqube-26.9` both report the same issues on the committed code.

Parity with SonarJS on its own ruling corpus, comparing issues per rule at file/line granularity:
471 rules compared, **99.9% precision, 97.5% recall**, 352 rules matching exactly. Most remaining
misses are rules where Sonar uses the full TypeScript type checker or cross-file types. Our
local inference stays silent there rather than guess.

Rule coverage: 554 of the 555 active SonarJS rules (S7742 is missing because it needs the
core-js-compat dataset), all 37 sonar-go rules, and 55 extra Go rules (`UG1001`–`UG1055`).

## How it's fast

- Native parsers: [oxc](https://oxc.rs) for JS/TS (parser, semantic model, control-flow graph) and
  tree-sitter for Go. Sonar runs the TypeScript compiler in Node.js to get type information. We
  use local type inference instead (literals, annotations, declarations, known APIs). It's precise
  but stays silent when it can't tell.
- Each file is parsed once into a reusable per-thread arena. Each rule subscribes to the node types
  it cares about, and every file's node list is walked once.
- Files are analyzed in parallel on all cores. Duplication detection is a parallel rolling hash
  plus sort. Everything is cached by content hash.

## Parity

`tools/parity.py` runs `unc` over SonarJS's ruling corpus (~60 real-world projects) and compares
issues with SonarJS's recorded expectations, per rule, at file/line granularity:

```bash
python3 tools/parity.py --unc target/release/unc --sonarjs <SonarJS checkout> --sources <jsts-test-sources>
```

## Layout

```
crates/uc-core     rule catalog, issue/metric types, line index
crates/uc-js       JS/TS frontend (oxc) and rules (src/rules/sNNNN.rs)
crates/uc-go       Go frontend (tree-sitter) and rules (src/rules/sNNNN.rs, ugNNNN.rs)
crates/uc-engine   discovery, cache, CPD, coverage, new code, measures, gate, reports
crates/uc-cli      the `unc` binary
catalog/           rule metadata (keys, types, severities, default profile, remediation)
docs/              rule authoring guides
tools/parity.py    parity harness against SonarJS's ruling expectations
```
