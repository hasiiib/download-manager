#!/usr/bin/env bash
# Developer-facing diagnostic runner. Does not install packages or use sudo.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

report_dir="$(mktemp -d "${TMPDIR:-/tmp}/download-manager-qa.XXXXXX")" || exit 1
printf 'Ubuntu QA logs: %s\n' "$report_dir"

run_step() {
  local name="$1"
  shift
  local log="$report_dir/$name.log"
  printf '\n== %s ==\n' "$name"
  if "$@" >"$log" 2>&1; then
    printf '[PASS] %s\n' "$name"
  else
    local code=$?
    printf '[FAIL] %s (exit code %s)\n' "$name" "$code"
    if [[ "$name" == rust-* ]]; then
      printf 'Rust error summary:\n'
      grep -n -E '^error(\\[[A-Z][0-9]+\\])?:|^ *--> src/' "$log" | head -n 35 || true
    fi
    printf 'Last 25 lines of %s:\n' "$log"
    tail -n 25 "$log"
    return 1
  fi
}

# Stop at the first failure: later failures usually hide the root cause.
run_step updater-syntax bash -n scripts/update-ubuntu.sh || exit 1
run_step prerequisites bash scripts/doctor.sh || exit 1
run_step icon python3 scripts/generate_icon.py || exit 1
run_step frontend-dependencies npm install --no-audit --no-fund || exit 1
run_step frontend-build npm run build || exit 1
run_step rust-check cargo check --manifest-path src-tauri/Cargo.toml || exit 1
run_step rust-tests cargo test --manifest-path src-tauri/Cargo.toml || exit 1
run_step config-tests python3 -m unittest discover -s tests -p test_project_config.py -v || exit 1
run_step deb-checker-tests python3 -m unittest discover -s tests -p test_deb_checker.py -v || exit 1
printf '\n[PASS] Build and unit checks. Logs: %s\n' "$report_dir"
printf 'Next step for installer: npm run tauri -- build --bundles deb\n'
