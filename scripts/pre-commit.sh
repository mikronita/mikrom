#!/bin/bash
set -e

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

echo "Running pre-commit checks..."

staged_files=$(git diff --cached --name-only --diff-filter=ACMR)

run_rust=false
run_app=false

for file in $staged_files; do
  case "$file" in
    Cargo.toml|Cargo.lock|Makefile|*.rs|*.proto|mikrom-api/*|mikrom-agent/*|mikrom-builder/*|mikrom-cli/*|mikrom-dns/*|mikrom-init/*|mikrom-network/*|mikrom-proto/*|mikrom-router/*|mikrom-scheduler/*)
      run_rust=true
      ;;
    mikrom-app/*)
      run_app=true
      ;;
  esac
done

if [ "$run_rust" = true ]; then
  echo "Checking Rust formatting..."
  cargo fmt --all -- --check

  echo "Running Clippy..."
  cargo clippy --all-targets --all-features -- -D warnings

  echo "Running Rust tests with nextest..."
  if ! command -v cargo-nextest &>/dev/null; then
    echo "cargo-nextest not found. Please install it with 'cargo binstall cargo-nextest' or 'cargo install cargo-nextest'."
    exit 1
  fi
  export TEST_DATABASE_URL="postgres://mikrom:mikrom_password@localhost:5432/mikrom_api_test"
  export TEST_NATS_URL="nats://localhost:4223"
  make test-all
fi

if [ "$run_app" = true ]; then
  echo "Linting mikrom-app..."
  make app-lint
  echo "Checking mikrom-app..."
  make app-check
  echo "Unit Tests mikrom-app..."
  make app-test-unit
  echo "Playwright e2e Tests on mikrom-app..."
  make app-test-e2e
fi

echo "All checks passed!"
