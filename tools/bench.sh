#!/usr/bin/env bash
# Benchmark `unc` against SonarQube's scanner on the same project directories.
#
# Requires a running SonarQube (default http://localhost:9000) and an analysis token:
#   SONAR_TOKEN=squ_... tools/bench.sh <dir> [<dir>...]
#
# SonarQube is measured as the scanner's wall time (analysis + upload), excluding the server's
# background processing; `unc` is measured with a cold cache, all rules for the profile enabled.
set -euo pipefail

UNC=${UNC:-target/release/unc}
HOST=${SONAR_HOST:-http://host.docker.internal:9000}
IMAGE=${SONAR_SCANNER_IMAGE:-sonarsource/sonar-scanner-cli:12.2.0.4256_8.1.0}
: "${SONAR_TOKEN:?set SONAR_TOKEN to a SonarQube analysis token}"

now() { python3 -c 'import time; print(time.time())'; }

printf "%-28s %10s %12s %12s %8s\n" project LOC sonar_s unc_s speedup
for dir in "$@"; do
  dir=$(cd "$dir" && pwd)
  name=$(basename "$dir")
  loc=$("$UNC" "$dir" --no-cache --no-gate -f json 2>/dev/null | python3 -c 'import json,sys; print(int(json.load(sys.stdin)["measures"]["ncloc"]))')

  t0=$(now)
  docker run --rm -v "$dir:/usr/src" -e SONAR_HOST_URL="$HOST" -e SONAR_TOKEN="$SONAR_TOKEN" "$IMAGE" \
    -Dsonar.projectKey="unc-bench-$name" -Dsonar.sources=. -Dsonar.scm.disabled=true \
    -Dsonar.javascript.node.maxspace=8192 >"/tmp/unc-bench-$name.log" 2>&1 || echo "sonar failed for $name (see /tmp/unc-bench-$name.log)"
  t1=$(now)

  "$UNC" "$dir" --no-cache --no-gate -f summary >/dev/null 2>&1 || true
  t2=$(now)

  python3 - "$name" "$loc" "$t0" "$t1" "$t2" <<'EOF'
import sys
name, loc, t0, t1, t2 = sys.argv[1], int(sys.argv[2]), *map(float, sys.argv[3:])
s, u = t1 - t0, t2 - t1
print(f"{name:<28} {loc:>10} {s:>12.1f} {u:>12.2f} {s / max(u, 1e-3):>7.0f}x")
EOF
done
