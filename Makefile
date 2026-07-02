.DEFAULT_GOAL := help

# ── Environment ───────────────────────────────────────────────────────────────

CEPH_LIB_DIR := $(shell pwd)/target/ceph-libs
CEPH_ENV := LIBRARY_PATH="$(CEPH_LIB_DIR)" RUSTFLAGS="-L $(CEPH_LIB_DIR)"
PROTOC_BIN := $(shell if [ -x /tmp/opencode/protoc/bin/protoc ]; then printf %s /tmp/opencode/protoc/bin/protoc; else command -v protoc; fi)
PROTOC_ENV := PROTOC="$(PROTOC_BIN)"
CEPH_LIBS := /usr/lib/x86_64-linux-gnu/librados.so.2 /usr/lib/x86_64-linux-gnu/librbd.so.1

define in_dir
cd $(1) && $(2)
endef

define check_tool
	@command -v $(1) >/dev/null 2>&1 || { echo >&2 "$(2)"; exit 1; }
endef

# ── Rust workspace ────────────────────────────────────────────────────────────

.PHONY: ceph-libs
ceph-libs:
	@mkdir -p $(CEPH_LIB_DIR)
	@ln -sf /usr/lib/x86_64-linux-gnu/librados.so.2 $(CEPH_LIB_DIR)/librados.so 2>/dev/null || true
	@ln -sf /usr/lib/x86_64-linux-gnu/librbd.so.1 $(CEPH_LIB_DIR)/librbd.so 2>/dev/null || true

.PHONY: check-ceph-libs
check-ceph-libs:
	@missing=""; \
	for lib in $(CEPH_LIBS); do \
		if [ ! -e "$${lib}" ]; then \
			missing="$$missing $${lib}"; \
		fi; \
	done; \
	if [ -n "$$missing" ]; then \
		echo >&2 "Missing Ceph client libraries:$$missing"; \
		echo >&2 "Install the Ceph development packages (for example: librbd-dev and librados-dev) before running make deb-agent."; \
		exit 1; \
	fi

.PHONY: build
build: ceph-libs ## Build all Rust crates (release)
	$(PROTOC_ENV) $(CEPH_ENV) cargo build --release

.PHONY: build-dev
build-dev: ceph-libs ## Build all Rust crates (debug)
	$(PROTOC_ENV) $(CEPH_ENV) cargo build

.PHONY: deb-agent
deb-agent: build-init ceph-libs check-ceph-libs ## Build Debian package for mikrom-agent
	$(call check_tool,cargo-deb,cargo-deb is not installed. Install it with: cargo install cargo-deb)
	$(call check_tool,cmake,cmake is not installed. Install it before building mikrom-agent Debian packages)
	cmake -S tundra-nat64 -B target/tundra-build -DCMAKE_BUILD_TYPE=Release
	cmake --build target/tundra-build --parallel
	strip target/tundra-build/tundra-nat64
	@mkdir -p target/release && cp target/tundra-build/tundra-nat64 target/release/tundra-nat64
	RUSTC_WRAPPER= $(PROTOC_ENV) $(CEPH_ENV) cargo build --release -p mikrom-agent
	$(call in_dir,mikrom-agent,RUSTC_WRAPPER= $(PROTOC_ENV) $(CEPH_ENV) cargo deb --no-build)
	@echo "✅ Debian package built in: target/debian/"

.PHONY: deb-network
deb-network: ## Build Debian package for mikrom-network
	$(call check_tool,cargo-deb,cargo-deb is not installed. Install it with: cargo install cargo-deb)
	$(PROTOC_ENV) cargo build --release -p mikrom-network
	$(call in_dir,mikrom-network,$(PROTOC_ENV) cargo deb --no-build)
	@echo "✅ Debian package built in: target/debian/"

.PHONY: deb-router
deb-router: ## Build Debian package for mikrom-router
	$(call check_tool,cargo-deb,cargo-deb is not installed. Install it with: cargo install cargo-deb)
	$(PROTOC_ENV) cargo build --release -p mikrom-router
	$(call in_dir,mikrom-router,$(PROTOC_ENV) cargo deb --no-build)
	@echo "✅ Debian package built in: target/debian/"

.PHONY: deb-dns
deb-dns: ## Build Debian package for mikrom-dns
	$(call check_tool,cargo-deb,cargo-deb is not installed. Install it with: cargo install cargo-deb)
	$(PROTOC_ENV) cargo build --release -p mikrom-dns
	$(call in_dir,mikrom-dns,$(PROTOC_ENV) cargo deb --no-build)
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

.PHONY: ci
ci: ci-full ## Run the local Dagger-based CI pipeline

.PHONY: ci-smoke
ci-smoke: ## Run the fastest Dagger-based validation profile
	cargo run -p mikrom-ci -- smoke

.PHONY: ci-fast
ci-fast: ## Run the intermediate Dagger-based validation profile
	cargo run -p mikrom-ci -- fast

.PHONY: ci-external-tests
ci-external-tests: ## Run opt-in integration tests that need local fixtures
	cargo run -p mikrom-ci -- external-tests

.PHONY: ci-ceph-tests
ci-ceph-tests: ## Run the Ceph integration test on a host with Ceph available
	MIKROM_RUN_CEPH_TESTS=1 cargo test -p mikrom-agent --test ceph_integration_tests -- --ignored

.PHONY: ci-full
ci-full: ## Run the full Dagger-based validation profile
	cargo run -p mikrom-ci -- full

.PHONY: ci-release
ci-release: ## Run validation plus image publish through Dagger
	cargo run -p mikrom-ci -- publish-release

.PHONY: ci-app
ci-app: ## Run the frontend validation subset through Dagger
	cargo run -p mikrom-ci -- app

.PHONY: ci-app-e2e
ci-app-e2e: ## Run the frontend e2e suite through Dagger
	cargo run -p mikrom-ci -- app-e2e

.PHONY: ci-images
ci-images: ## Build service images through Dagger
	cargo run -p mikrom-ci -- images

.PHONY: ci-publish
ci-publish: ## Publish service images through Dagger
	cargo run -p mikrom-ci -- publish

# ── Tests: Unit ──────────────────────────────────────────────────────────────

define check_nextest
	@command -v cargo-nextest >/dev/null 2>&1 || { echo >&2 "cargo-nextest is not installed. Install it with: cargo binstall cargo-nextest or cargo install cargo-nextest"; exit 1; }
endef

.PHONY: test
test: ## Run all unit tests (no DB required)
	$(call check_tool,cargo-nextest,cargo-nextest is not installed. Install it with: cargo binstall cargo-nextest or cargo install cargo-nextest)
	cargo nextest run --lib

.PHONY: test-verbose
test-verbose: ## Run unit tests with output
	$(call check_tool,cargo-nextest,cargo-nextest is not installed. Install it with: cargo binstall cargo-nextest or cargo install cargo-nextest)
	cargo nextest run --lib -- --nocapture

.PHONY: test-one
test-one: ## Run a single test by name  →  make test-one NAME=test_score_idle
	$(call check_tool,cargo-nextest,cargo-nextest is not installed. Install it with: cargo binstall cargo-nextest or cargo install cargo-nextest)
	cargo nextest run --lib $(NAME)

.PHONY: test-cli
test-cli: ## Run mikrom-cli unit tests
	$(call check_tool,cargo-nextest,cargo-nextest is not installed. Install it with: cargo binstall cargo-nextest or cargo install cargo-nextest)
	cargo nextest run --lib -p mikrom-cli

# ── Tests: Integration ───────────────────────────────────────────────────────

.PHONY: test-integration
test-integration: ## Run integration tests (starts PostgreSQL and test NATS via Docker)
	$(call check_tool,cargo-nextest,cargo-nextest is not installed. Install it with: cargo binstall cargo-nextest or cargo install cargo-nextest)
	docker compose --profile test up -d --wait postgres nats-test
	TEST_NATS_URL=nats://localhost:4223 cargo nextest run --test integration_auth_tests -p mikrom-api --features test-utils,api-e2e && \
	TEST_NATS_URL=nats://localhost:4223 cargo nextest run --test integration_app_lifecycle_tests -p mikrom-api --features test-utils,api-e2e

# ── Tests: Workspace ─────────────────────────────────────────────────────────

.PHONY: test-all-crates
test-all-crates: ceph-libs ## Run unit tests for all crates
	$(call check_tool,cargo-nextest,cargo-nextest is not installed. Install it with: cargo binstall cargo-nextest or cargo install cargo-nextest)
	$(PROTOC_ENV) $(CEPH_ENV) cargo nextest run -p mikrom-proto && \
	$(PROTOC_ENV) $(CEPH_ENV) cargo nextest run -p mikrom-scheduler && \
	$(PROTOC_ENV) $(CEPH_ENV) cargo nextest run -p mikrom-agent && \
	$(PROTOC_ENV) $(CEPH_ENV) cargo nextest run -p mikrom-builder && \
	$(PROTOC_ENV) $(CEPH_ENV) cargo nextest run -p mikrom-api --features test-utils && \
	$(PROTOC_ENV) $(CEPH_ENV) cargo nextest run -p mikrom-router && \
	$(PROTOC_ENV) $(CEPH_ENV) cargo nextest run -p mikrom-cli && \
	$(PROTOC_ENV) $(CEPH_ENV) cargo nextest run -p mikrom-dns && \
	$(PROTOC_ENV) $(CEPH_ENV) cargo nextest run -p mikrom-network

.PHONY: test-all
test-all: test-all-crates test-integration ## Run unit + integration tests

# ── Tests: Coverage ──────────────────────────────────────────────────────────

.PHONY: test-coverage
test-coverage: ## Run tests and generate coverage report (requires cargo-llvm-cov)
	$(call check_tool,cargo-llvm-cov,cargo-llvm-cov is not installed. Install it with: cargo install cargo-llvm-cov)
	cargo llvm-cov --workspace --all-features --html

# ── Run services: Rust ────────────────────────────────────────────────────────

.PHONY: run-api
run-api: ## Run mikrom-api with watch (port 5001)
	$(call in_dir,mikrom-api,cargo watch -x run)

.PHONY: run-scheduler
run-scheduler: ## Run mikrom-scheduler with watch
	$(call in_dir,mikrom-scheduler,cargo watch -x run)

.PHONY: run-agent
run-agent: ceph-libs ## Run mikrom-agent with watch (port 5003)
	$(call in_dir,mikrom-agent,LIBRARY_PATH="$(shell pwd)/target/ceph-libs" RUSTFLAGS="-L $(shell pwd)/target/ceph-libs" cargo watch -x run)

.PHONY: run-builder
run-builder: ## Run mikrom-builder with watch
	$(call in_dir,mikrom-builder,cargo watch -x run)

.PHONY: run-router
run-router: ## Run mikrom-router (Rust/Pingora)
	$(call in_dir,mikrom-router,cargo watch -x run)

# ── Run services: Zig ─────────────────────────────────────────────────────────

.PHONY: build-init
build-init: ## Build mikrom-init with Zig and stage it for the agent package
	$(call check_tool,zig,zig is not installed. Install Zig to build mikrom-init)
	$(call in_dir,mikrom-init,zig build -Doptimize=ReleaseSafe)
	@mkdir -p target/release
	cp mikrom-init/zig-out/bin/mikrom-init target/release/mikrom-init

.PHONY: test-init
test-init: ## Run mikrom-init tests
	$(call check_tool,zig,zig is not installed. Install Zig to test mikrom-init)
	$(call in_dir,mikrom-init,zig build test)

# ── Run services: App and Dev ────────────────────────────────────────────────

.PHONY: run-app
run-app: ## Run mikrom-app dev server  (port 3001)
	$(call in_dir,mikrom-app,pnpm run dev --host)

.PHONY: dev
dev: ## Launch or attach to the tmux-based dev session
	@if tmux has-session -t mikrom 2>/dev/null; then \
		tmux attach-session -t mikrom; \
	else \
		tmux new-session -d -s mikrom -n api 'make run-api 2>&1 | tee /tmp/mikrom-api.log'; \
		tmux new-window -t mikrom -n scheduler 'make run-scheduler 2>&1 | tee /tmp/mikrom-scheduler.log'; \
		tmux new-window -t mikrom -n builder 'make run-builder 2>&1 | tee /tmp/mikrom-builder.log'; \
		tmux new-window -t mikrom -n app 'make run-app'; \
		tmux select-window -t mikrom:api; \
		tmux attach-session -t mikrom; \
	fi

.PHONY: dev-stop
dev-stop: ## Stop the tmux-based dev session
	@tmux kill-session -t mikrom 2>/dev/null || true

.PHONY: run-cli
run-cli: ## Run mikrom-cli  →  make run-cli ARGS="health"
	cargo run -p mikrom-cli -- $(ARGS)

.PHONY: install-cli
install-cli: ## Install the mikrom binary to ~/.cargo/bin
	cargo install --path mikrom-cli

# ── Frontends ────────────────────────────────────────────────────────────────

.PHONY: app-install
app-install: ## Install mikrom-app dependencies
	$(call in_dir,mikrom-app,pnpm install)

.PHONY: app-build
app-build: ## Build mikrom-app for production
	$(call in_dir,mikrom-app,pnpm build)

.PHONY: app-check
app-check: ## Run mikrom-app type and Svelte checks
	$(call in_dir,mikrom-app,pnpm check)

.PHONY: app-test
app-test: ## Run mikrom-app tests in watch mode
	$(call in_dir,mikrom-app,pnpm test)

.PHONY: app-test-unit
app-test-unit: ## Run mikrom-app unit tests
	$(call in_dir,mikrom-app,pnpm test:unit)

.PHONY: app-test-watch
app-test-watch: ## Run mikrom-app unit tests in watch mode
	$(call in_dir,mikrom-app,pnpm test:watch)

.PHONY: app-test-coverage
app-test-coverage: ## Run mikrom-app tests with coverage
	$(call in_dir,mikrom-app,pnpm test:coverage)

.PHONY: app-test-e2e
app-test-e2e: ## Run mikrom-app Playwright e2e tests
	$(call in_dir,mikrom-app,pnpm test:e2e)

.PHONY: app-lint
app-lint: ## Lint mikrom-app
	$(call in_dir,mikrom-app,pnpm lint)

# ── Docker: Base ──────────────────────────────────────────────────────────────

.PHONY: up
up: ## Start the core Docker Compose infrastructure stack
	docker compose up --build

.PHONY: up-detach
up-detach: ## Start the Docker Compose infrastructure stack in the background
	docker compose up --build -d

.PHONY: down
down: ## Stop and remove containers
	docker compose down

.PHONY: down-volumes
down-volumes: ## Stop containers and remove volumes (deletes DB data)
	docker compose down -v

.PHONY: db-start
db-start: ## Start PostgreSQL only (for local development)
	docker compose up -d --wait postgres

.PHONY: db-stop
db-stop: ## Stop PostgreSQL instance
	docker compose stop postgres

# ── Docker: Optional ──────────────────────────────────────────────────────────

.PHONY: up-buildkit
up-buildkit: ## Start the BuildKit service for local image builds
	docker compose --profile buildkit up --build -d buildkit

.PHONY: up-observability
up-observability: ## Start the observability stack
	docker compose --profile observability up --build -d otel-lgtm

.PHONY: up-full
up-full: ## Start the full local Compose stack
	docker compose --profile buildkit --profile observability up --build -d

.PHONY: down-full
down-full: ## Stop the full local development stack
	@$(MAKE) dev-stop
	@$(MAKE) down

# ── Docker: Logs ──────────────────────────────────────────────────────────────

.PHONY: logs
logs: ## Follow logs for the Docker Compose stack
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

.PHONY: logs-postgres
logs-postgres: logs-db ## Follow PostgreSQL logs

.PHONY: logs-db
logs-db: ## Follow PostgreSQL logs
	docker compose logs -f postgres

.PHONY: logs-nats
logs-nats: ## Follow NATS logs
	docker compose logs -f nats

.PHONY: logs-buildkit
logs-buildkit: ## Follow BuildKit logs
	docker compose --profile buildkit logs -f buildkit

.PHONY: logs-observability
logs-observability: ## Follow observability logs
	docker compose --profile observability logs -f otel-lgtm

# ── Housekeeping ──────────────────────────────────────────────────────────────

.PHONY: clean
clean: ## Remove Rust build artefacts
	cargo clean

.PHONY: check
check: fmt-check clippy test ## Run all checks (Rust)

.PHONY: help
help: ## Show this help
	@awk 'BEGIN {FS = ":.*##"; printf "\nUsage:\n  make \033[36m<target>\033[0m\n\nTargets:\n"} \
	  /^# ── / { section = $$0; sub(/^# ── /, "", section); sub(/[[:space:]─]+$$/, "", section); section_pending = 1; next } \
	  /^[a-zA-Z0-9_-]+:.*?##/ { if (section_pending) { printf "\n%s\n", section; section_pending = 0 } printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)
