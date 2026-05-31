.DEFAULT_GOAL := help

# ── Environment ───────────────────────────────────────────────────────────────

CEPH_LIB_DIR := $(shell pwd)/target/ceph-libs
CEPH_ENV := LIBRARY_PATH="$(CEPH_LIB_DIR)" RUSTFLAGS="-L $(CEPH_LIB_DIR)"

# ── Rust workspace ────────────────────────────────────────────────────────────

.PHONY: ceph-libs
ceph-libs:
	@mkdir -p $(CEPH_LIB_DIR)
	@ln -sf /usr/lib/x86_64-linux-gnu/librados.so.2 $(CEPH_LIB_DIR)/librados.so 2>/dev/null || true
	@ln -sf /usr/lib/x86_64-linux-gnu/librbd.so.1 $(CEPH_LIB_DIR)/librbd.so 2>/dev/null || true

.PHONY: build
build: ceph-libs ## Build all Rust crates (release)
	$(CEPH_ENV) cargo build --release

.PHONY: build-dev
build-dev: ceph-libs ## Build all Rust crates (debug)
	$(CEPH_ENV) cargo build

.PHONY: deb-agent
deb-agent: build-init ceph-libs ## Build Debian package for mikrom-agent
	@command -v cargo-deb >/dev/null 2>&1 || { echo >&2 "cargo-deb is not installed. Install it with: cargo install cargo-deb"; exit 1; }
	$(CEPH_ENV) cargo build --release -p mikrom-agent
	cd mikrom-agent && $(CEPH_ENV) cargo deb --no-build
	@echo "✅ Debian package built in: target/debian/"

.PHONY: deb-network
deb-network: ## Build Debian package for mikrom-network
	@command -v cargo-deb >/dev/null 2>&1 || { echo >&2 "cargo-deb is not installed. Install it with: cargo install cargo-deb"; exit 1; }
	cargo build --release -p mikrom-network
	cd mikrom-network && cargo deb --no-build
	@echo "✅ Debian package built in: target/debian/"

.PHONY: deb-router
deb-router: ## Build Debian package for mikrom-router
	@command -v cargo-deb >/dev/null 2>&1 || { echo >&2 "cargo-deb is not installed. Install it with: cargo install cargo-deb"; exit 1; }
	cargo build --release -p mikrom-router
	cd mikrom-router && cargo deb --no-build
	@echo "✅ Debian package built in: target/debian/"

.PHONY: deb-dns
deb-dns: ## Build Debian package for mikrom-dns
	@command -v cargo-deb >/dev/null 2>&1 || { echo >&2 "cargo-deb is not installed. Install it with: cargo install cargo-deb"; exit 1; }
	cargo build --release -p mikrom-dns
	cd mikrom-dns && cargo deb --no-build
	@echo "✅ Debian package built in: target/debian/"

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
	TEST_NATS_URL=nats://localhost:4223 cargo nextest run --test integration_auth_tests -p mikrom-api --features test-utils,api-e2e && \
	TEST_NATS_URL=nats://localhost:4223 cargo nextest run --test integration_app_lifecycle_tests -p mikrom-api --features test-utils,api-e2e

.PHONY: test-all-crates
test-all-crates: ceph-libs ## Run unit tests for all crates
	$(call check_nextest)
	$(CEPH_ENV) cargo nextest run -p mikrom-proto && \
	$(CEPH_ENV) cargo nextest run -p mikrom-scheduler && \
	$(CEPH_ENV) cargo nextest run -p mikrom-agent && \
	$(CEPH_ENV) cargo nextest run -p mikrom-builder && \
	$(CEPH_ENV) cargo nextest run -p mikrom-api --features test-utils && \
	$(CEPH_ENV) cargo nextest run -p mikrom-init && \
	$(CEPH_ENV) cargo nextest run -p mikrom-router

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
run-agent: ceph-libs ## Run mikrom-agent with watch (port 5003)
	cd mikrom-agent && LIBRARY_PATH="$(shell pwd)/target/ceph-libs" RUSTFLAGS="-L $(shell pwd)/target/ceph-libs" cargo watch -x run

.PHONY: run-builder
run-builder: ## Run mikrom-builder with watch
	cd mikrom-builder && cargo watch -x run

.PHONY: run-router
run-router: ## Run mikrom-router (Rust/Pingora)
	cd mikrom-router && cargo watch -x run

.PHONY: build-init
build-init: ## Build mikrom-init as a static binary (musl)
	rustup target add x86_64-unknown-linux-musl >/dev/null 2>&1 || true
	cargo build -p mikrom-init --release --target x86_64-unknown-linux-musl
	@mkdir -p target/release && cp target/x86_64-unknown-linux-musl/release/mikrom-init target/release/mikrom-init

.PHONY: run-app
run-app: ## Run mikrom-app dev server  (port 3001)
	cd mikrom-app && pnpm run dev --host

.PHONY: dev
dev: ## Launch all services in tmux windows
	@tmux new-session -d -s mikrom -n api 'make run-api 2>&1 | tee /tmp/mikrom-api.log'
	@tmux new-window -t mikrom -n scheduler 'make run-scheduler 2>&1 | tee /tmp/mikrom-scheduler.log'
	@tmux new-window -t mikrom -n builder 'make run-builder 2>&1 | tee /tmp/mikrom-builder.log'
	@tmux new-window -t mikrom -n app 'make run-app'
	@tmux select-window -t mikrom:api
	@tmux attach-session -t mikrom

.PHONY: run-cli
run-cli: ## Run mikrom-cli  →  make run-cli ARGS="health"
	cargo run -p mikrom-cli -- $(ARGS)

.PHONY: install-cli
install-cli: ## Install the mikrom binary to ~/.cargo/bin
	cargo install --path mikrom-cli

# ── Frontends ────────────────────────────────────────────────────────────────

.PHONY: app-install
app-install: ## Install mikrom-app dependencies
	cd mikrom-app && pnpm install

.PHONY: app-build
app-build: ## Build mikrom-app for production
	cd mikrom-app && pnpm build

.PHONY: app-lint
app-lint: ## Lint mikrom-app
	cd mikrom-app && pnpm lint

.PHONY: landing-dev
landing-dev: ## Run mikrom-landing dev server
	cd mikrom-landing && pnpm run dev

.PHONY: landing-build
landing-build: ## Build mikrom-landing
	cd mikrom-landing && ./node_modules/.bin/astro build

.PHONY: landing-preview
landing-preview: ## Preview mikrom-landing build
	cd mikrom-landing && pnpm run preview

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
