#!/usr/bin/env python3
"""Compare `unc` results against SonarJS ruling expectations.

SonarJS's ruling test (its/ruling in the SonarJS repo) records, for ~60 real-world projects, the
lines on which each rule raises issues. This script analyzes the same projects with the same
settings and reports per-rule precision/recall at (file, line) granularity.

usage: parity.py --unc target/fast/unc --sonarjs <SonarJS checkout> --sources <jsts-test-sources>
                 [--rules S1067,S3776] [--projects jquery,Ghost] [--out report.json]
"""

import argparse
import collections
import glob
import json
import os
import subprocess
import sys
import tempfile
import time

# (project, exclusions, test dir) — mirrors its/ruling RulingTest.java.
PROJECTS = [
    ("amplify", "external/**", "test"), ("angular.js", "src/ngLocale/**, i18n/**", "test"),
    ("backbone", "", "test"), ("es5-shim", "", "tests"), ("fireact", "", ""), ("ace", "", ""),
    ("ecmascript6-today", "", ""), ("expressionist.js", "", ""), ("Ghost", "", ""),
    ("hs-template", "", ""), ("http", "", ""), ("reddit-mobile", "", ""), ("redux", "", ""),
    ("router", "", ""), ("snoode", "", ""), ("sonar-web", "", ""), ("templating", "", ""),
    ("watchtower.js", "", ""), ("jira-clone", "", ""), ("TypeScript", "", "src/harness/unittests"),
    ("jquery", "", "test"), ("jshint", "", "tests"), ("jStorage", "", "tests"),
    ("knockout", "", "spec"), ("mootools-core", "", "Specs"), ("ocanvas", "build/**", ""),
    ("p5.js", "", "test"), ("paper.js", "gulp/jsdoc/**, packages/**", "test"),
    ("prototype", "", "test"), ("qunit", "", "test"), ("react-cloud-music", "", ""),
    ("sizzle", "external/**", "test"), ("underscore", "", "test"), ("ag-grid", "", "spec"),
    ("ant-design", "", "tests"), ("console", "", ""), ("courselit", ".yarn/**", ""),
    ("desktop", "", "app/test"), ("eigen", "", ""), ("fireface", "", ""), ("ionic2-auth", "", ""),
    ("Joust", "", ""), ("moose", "", ""), ("postgraphql", "", ""), ("prettier-vscode", "", ""),
    ("rxjs", "", "spec"), ("searchkit", "", ""), ("vuetify", "", ""), ("it-tools", "", ""),
    ("custom-jsts", "", "tests"), ("fresh", "", ""), ("oak", "", ""), ("vitest", "", "test"),
]


# Rule parameters of the ruling's quality profiles (RulingTest.setUp). The JS profile applies to
# JS files, the TS profile to TS files; rules whose parameters differ get a second, TS-only run.
RULE_PARAMS_JS = {
    "S1192": {"threshold": 4},
    "S1451": {
        "headerFormat": "// Copyright 20\\d\\d The Closure Library Authors. All Rights Reserved.",
        "isRegularExpression": True,
    },
}
RULE_PARAMS_TS = {"S1451": {"headerFormat": "//.*", "isRegularExpression": True}}
TS_GLOBS = ["**/*.ts", "**/*.tsx", "**/*.mts", "**/*.cts"]


def toml_value(v):
    if isinstance(v, bool):
        return "true" if v else "false"
    return json.dumps(v)


def toml_table(params):
    return "{ " + ", ".join(f"{k} = {toml_value(v)}" for k, v in params.items()) + " }"


def toml_list(items):
    return "[" + ", ".join(json.dumps(i) for i in items) + "]"


def run_project(unc, src_dir, exclusions, test_dir, rules, params=None, inclusions=None):
    excl = ["**/.*", "**/.*/**", "**/*.d.ts", "**/.git/**", "**/node_modules/**", "**/bower_components/**",
            "**/dist/**", "**/vendor/**", "**/external/**", "**/contrib/**"]
    excl += [e.strip() for e in exclusions.split(",") if e.strip()]
    # With no test dir, Sonar classifies tests by file-name heuristic (our defaults).
    tests_line = f"tests = {toml_list([f'{test_dir}/**'])}\n" if test_dir else ""
    incl_line = f"inclusions = {toml_list(inclusions)}\n" if inclusions else ""
    params = RULE_PARAMS_JS if params is None else params
    cfg = f"""[analysis]
profile = "all"
replace_default_exclusions = true
exclusions = {toml_list(excl)}
{tests_line}{incl_line}max_file_size = {4000 * 1000}
respect_gitignore = false
nosonar = true

[cpd]
enabled = false
"""
    if rules:
        # Only the requested rules: disable everything else via profile + explicit enable.
        cfg = cfg.replace('profile = "all"', 'profile = "none"')
        cfg += "\n[rules]\n" + "".join(
            f"{r} = {toml_table(params[r])}\n" if r in params else f'{r} = "on"\n' for r in rules
        )
    else:
        cfg += "\n[rules]\n" + "".join(f"{r} = {toml_table(p)}\n" for r, p in params.items())
    with tempfile.NamedTemporaryFile("w", suffix=".toml", delete=False) as f:
        f.write(cfg)
        cfg_path = f.name
    t0 = time.time()
    out = subprocess.run(
        [unc, src_dir, "--config", cfg_path, "-f", "json", "--no-cache", "--no-gate"],
        capture_output=True, text=True,
    )
    elapsed = time.time() - t0
    os.unlink(cfg_path)
    if out.returncode not in (0, 1):
        print(f"unc failed on {src_dir}: {out.stderr[:2000]}", file=sys.stderr)
        return {}, elapsed, 0
    data = json.loads(out.stdout)
    issues = collections.defaultdict(lambda: collections.Counter())
    for i in data["issues"]:
        issues[i["rule"]][(i["file"], i["line"])] += 1
    return issues, elapsed, data["measures"].get("ncloc", 0)


def load_expected(expected_dir, project):
    exp = collections.defaultdict(lambda: collections.Counter())
    for path in glob.glob(os.path.join(expected_dir, project, "*.json")):
        name = os.path.basename(path)[:-5]  # javascript-S1067
        if "-" not in name:
            continue
        lang, rule = name.split("-", 1)
        if lang not in ("javascript", "typescript"):
            continue
        with open(path) as f:
            data = json.load(f)
        for key, lines in data.items():
            file = key.split(":", 1)[1]
            for line in lines:
                exp[rule][(file, line)] += 1
    return exp


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--unc", required=True)
    ap.add_argument("--sonarjs", required=True)
    ap.add_argument("--sources", required=True)
    ap.add_argument("--rules", default="")
    ap.add_argument("--projects", default="")
    ap.add_argument("--out", default="")
    ap.add_argument("--show", type=int, default=0, help="print N example FP/FN per rule")
    args = ap.parse_args()
    rules = [r for r in args.rules.split(",") if r]
    only = set(p for p in args.projects.split(",") if p)
    expected_dir = os.path.join(args.sonarjs, "its/ruling/src/test/resources/expected")
    listing = subprocess.run([args.unc, "rules", "--json", "--lang", "js"], capture_output=True, text=True).stdout
    implemented = {r["key"] for r in json.loads(listing)}
    stats = collections.defaultdict(lambda: {"tp": 0, "fp": 0, "fn": 0, "fp_ex": [], "fn_ex": []})
    total_time = 0.0
    total_loc = 0
    for project, exclusions, test_dir in PROJECTS:
        if only and project not in only:
            continue
        src = (os.path.join(args.sonarjs, "its/sources/custom/jsts") if project == "custom-jsts"
               else os.path.join(args.sources, project))
        if not os.path.isdir(src):
            continue
        actual, elapsed, loc = run_project(args.unc, src, exclusions, test_dir, rules)
        ts_rules = [r for r in set(RULE_PARAMS_JS) | set(RULE_PARAMS_TS) if not rules or r in rules]
        if ts_rules:
            ts_only, el2, _ = run_project(args.unc, src, exclusions, test_dir, ts_rules, RULE_PARAMS_TS, TS_GLOBS)
            total_time += el2
            for r in ts_rules:
                merged = collections.Counter(
                    {k: v for k, v in actual.get(r, {}).items() if not k[0].endswith((".ts", ".tsx", ".mts", ".cts"))}
                )
                merged.update(ts_only.get(r, collections.Counter()))
                actual[r] = merged
        total_time += elapsed
        total_loc += loc
        expected = load_expected(expected_dir, project)
        for rule in set(actual) | set(expected):
            if rules and rule not in rules:
                continue
            if rule not in implemented:
                continue
            a, e = actual.get(rule, collections.Counter()), expected.get(rule, collections.Counter())
            s = stats[rule]
            for k in set(a) | set(e):
                tp = min(a[k], e[k])
                s["tp"] += tp
                if a[k] > tp:
                    s["fp"] += a[k] - tp
                    if len(s["fp_ex"]) < 20:
                        s["fp_ex"].append(f"{project}:{k[0]}:{k[1]}")
                if e[k] > tp:
                    s["fn"] += e[k] - tp
                    if len(s["fn_ex"]) < 20:
                        s["fn_ex"].append(f"{project}:{k[0]}:{k[1]}")
    rows = []
    for rule, s in stats.items():
        p = s["tp"] / (s["tp"] + s["fp"]) if s["tp"] + s["fp"] else 1.0
        r = s["tp"] / (s["tp"] + s["fn"]) if s["tp"] + s["fn"] else 1.0
        rows.append((rule, s["tp"], s["fp"], s["fn"], p, r))
    rows.sort(key=lambda x: (x[2] + x[3]), reverse=True)
    tp = sum(r[1] for r in rows)
    fp = sum(r[2] for r in rows)
    fn = sum(r[3] for r in rows)
    print(f"{'rule':<8} {'tp':>7} {'fp':>7} {'fn':>7} {'prec':>6} {'recall':>6}")
    for rule, t, f_, n, p, r in rows:
        print(f"{rule:<8} {t:>7} {f_:>7} {n:>7} {p:>6.1%} {r:>6.1%}")
        if args.show:
            for ex in stats[rule]["fp_ex"][: args.show]:
                print(f"    FP {ex}")
            for ex in stats[rule]["fn_ex"][: args.show]:
                print(f"    FN {ex}")
    perfect = sum(1 for r in rows if r[2] == 0 and r[3] == 0)
    print(f"\nTOTAL    tp={tp} fp={fp} fn={fn} precision={tp / max(tp + fp, 1):.1%} recall={tp / max(tp + fn, 1):.1%}")
    print(f"rules compared: {len(rows)}, exact match: {perfect}; analysis time {total_time:.1f}s for {total_loc} LOC")
    if args.out:
        with open(args.out, "w") as f:
            json.dump({k: {kk: vv for kk, vv in v.items()} for k, v in stats.items()}, f, indent=1)


if __name__ == "__main__":
    main()
