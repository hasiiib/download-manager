#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

echo '== Ubuntu prerequisite checks =='
bash scripts/doctor.sh

echo '== Generate Tauri icon =='
python3 scripts/generate_icon.py

echo '== TypeScript and Vite build =='
npm run build

echo '== Rust backend build check =='
cargo check --manifest-path src-tauri/Cargo.toml

echo '== Rust unit tests =='
cargo test --manifest-path src-tauri/Cargo.toml

echo '== Project configuration tests =='
python3 -m unittest discover -s tests -p 'test_project_config.py' -v

echo '== Python test file syntax =='
python3 -m compileall -q tests

echo '== aria2 authenticated HTTP transfer + checksum =='
python3 -m unittest discover -s tests -p 'test_aria2_rpc.py' -v

echo '== aria2 session persistence =='
python3 -m unittest discover -s tests -p 'test_aria2_session.py' -v

echo '== HTTP Range pause / resume =='
python3 -m unittest discover -s tests -p 'test_aria2_resume.py' -v

echo '== Partial download recovery after engine restart =='
python3 -m unittest discover -s tests -p 'test_aria2_restart_resume.py' -v

echo 'All local validation steps passed.'
