# uncomplicated (`unc`)

**Fast static analysis for CI.** One native binary checks JavaScript, TypeScript and Go code,
Dockerfiles, Kubernetes manifests, GitHub Actions workflows, Terraform, stylesheets, secrets and
dependencies, with SonarQube's rules, metrics and quality gate. There's no server, JVM or Node.js
to run, and a 1.6M-line monorepo takes about a second.

```console
$ unc --new-code origin/main --coverage coverage/lcov.info

src/api/users.ts
     42:18  BLOCKER  UJ1001 Change this code to not construct SQL queries directly from user-controlled data.
     57:3   MAJOR    S3776  Refactor this function to reduce its Cognitive Complexity from 22 to the 15 allowed.
.github/workflows/ci.yml
     18:14  CRITICAL actions:GHA001 Don't use untrusted input "github.event.pull_request.title" directly in a run script.

8,415 files (1,601,811 LOC) · 1,141 rules · 3 issues: 0 bugs, 1 vulnerability, 2 code smells
Reliability A · Security E · Maintainability A · Duplication 0.8% · Coverage 84.2%
Quality gate: FAILED — new_security_rating = E (must be ≤ A)
Analyzed 70 MB in 1,260 ms
```

## What it checks

| Area | Rules | Highlights |
|---|---|---|
| **JavaScript / TypeScript** | 554 SonarJS + 13 | Every active SonarJS rule but one, with the same keys, messages and locations. React, Vue, Angular, Node.js, tests and accessibility. Taint analysis for SQL, command and path injection, XSS, open redirects, SSRF, NoSQL and code injection. Module graph: unresolved imports, missing exports, import cycles, unused exports. |
| **Go** | 37 sonar-go + 65 | All sonar-go rules, plus checks modeled on go vet, staticcheck, errcheck, ineffassign and gosec. Package-level checks: unused declarations, ignored errors, locks copied by value. Taint analysis for HTTP handlers (net/http, gin, echo, chi, gorilla, fiber). Loop-variable capture before Go 1.22. |
| **Secrets** | 21 | Cloud and SaaS tokens (AWS, GitHub, GitLab, Slack, Stripe, Google, OpenAI, Anthropic, npm…), private keys, credentials in connection URLs, secrets in `.env` and config files. Every file is scanned, not only source code. |
| **Dockerfile** | 15 | Unpinned base images, running as root, `curl \| sh`, secrets in `ENV`/`ARG`, apt/apk hygiene, misplaced `CMD`/`ENTRYPOINT`. |
| **Kubernetes** | 11 | Privileged containers, host namespaces and paths, dangerous capabilities, missing memory limits, `:latest` images, literal secrets in `env`. |
| **GitHub Actions** | 8 | Script injection from untrusted event data, `pull_request_target` checkouts that run PR code, `write-all` permissions, echoed secrets, deprecated workflow commands, outdated core actions. |
| **Terraform** | 12 | Public buckets, open security groups, unencrypted storage, publicly accessible databases, hard-coded credentials, wildcard IAM policies, weak TLS. AWS, GCP, Azure. |
| **CSS, SCSS, Less** | 24 | SonarQube's CSS rules: unknown properties, units, selectors and at-rules, invalid colors and `calc()`, duplicate and overridden properties. |
| **Dependencies** | 4 | Vulnerable and malicious package versions in `package-lock.json`, `yarn.lock`, `pnpm-lock.yaml` and `go.mod`, matched offline against OSV. Opt-in. |

All rules are listed in [docs/RULES.md](docs/RULES.md); `unc rules` lists them too.

Besides issues, `unc` computes SonarQube's measures and runs its quality gate:

- **Size and complexity:** lines of code, comments, functions, cyclomatic and cognitive complexity.
- **Debt and ratings:** technical debt and A–E reliability, security and maintainability ratings.
- **Duplication:** token-based and cross-file.
- **Coverage:** LCOV, Go, Cobertura, SonarQube generic XML, Clover and Istanbul reports.
- **New code:** lines changed since the merge-base with a branch, so the gate can apply only to what a pull request changes.

## Install

Prebuilt static binaries for Linux (x86_64, arm64) and macOS (arm64) are on the
[releases page](https://github.com/gokaykucuk/uncomplicated/releases):

```bash
curl -fsSL https://github.com/gokaykucuk/uncomplicated/releases/latest/download/unc-aarch64-apple-darwin.tar.gz | tar -xz -C ~/.local/bin
```

The Docker image `ghcr.io/gokaykucuk/uncomplicated` (multi-arch, includes `git`) is published
with each release. To build from source: `cargo build --release`.

## Usage

```bash
unc                                    # analyze the current directory
unc --new-code origin/main             # quality gate on the lines changed since origin/main
unc --coverage coverage/lcov.info      # add coverage (format detected; repeatable)
unc -f sarif -o unc.sarif              # write a report in another format
unc --report sarif=unc.sarif --report gitlab=gl.json   # several reports in one run
unc --write-baseline .uncomplicated/baseline.json      # accept today's issues…
unc --baseline .uncomplicated/baseline.json            # …and only report new ones from now on
unc deps update && unc --deps          # check lockfiles for known vulnerabilities
unc rules                              # list rules (--missing: catalog rules not implemented)
unc init                               # write a starter uncomplicated.toml
```

**Output formats:** text (default), `summary`, `json`, `sarif` (SARIF 2.1.0), `gitlab` (Code
Quality), `github` (workflow annotations), `sonar` (SonarQube generic issue import).

**Exit status:** `0` gate passed, `1` gate failed or `--fail-on <severity>` matched, `2` error.

Results are cached in `.uncomplicated/`. With that directory cached between CI runs, unchanged
files aren't analyzed again.

## CI

### GitHub Actions

```yaml
jobs:
  quality:
    runs-on: ubuntu-latest
    permissions:
      contents: read
    steps:
      - uses: actions/checkout@v7
        with:
          fetch-depth: 0 # new-code detection needs the merge base
      - uses: gokaykucuk/uncomplicated@v0.1.1
        with:
          args: --format github --new-code origin/${{ github.base_ref || 'main' }} --coverage coverage/lcov.info
```

The action caches `.uncomplicated/` between runs. Issues show up as annotations on the pull
request. For GitHub code scanning, add `--report sarif=unc.sarif` and upload the file with
`github/codeql-action/upload-sarif`.

### GitLab CI

```yaml
code_quality:
  image:
    name: ghcr.io/gokaykucuk/uncomplicated:0.1.1
    entrypoint: [""]
  variables:
    GIT_DEPTH: 0
  script: unc --new-code origin/$CI_MERGE_REQUEST_TARGET_BRANCH_NAME --report gitlab=gl-code-quality.json
  cache:
    key: unc
    paths: [.uncomplicated/]
  artifacts:
    reports:
      codequality: gl-code-quality.json
```

## Configuration

Nothing is required. Projects already set up for SonarQube work as they are: without an
`uncomplicated.toml`, `unc` reads `sonar-project.properties` (sources, tests, exclusions, coverage
paths, issue exclusions). To get exactly what a given SonarQube server reports, pin its rules
with `--profile sonarqube-26.9`.

A typical `uncomplicated.toml`:

```toml
[analysis]
exclusions = ["generated/**"]

[new_code]
reference = "origin/main"

[coverage]
reports = ["coverage/lcov.info", "coverage.out"]

[rules]
S3776 = { threshold = 20 }   # parameters
S1135 = "off"                # JS/TS rules by key
"docker:DKR001" = "off"      # other repositories with a prefix
```

`// NOSONAR`, `eslint-disable` comments and `[[ignore]]` entries suppress issues. The full
reference is in [docs/CONFIGURATION.md](docs/CONFIGURATION.md).

## Performance

Measured with a cold cache and the default rules (`--no-cache`):

| Corpus | Files | Lines of code | Wall time | Peak memory |
|---|---|---|---|---|
| SonarJS ruling corpus (JS/TS, 53 projects) | 20,417 | 2.35M | 1.4–1.7 s | 0.9 GB |
| Kubernetes (Go) | 9,724 | 2.23M | 3.2 s | 0.55 GB |
| Prometheus (Go) | 998 | 345k | 0.5 s | 0.2 GB |

These numbers are from a 12-core laptop. The [`bench` workflow](.github/workflows/bench.yml)
measures every change against its base on GitHub-hosted runners (Kubernetes, Prometheus, Grafana,
Discourse, Excalidraw) and reports wall time, memory and issue-count changes per rule. With a
warm cache and nothing changed, the JS/TS corpus takes 0.4 s.

On a real 1.5M-line monorepo, the project's SonarQube scanner took 633 s and `unc` takes about a
second. With `--profile sonarqube-26.9` both report the same issues.

**Parity with SonarJS**, on its own ruling corpus, comparing issues per rule at file/line
granularity:

- 471 rules compared.
- **99.9% precision, 97.5% recall.**
- 352 rules match exactly.

Most misses are rules where Sonar uses the full TypeScript type checker. `unc` infers types
locally and stays silent when it can't tell.

### How it's fast

- **Native parsers:** [oxc](https://oxc.rs) for JS/TS, tree-sitter for Go, YAML and HCL, and
  hand-written parsers for Dockerfiles and CSS. No TypeScript compiler, no JVM.
- **One parse, one walk:** each file is parsed once into a per-thread arena. Rules subscribe to
  the node types they need, and one walk over the nodes dispatches to all of them.
- **Cheap prefilters:** secret detection is one Aho-Corasick pass, and only keyword hits are
  checked with regexes. Taint analysis skips files with no possible source.
- **Parallel and cached:** files are analyzed on all cores and cached by content hash. Cross-file
  checks run from cached per-file summaries, so unchanged files are never parsed again.

## Documentation

- [Configuration reference](docs/CONFIGURATION.md)
- [Rule list](docs/RULES.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Writing JS/TS rules](docs/RULE_AUTHORING.md) · [Writing Go rules](docs/GO_RULE_AUTHORING.md)
- [Contributing](CONTRIBUTING.md)

`tools/parity.py` runs `unc` over SonarJS's ruling corpus and compares issues with SonarJS's
recorded expectations:

```bash
python3 tools/parity.py --unc target/release/unc --sonarjs <SonarJS checkout> --sources <jsts-test-sources>
```

## License

MIT.
