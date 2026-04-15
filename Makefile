.DEFAULT_GOAL := help

# ── Rust workspace ────────────────────────────────────────────────────────────

.PHONY: build
build: ## Build all Rust crates (release)
	cargo build --release

.PHONY: build-dev
build-dev: ## Build all Rust crates (debug)
	cargo build

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

.PHONY: test-integration
test-integration: ## Run integration tests (starts PostgreSQL via Docker)
	cd mikrom-api && docker compose up -d postgres && \
	  sleep 2 && \
	  cargo test --test integration; \
	  docker compose stop postgres

.PHONY: test-all
test-all: test test-integration ## Run unit + integration tests

# ── Run services ──────────────────────────────────────────────────────────────

.PHONY: run-api
run-api: ## Run mikrom-api  (port 5001)
	cargo run -p mikrom-api

.PHONY: run-scheduler
run-scheduler: ## Run mikrom-scheduler  (port 5002)
	cargo run -p mikrom-scheduler

.PHONY: run-agent
run-agent: ## Run mikrom-agent  (port 5003)
	cargo run -p mikrom-agent

.PHONY: run-app
run-app: ## Run mikrom-app dev server  (port 3000)
	cd mikrom-app && pnpm dev

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

.PHONY: db-start
db-start: ## Start only PostgreSQL (for local development)
	cd mikrom-api && docker compose up -d postgres

.PHONY: db-stop
db-stop: ## Stop PostgreSQL
	cd mikrom-api && docker compose stop postgres

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
