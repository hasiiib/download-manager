#!/usr/bin/env bash
set -uo pipefail

failures=0
check() {
  local tool="$1"
  if command -v "$tool" >/dev/null 2>&1; then
    printf '[OK] %-13s %s\n' "$tool" "$(command -v "$tool")"
  else
    printf '[MISSING] %s\n' "$tool"
    failures=$((failures + 1))
  fi
}

printf 'Download Manager development environment check\n'
printf 'OS: '
if command -v lsb_release >/dev/null 2>&1; then
  lsb_release -ds
else
  uname -s
fi

for tool in node npm cargo rustc aria2c pkg-config; do
  check "$tool"
done

if command -v node >/dev/null 2>&1; then
  node -p '"Node.js version: " + process.versions.node'
  major="$(node -p 'parseInt(process.versions.node.split(".")[0], 10)')"
  if (( major < 22 )); then
    echo '[MISSING] Node.js 22 or newer is required'
    failures=$((failures + 1))
  fi
fi

if command -v pkg-config >/dev/null 2>&1; then
  for lib in webkit2gtk-4.1 gtk+-3.0 libsoup-3.0; do
    if pkg-config --exists "$lib"; then
      printf '[OK] Linux library: %s\n' "$lib"
    else
      printf '[MISSING] Linux library: %s\n' "$lib"
      failures=$((failures + 1))
    fi
  done
fi

if (( failures != 0 )); then
  printf 'Environment check failed: %s missing requirement(s).\n' "$failures"
  exit 1
fi

printf 'Environment prerequisites appear present. Run the application build to verify compilation.\n'
