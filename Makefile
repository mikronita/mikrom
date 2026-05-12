.DEFAULT_GOAL := help

# ── Rust workspace ────────────────────────────────────────────────────────────

.PHONY: build
build: ## Build all Rust crates (release)
	cargo build --release

.PHONY: build-dev
build-dev: ## Build all Rust crates (debug)
	cargo build

.PHONY: deb-agent
deb-agent: build-init ## Build Debian package for mikrom-agent
	@command -v cargo-deb >/dev/null 2>&1 || { echo >&2 "cargo-deb is not installed. Install it with: cargo install cargo-deb"; exit 1; }
	cargo build --release -p mikrom-agent
	cd mikrom-agent && cargo deb --no-build

.PHONY: deb-router
deb-router: ## Build Debian package for mikrom-router (Rust/Pingora)
	cd mikrom-router && ./package.sh

.PHONY: fmt
fmt: ## Format Rust code
	cargo fmt

.PHONY: fmt-check
fmt-check: ## Check Rust formatting without writing
	cargo fmt -- --check

.PHONY: clippy
clippy: ## Run Clippy linter
	cargo clippy -- -D warnings

# ── Tests ─────────────────────────────────────────────────────────────────────

define check_nextest
	@command -v cargo-nextest >/dev/null 2>&1 || { echo >&2 "cargo-nextest is not installed. Install it with: cargo binstall cargo-nextest or cargo install cargo-nextest"; exit 1; }
endef

.PHONY: test
test: ## Run all unit tests (no DB required)
	$(call check_nextest)
	cargo nextest run --lib

.PHONY: test-verbose
test-verbose: ## Run unit tests with output
	$(call check_nextest)
	cargo nextest run --lib -- --nocapture

.PHONY: test-one
test-one: ## Run a single test by name  →  make test-one NAME=test_score_idle
	$(call check_nextest)
	cargo nextest run --lib $(NAME)

.PHONY: test-cli
test-cli: ## Run mikrom-cli unit tests
	$(call check_nextest)
	cargo nextest run --lib -p mikrom-cli

.PHONY: test-integration
test-integration: ## Run integration tests (starts PostgreSQL via Docker)
	$(call check_nextest)
	docker compose up -d --wait postgres nats-test
	TEST_NATS_URL=nats://localhost:4223 cargo nextest run --test integration -p mikrom-api --features test-utils

.PHONY: test-all-crates
test-all-crates: ## Run unit tests for all crates
	$(call check_nextest)
	cargo nextest run -p mikrom-proto && \
	cargo nextest run -p mikrom-scheduler && \
	cargo nextest run -p mikrom-agent && \
	cargo nextest run -p mikrom-builder && \
	cargo nextest run -p mikrom-api --features test-utils && \
	cargo nextest run -p mikrom-init && \
	cargo nextest run -p mikrom-telemetry && \
	cargo nextest run -p mikrom-router

.PHONY: test-all
test-all: test-all-crates test-integration ## Run unit + integration tests

.PHONY: test-coverage
test-coverage: ## Run tests and generate coverage report (requires cargo-llvm-cov)
	@command -v cargo-llvm-cov >/dev/null 2>&1 || { echo >&2 "cargo-llvm-cov is not installed. Install it with: cargo install cargo-llvm-cov"; exit 1; }
	cargo llvm-cov --workspace --all-features --html

# ── Run services ──────────────────────────────────────────────────────────────

.PHONY: run-api
run-api: ## Run mikrom-api with watch (port 5001)
	cd mikrom-api && cargo watch -x run

.PHONY: run-scheduler
run-scheduler: ## Run mikrom-scheduler with watch
	cd mikrom-scheduler && cargo watch -x run

.PHONY: run-agent
run-agent: ## Run mikrom-agent with watch (port 5003)
	cd mikrom-agent && cargo watch -x run

.PHONY: run-builder
run-builder: ## Run mikrom-builder with watch
	cd mikrom-builder && cargo watch -x run

.PHONY: run-telemetry
run-telemetry: ## Run mikrom-telemetry with watch
	cd mikrom-telemetry && cargo watch -x run

.PHONY: run-router
run-router: ## Run mikrom-router (Rust/Pingora)
	cd mikrom-router && cargo watch -x run

.PHONY: build-init
build-init: ## Build mikrom-init as a static binary (musl)
	rustup target add x86_64-unknown-linux-musl >/dev/null 2>&1 || true
	cargo build -p mikrom-init --release --target x86_64-unknown-linux-musl
	@mkdir -p target/release && cp target/x86_64-unknown-linux-musl/release/mikrom-init target/release/mikrom-init

.PHONY: run-app
run-app: ## Run mikrom-app dev server  (port 3000)
	cd mikrom-app && pnpm dev

.PHONY: dev
dev: ## Launch all services in tmux windows
	@tmux new-session -d -s mikrom -n api 'make run-api 2>&1 | tee /tmp/mikrom-api.log'
	@tmux new-window -t mikrom -n scheduler 'make run-scheduler 2>&1 | tee /tmp/mikrom-scheduler.log'
	@tmux new-window -t mikrom -n builder 'make run-builder'
	@tmux new-window -t mikrom -n telemetry 'make run-telemetry'
	@tmux new-window -t mikrom -n app 'make run-app'
	@tmux select-window -t mikrom:api
	@tmux attach-session -t mikrom

.PHONY: run-cli
run-cli: ## Run mikrom-cli  →  make run-cli ARGS="health"
	cargo run -p mikrom-cli -- $(ARGS)

.PHONY: install-cli
install-cli: ## Install the mikrom binary to ~/.cargo/bin
	cargo install --path mikrom-cli

# ── Next.js ───────────────────────────────────────────────────────────────────

.PHONY: app-install
app-install: ## Install mikrom-app dependencies
	cd mikrom-app && pnpm install

.PHONY: app-build
app-build: ## Build mikrom-app for production
	cd mikrom-app && pnpm build

.PHONY: app-lint
app-lint: ## Lint mikrom-app
	cd mikrom-app && pnpm lint

# ── Docker ────────────────────────────────────────────────────────────────────

.PHONY: up
up: ## Start all services with Docker Compose
	docker compose up --build

.PHONY: up-detach
up-detach: ## Start all services in the background
	docker compose up --build -d

.PHONY: down
down: ## Stop and remove containers
	docker compose down

.PHONY: down-volumes
down-volumes: ## Stop containers and remove volumes (deletes DB data)
	docker compose down -v

.PHONY: logs
logs: ## Follow logs of all services
	docker compose logs -f

.PHONY: logs-api
logs-api: ## Follow mikrom-api logs
	docker compose logs -f mikrom-api

.PHONY: logs-scheduler
logs-scheduler: ## Follow mikrom-scheduler logs
	docker compose logs -f mikrom-scheduler

.PHONY: logs-agent
logs-agent: ## Follow mikrom-agent logs
	docker compose logs -f mikrom-agent

.PHONY: logs-telemetry
logs-telemetry: ## Follow mikrom-telemetry logs
	docker compose logs -f mikrom-telemetry

.PHONY: db-start
db-start: ## Start PostgreSQL instance (for local development)
	docker compose up -d --wait postgres

.PHONY: db-stop
db-stop: ## Stop PostgreSQL instance
	docker compose stop postgres

# ── Housekeeping ──────────────────────────────────────────────────────────────

.PHONY: clean
clean: ## Remove Rust build artefacts
	cargo clean

.PHONY: check
check: fmt-check clippy test ## Run all checks (Rust)

.PHONY: help
help: ## Show this help
	@awk 'BEGIN {FS = ":.*##"; printf "\nUsage:\n  make \033[36m<target>\033[0m\n\nTargets:\n"} \
	  /^[a-zA-Z_-]+:.*?##/ { printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)
