# Configuration

`unc` works without configuration. When it needs some, it reads, in order:

1. the file given with `--config FILE`;
2. `uncomplicated.toml` at the project root;
3. `sonar-project.properties` at the project root (see [SonarQube projects](#sonarqube-projects)).

Command-line flags override the file. `unc init` writes a commented starter file.

## Full reference

Every key is optional; the values shown are the defaults.

```toml
[analysis]
profile = "sonar-way"            # "sonar-way", "all", "none", or a pinned profile such as "sonarqube-26.9"
sources = []                     # directories to analyze, relative to the root (empty: the whole root)
inclusions = []                  # only analyze files matching these globs
exclusions = []                  # extra exclusion globs, added to the defaults below
replace_default_exclusions = false
tests = ["**/*.test.*", "**/*.spec.*", "**/__tests__/**", "**/*_test.go"]  # test-code globs (default shortened here)
nosonar = true                   # honor // NOSONAR comments
max_file_size = 1024000          # skip larger files (bytes)
respect_gitignore = true
detect_bundles = true            # skip concatenated JS bundles
baseline = ".uncomplicated/baseline.json"   # drop issues recorded with --write-baseline (default: unset)

[new_code]
reference = "origin/main"        # git ref; new code = lines changed since the merge-base (default: unset)

[coverage]
lcov = ["coverage/lcov.info"]    # LCOV
go = ["coverage.out"]            # go test -coverprofile
cobertura = []                   # Cobertura XML
generic = []                     # SonarQube generic coverage XML (sonar.coverageReportPaths)
clover = []                      # Clover XML
istanbul = []                    # Istanbul coverage-final.json
reports = []                     # any of the above, detected from the content
exclusions = []                  # files left out of coverage

[cpd]
enabled = true
min_tokens = 100
min_lines = 10
exclusions = []

[secrets]
enabled = true
other_languages = false          # also scan .py, .rb, .java, .rs... source files (slower on polyglot repos)

[deps]
enabled = false                  # needs `unc deps update` first
db = ".uncomplicated/osv.bin"

[rules]                          # JS/TS rules, or any repository with a prefix
S3776 = { threshold = 20 }       # parameters (implies on)
S1135 = "off"
UJ1053 = "on"                    # enable a rule that is off by default (unused exports)
"docker:DKR005" = "off"          # other repositories: repo:key
"secrets:SEC014" = "off"

[ts_rules]                       # overrides for TypeScript files only
S4325 = "off"

[go_rules]                       # Go rules (same as "go:KEY" in [rules])
S1192 = "off"

[[ignore]]                       # drop issues by rule and path (like sonar.issue.ignore.multicriteria)
rule = "typescript:S2245"        # key, or prefix:key; "*" for every rule
paths = ["src/legacy/**"]

[gate]
fallback_overall = true          # without a new-code reference, apply new_* conditions to overall code

[[gate.conditions]]              # replaces the default "Sonar way" gate when present
metric = "new_coverage"
op = "lt"                        # fail when value < threshold ("gt": value > threshold)
threshold = 80
```

Globs are relative to the project root, like SonarQube's: `generated/**` matches only at the root,
`**/generated/**` anywhere.

### Default exclusions

`**/node_modules/**`, `**/bower_components/**`, `**/dist/**`, `**/vendor/**`, `**/external/**`,
`**/contrib/**`, `**/*.d.ts`, `**/testdata/**`, `.git`, plus `.gitignore`d files. Hidden files and
directories are skipped for JS/TS and Go (`.github/workflows`, `.env` and similar are still read
for workflow and secret checks). Bundles, minified files and generated Go code are skipped by
content.

## Rule repositories

Rule keys are unique within a repository. `unc rules` lists them; `--lang <repo>` filters.

| Repository | Prefix | Keys | Files |
|---|---|---|---|
| JavaScript / TypeScript | `js` (or `javascript`, `typescript`) | SonarJS `S*`, extra `UJ*` | `.js .jsx .mjs .cjs .ts .tsx .mts .cts .vue .html` |
| Go | `go` | sonar-go `S*`, extra `UG*` | `.go` |
| Secrets | `secrets` | `SEC*` | every analyzed file, plus `.env*`, `.npmrc`, config, script and key files (other languages' source with `other_languages`) |
| Dockerfile | `docker` | `DKR*` | `Dockerfile`, `Dockerfile.*`, `*.dockerfile`, `Containerfile` |
| Kubernetes | `kubernetes` | `K8S*` | `.yml`/`.yaml` files containing Kubernetes objects |
| GitHub Actions | `actions` | `GHA*` | `.github/workflows/*.yml` |
| Terraform | `terraform` | `TF*` | `.tf`, `.tfvars` |
| CSS | `css` | SonarQube CSS `S*` | `.css .scss .less` |
| Dependencies | `deps` | `DEP*` | `package-lock.json`, `npm-shrinkwrap.json`, `yarn.lock`, `pnpm-lock.yaml`, `go.mod` |

On the command line: `--rule S1135=off`, `--rule go:S1192=off`, `--rule docker:DKR001=off`.

## Profiles

- `sonar-way` (default): the rules SonarQube enables by default, plus the extra rules marked as
  default in their catalogs.
- `all`: every implemented rule.
- `none`: only rules enabled explicitly in `[rules]`.
- `sonarqube-26.9`: exactly SonarQube 26.9's Sonar way JS/TS and Go rules, with that release's
  parameters. Other repositories are off, so results match the server.

## Suppressing issues

- `// NOSONAR` on a line (any comment syntax for the language: `# NOSONAR` in YAML, Dockerfiles
  and Terraform).
- JS/TS: `eslint-disable` / `eslint-disable-next-line` directives with the rule's ESLint name.
- `[[ignore]]` entries for rules in paths.
- A baseline for everything that exists today (below).

## Baselines

```bash
unc --write-baseline .uncomplicated/baseline.json   # record every current issue
unc --baseline .uncomplicated/baseline.json         # later runs: only issues not in the baseline
```

Issues are matched by file, rule, message and the content of their line, not the line number,
so edits elsewhere in the file don't bring them back. Baselined issues don't count toward
measures or the quality gate. Commit the file, or set `[analysis] baseline` to use it
automatically. Unlike `--new-code`, a baseline needs no git history.

## Coverage

Pass reports with the specific flags (`--lcov`, `--go-coverage`, `--cobertura`) or let `unc`
detect the format with `--coverage FILE` (repeatable, `*` globs allowed). Supported: LCOV, Go
cover profiles, Cobertura XML, SonarQube generic coverage XML, Clover XML and Istanbul
`coverage-final.json`. Paths in reports are matched to project files even when the report was
produced in another directory or container. Files missing from every report count as uncovered,
like in SonarQube.

## Vulnerable dependencies

```bash
unc deps update        # downloads the OSV npm (~220 MB) and Go (~13 MB) archives, builds .uncomplicated/osv.bin
unc --deps             # report vulnerable or malicious package versions pinned by lockfiles
```

`unc deps update --from npm.zip --from go.zip` builds the database from archives downloaded
elsewhere (air-gapped CI). `--ecosystems npm` limits the download. Analysis never uses the
network. Issues land on the lockfile line pinning the version, under DEP001 (critical) to DEP004
(low) by the most severe advisory, with the version that fixes them all.

In CI, cache `.uncomplicated/osv.bin` with a daily key and run `unc deps update` only on a miss.

## SonarQube projects

Without an `uncomplicated.toml`, `unc` reads `sonar-project.properties`:

| Property | Used as |
|---|---|
| `sonar.sources`, `sonar.tests` | directories analyzed; `sonar.tests` directories are test code |
| `sonar.inclusions`, `sonar.exclusions`, `sonar.{javascript,typescript,go}.exclusions`, `sonar.test.exclusions` | inclusions / exclusions |
| `sonar.test.inclusions` | test globs |
| `sonar.javascript.lcov.reportPaths`, `sonar.typescript.lcov.reportPaths` | LCOV reports |
| `sonar.go.coverage.reportPaths` | Go cover profiles |
| `sonar.coverageReportPaths` | generic coverage (format detected, so Cobertura works too) |
| `sonar.coverage.exclusions`, `sonar.cpd.exclusions` | coverage / duplication exclusions |
| `sonar.issue.ignore.multicriteria` | `[[ignore]]` entries |

## Quality gate

The default gate is SonarQube's "Sonar way", on new code:

| Metric | Fails when |
|---|---|
| `new_reliability_rating` | worse than A (any new bug) |
| `new_security_rating` | worse than A (any new vulnerability) |
| `new_maintainability_rating` | worse than A (debt ratio above 5%) |
| `new_coverage` | below 80% (needs a coverage report) |
| `new_duplicated_lines_density` | above 3% |

Coverage and duplication conditions are skipped when fewer than 20 lines changed. Without
`--new-code`, the conditions apply to the whole codebase (`fallback_overall`). Security hotspots
are reported but never fail the gate.

Any measure in the JSON report can be used in a condition: `coverage`, `bugs`,
`vulnerabilities`, `code_smells`, `security_hotspots`, `sqale_rating`, `duplicated_lines_density`,
`cognitive_complexity`, `new_violations`, `new_blocker_violations` and so on.

## Exit status

`0` gate passed, `1` gate failed (or an issue matched `--fail-on <severity>`), `2` usage or
runtime error. `--no-gate` never fails on the gate.
