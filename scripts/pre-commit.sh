#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

if [ -x /tmp/opencode/protoc/bin/protoc ]; then
  export PROTOC=/tmp/opencode/protoc/bin/protoc
elif command -v protoc >/dev/null 2>&1; then
  export PROTOC="$(command -v protoc)"
fi

echo "Running pre-commit checks..."

run_rust=false
run_app=false
run_zig_init=false

while IFS= read -r -d '' file; do
  case "$file" in
  Cargo.toml | Cargo.lock | Makefile | *.rs | *.proto | mikrom-api/* | mikrom-agent/* | mikrom-builder/* | mikrom-cli/* | mikrom-dns/* | mikrom-network/* | mikrom-proto/* | mikrom-router/* | mikrom-scheduler/* | ci/*)
    run_rust=true
    ;;
  mikrom-init/*)
    run_zig_init=true
    ;;
  mikrom-app/*)
    run_app=true
    ;;
  esac
done < <(git diff --cached --name-only -z --diff-filter=ACMR)

if [ "$run_rust" = true ]; then
  echo "Running Dagger-backed Rust validation (make ci-fast)..."
  make ci-fast
fi

if [ "$run_app" = true ]; then
  echo "Running Dagger-backed frontend validation (make ci-app)..."
  make ci-app
fi

if [ "$run_zig_init" = true ]; then
  echo "Running Zig init validation (make test-init)..."
  make test-init
fi

echo "All checks passed!"
