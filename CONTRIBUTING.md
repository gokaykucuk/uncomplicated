# Contributing

## Build and test

```bash
cargo build --release            # target/release/unc
cargo build --profile fast       # quicker optimized build for local benchmarking
cargo test --workspace
```

The toolchain is pinned in `rust-toolchain.toml`.

## Ground rules

- **Precision first.** A rule that cries wolf gets disabled. When the analysis can't be sure (no
  type information, dynamic code), stay silent.
- **Speed is a feature.** Measure before and after with a cold cache:
  `./target/fast/unc <repo> --no-cache --no-gate -f summary`. `UNC_PROFILE=1` prints time per rule.
  Compile regexes once, prefilter with `memchr`/Aho-Corasick, avoid per-node allocations.
- **SonarQube compatibility.** Rules with Sonar keys follow Sonar's defaults, messages and
  locations. `tools/parity.py` compares JS/TS results with SonarJS's own expectations.
- **Formatting.** Format the files you change with
  `rustfmt --edition 2024 --config max_width=120,use_small_heuristics=Max <file>`. Don't run
  `cargo fmt` over the whole workspace.

## Where things are

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the pipeline and crates, and the rule
authoring guides for [JS/TS](docs/RULE_AUTHORING.md) and [Go](docs/GO_RULE_AUTHORING.md).

A rule change comes with tests: compliant and noncompliant cases next to the rule. Before
proposing a new rule, run it on a few real projects and look at every issue it raises.
