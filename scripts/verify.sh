#!/usr/bin/env bash
# Reproduce the reported results on this machine: toolchain, lint, tests, and a
# real ambard start/stop in a throwaway data directory. Safe to re-run; does
# not touch your real Ambar data or install any service.
set -euo pipefail
cd "$(dirname "$0")/.."

step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }

step "toolchain";      rustc --version; cargo --version; uname -sm
step "format";         cargo fmt --all --check
step "lint";           cargo clippy --workspace --all-targets --locked -- -D warnings
step "tests";          cargo test --workspace --locked 2>&1 | tee /tmp/ambar-test.log | grep -E "^test result|FAILED|panicked" || true
if grep -qE "FAILED|panicked" /tmp/ambar-test.log; then echo "TESTS FAILED — full log: /tmp/ambar-test.log"; exit 1; fi
total=$(grep "^test result" /tmp/ambar-test.log | awk '{p+=$4} END {print p}')
echo "all tests passed: $total"

step "python: SDK, Automation Center, Plasma theme, OS recipe"
if python3 -c "import PySide6, pytest" 2>/dev/null; then
  QT_QPA_PLATFORM=offscreen python3 -m pytest -q sdk/python/tests desktop os/tests tests
else
  echo "skipped (pip install PySide6-Essentials pytest to run them)"
fi

step "service smoke test (temporary data dir)"
cargo build --workspace --locked -q
T=$(mktemp -d /tmp/ambar.XXXXXX); export AMBAR_DATA_DIR="$T/d" AMBARD_NOTIFY=log
trap 'target/debug/ambar service stop >/dev/null 2>&1 || true; rm -rf "$T"' EXIT
mkdir -p "$T/out"
cat > "$T/hello.toml" <<TOML
id = "hello"
name = "Hello"
[trigger]
type = "manual"
[[actions]]
type = "write_file"
path = "$T/out/hello.txt"
content = "hello from ambard"
[permissions]
write = ["$T/out"]
TOML
target/debug/ambar service start
target/debug/ambar workflow add "$T/hello.toml" --yes --enable
target/debug/ambar workflow run hello
target/debug/ambar service status
target/debug/ambar service stop
grep -q "hello from ambard" "$T/out/hello.txt" && echo "smoke test passed"

step "done — everything above passed on $(uname -s)"
