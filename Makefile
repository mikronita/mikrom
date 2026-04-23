.DEFAULT_GOAL := help

# ── Rust workspace ────────────────────────────────────────────────────────────

.PHONY: build
build: ## Build all Rust crates (release)
	cargo build --release

.PHONY: build-dev
build-dev: ## Build all Rust crates (debug)
	cargo build

.PHONY: deb-agent
deb-agent: ## Build Debian package for mikrom-agent
	@command -v cargo-deb >/dev/null 2>&1 || { echo >&2 "cargo-deb is not installed. Install it with: cargo install cargo-deb"; exit 1; }
	cd mikrom-agent && cargo deb

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

.PHONY: test
test: ## Run all unit tests (no DB required)
	cargo test --lib

.PHONY: test-verbose
test-verbose: ## Run unit tests with output
	cargo test --lib -- --nocapture

.PHONY: test-one
test-one: ## Run a single test by name  →  make test-one NAME=test_score_idle
	cargo test --lib $(NAME)

.PHONY: test-cli
test-cli: ## Run mikrom-cli unit tests
	cargo test --lib -p mikrom-cli

.PHONY: test-integration
test-integration: ## Run integration tests (starts PostgreSQL via Docker)
	docker compose up -d postgres && \
	  sleep 5 && \
	  cargo test --test integration; \
	  docker compose stop postgres

.PHONY: test-e2e
test-e2e: ## Run end-to-end deployment tests
	cargo test -p mikrom-api --test deploy_e2e

.PHONY: test-all-crates
test-all-crates: ## Run unit tests for all crates plus e2e
	cargo test -p mikrom-proto && \
	cargo test -p mikrom-scheduler && \
	cargo test -p mikrom-agent && \
	cargo test -p mikrom-api && \
	make test-e2e

.PHONY: test-all
test-all: test-all-crates test-integration ## Run unit + integration + e2e tests

.PHONY: test-coverage
test-coverage: ## Run tests and generate coverage report (requires cargo-llvm-cov)
	@command -v cargo-llvm-cov >/dev/null 2>&1 || { echo >&2 "cargo-llvm-cov is not installed. Install it with: cargo install cargo-llvm-cov"; exit 1; }
	cargo llvm-cov --workspace --all-features --html

# ── Run services ──────────────────────────────────────────────────────────────

.PHONY: run-api
run-api: ## Run mikrom-api with watch (port 5001)
	cd mikrom-api && cargo watch -x run

.PHONY: run-scheduler
run-scheduler: ## Run mikrom-scheduler with watch (port 5002)
	cd mikrom-scheduler && cargo watch -x run

.PHONY: run-agent
run-agent: ## Run mikrom-agent with watch (port 5003)
	cd mikrom-agent && cargo watch -x run

.PHONY: run-builder
run-builder: ## Run mikrom-builder with watch (port 5004)
	cd mikrom-builder && cargo watch -x run

.PHONY: run-router
run-router: ## Run mikrom-router (configurable via .env)
	cd mikrom-router && cargo watch -x run

.PHONY: run-app
run-app: ## Run mikrom-app dev server  (port 3000)
	cd mikrom-app && pnpm dev

.PHONY: dev
dev: ## Launch all services in tmux windows
	@tmux new-session -d -s mikrom -n api 'make run-api'
	@tmux new-window -t mikrom -n scheduler 'make run-scheduler'
	@tmux new-window -t mikrom -n builder 'make run-builder'
	@tmux new-window -t mikrom -n router 'make run-router'
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

.PHONY: db-start
db-start: ## Start only PostgreSQL (for local development)
	docker compose up -d postgres
	@echo "Waiting for PostgreSQL to be ready..."
	@sleep 5

.PHONY: db-stop
db-stop: ## Stop PostgreSQL
	docker compose stop postgres

# ── Housekeeping ──────────────────────────────────────────────────────────────

.PHONY: clean
clean: ## Remove Rust build artefacts
	cargo clean

.PHONY: check
check: fmt-check clippy test ## fmt-check + clippy + unit tests

.PHONY: help
help: ## Show this help
	@awk 'BEGIN {FS = ":.*##"; printf "\nUsage:\n  make \033[36m<target>\033[0m\n\nTargets:\n"} \
	  /^[a-zA-Z_-]+:.*?##/ { printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)
