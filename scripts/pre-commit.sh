#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

echo "Running pre-commit checks..."

staged_files=$(git diff --cached --name-only --diff-filter=ACMR)

run_rust=false
run_app=false
run_zig_init=false

for file in $staged_files; do
  case "$file" in
  Cargo.toml | Cargo.lock | Makefile | *.rs | *.proto | mikrom-api/* | mikrom-agent/* | mikrom-builder/* | mikrom-cli/* | mikrom-dns/* | mikrom-init/* | mikrom-network/* | mikrom-proto/* | mikrom-router/* | mikrom-scheduler/* | ci/*)
    run_rust=true
    ;;
  mikrom-init-zig/*)
    run_zig_init=true
    ;;
  mikrom-app/*)
    run_app=true
    ;;
  esac
done

if [ "$run_rust" = true ]; then
  echo "Running Dagger-backed Rust validation (make ci-fast)..."
  make ci-fast
fi

if [ "$run_app" = true ]; then
  echo "Running Dagger-backed frontend validation (make ci-app)..."
  make ci-app
fi

if [ "$run_zig_init" = true ]; then
  echo "Running Zig init validation (make test-init-zig)..."
  make test-init-zig
fi

echo "All checks passed!"
