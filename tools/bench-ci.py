#!/usr/bin/env python3
"""Compares two `unc` builds on benchmark corpora and prints a Markdown report.

Wall time is measured with hyperfine (cold cache, interleaved), peak memory with /usr/bin/time,
and issue counts per rule from each build's JSON report.

    python3 tools/bench-ci.py --base base/target/fast/unc --head target/fast/unc \\
        --corpora corpora --corpora-list tools/bench-corpora.txt --out bench
"""

import argparse
import collections
import json
import os
import re
import subprocess
import sys

ARGS = ["--no-cache", "--no-gate"]


def corpora(list_path, root):
    for line in open(list_path):
        parts = line.split()
        if parts and not parts[0].startswith("#") and os.path.isdir(os.path.join(root, parts[0])):
            yield parts[0], os.path.join(root, parts[0])


def hyperfine(base, head, path, out, runs):
    cmd = lambda b: " ".join([b, path, *ARGS, "-f", "summary"])
    subprocess.run(
        ["hyperfine", "-N", "--warmup", "1", "--runs", str(runs),
         "--export-json", out, "-n", "base", cmd(base), "-n", "head", cmd(head)],
        check=True, stdout=subprocess.DEVNULL)
    res = json.load(open(out))["results"]
    return {r["command"]: r for r in res}


def peak_rss_mb(binary, path):
    p = subprocess.run(["/usr/bin/time", "-v", binary, path, *ARGS, "-f", "summary"],
                       stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
    m = re.search(r"Maximum resident set size \(kbytes\): (\d+)", p.stderr)
    return int(m.group(1)) / 1024 if m else float("nan")


def report(binary, path, out):
    p = subprocess.run([binary, path, *ARGS, "-f", "json", "-o", out], stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL)
    if p.returncode not in (0, 1):
        raise RuntimeError(f"{binary} failed on {path}")
    data = json.load(open(out))
    counts = collections.Counter(f"{i.get('repo', 'go' if i['file'].endswith('.go') else 'js')}:{i['rule']}"
                                 for i in data["issues"])
    return data["measures"], counts


def fmt_time(r):
    return f"{r['mean']:.2f} ± {r['stddev']:.2f} s"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", required=True)
    ap.add_argument("--head", required=True)
    ap.add_argument("--corpora", required=True)
    ap.add_argument("--corpora-list", required=True)
    ap.add_argument("--base-label", default="base")
    ap.add_argument("--head-label", default="head")
    ap.add_argument("--runs", type=int, default=5)
    ap.add_argument("--out", default="bench")
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)

    rows, changes = [], []
    for name, path in corpora(a.corpora_list, a.corpora):
        print(f"benchmarking {name}", file=sys.stderr)
        t = hyperfine(a.base, a.head, path, os.path.join(a.out, f"{name}-hyperfine.json"), a.runs)
        rss = (peak_rss_mb(a.base, path), peak_rss_mb(a.head, path))
        bm, bc = report(a.base, path, os.path.join(a.out, f"{name}-base.json"))
        hm, hc = report(a.head, path, os.path.join(a.out, f"{name}-head.json"))
        ratio = t["head"]["mean"] / t["base"]["mean"]
        rows.append(
            f"| {name} | {int(hm.get('files', 0)):,} | {int(hm.get('ncloc', 0)):,} | {fmt_time(t['base'])} | "
            f"{fmt_time(t['head'])} | {ratio:.2f}× | {rss[0]:.0f} → {rss[1]:.0f} MB | "
            f"{sum(bc.values()):,} → {sum(hc.values()):,} |")
        for rule in sorted(set(bc) | set(hc)):
            if bc[rule] != hc[rule]:
                changes.append((name, rule, bc[rule], hc[rule]))

    print("## Benchmark\n")
    print(f"`{a.base_label}` vs `{a.head_label}`, cold cache (`--no-cache`), {a.runs} runs after a warmup, "
          f"{os.cpu_count()} CPUs.\n")
    print("| Corpus | Files | Lines of code | Base | Head | Head/base | Peak memory | Issues |")
    print("|---|---|---|---|---|---|---|---|")
    print("\n".join(rows))
    print("\n## Issue count changes by rule\n")
    if not changes:
        print("No changes.")
    else:
        print("| Corpus | Rule | Base | Head |\n|---|---|---|---|")
        for name, rule, b, h in changes:
            print(f"| {name} | `{rule}` | {b} | {h} |")


if __name__ == "__main__":
    main()
