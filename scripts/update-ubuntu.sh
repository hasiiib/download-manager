#!/usr/bin/env bash
# One-command developer update: pull, verify, build, install and launch.
# Run as a normal user: bash ~/download-manager/scripts/update-ubuntu.sh
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "[ERROR] This helper is intended for Linux/Ubuntu only." >&2
  exit 1
fi

if pgrep -x download-manager >/dev/null 2>&1; then
  echo "[ERROR] Close Download Manager before rebuilding and reinstalling it." >&2
  echo "After closing the window, run this same command again." >&2
  exit 1
fi

log_dir="$(mktemp -d "${TMPDIR:-/tmp}/download-manager-update.XXXXXX")"
printf 'Build logs: %s\n' "$log_dir"

step() {
  local label="$1"
  shift
  local log="$log_dir/$label.log"
  printf '\n== %s ==\n' "$label"
  if "$@" > "$log" 2>&1; then
    printf '[PASS] %s\n' "$label"
  else
    local status=$?
    printf '[FAIL] %s (exit %s)\n' "$label" "$status" >&2
    tail -n 40 "$log" >&2
    printf '\nFull log: %s\n' "$log" >&2
    return "$status"
  fi
}

# Use a fast-forward-only pull to avoid silently overwriting local work.
step git-pull git pull --ff-only origin main
step ubuntu-qa bash scripts/ubuntu-qa.sh
step deb-build npm run tauri -- build --bundles deb

shopt -s nullglob
packages=(src-tauri/target/release/bundle/deb/*.deb)
if [[ "${#packages[@]}" -ne 1 ]]; then
  echo "[FAIL] Expected exactly one .deb, got ${#packages[@]}" >&2
  exit 1
fi
package="$(realpath "${packages[0]}")"
step deb-verify python3 scripts/check_deb.py "$package"

printf '\n== install (may ask for your sudo password) ==\n'
sudo apt install --reinstall "$package"

printf '\n[PASS] Code updated, QA passed, package built and installed.\n'
printf 'Opening Download Manager. Close its window to return to the terminal.\n'
exec download-manager
