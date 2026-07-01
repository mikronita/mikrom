use dagger_sdk::{
    CacheVolume, Container, Directory, DirectoryDockerBuildOpts, HostDirectoryOpts, Query, Service,
};
use eyre::{Context, Result};
use std::env;
use tracing::info;

const WORKDIR: &str = "/workspace";
const RUST_IMAGE: &str = "rust:1.96-trixie";
const NODE_IMAGE: &str = "node:24-trixie";
const PLAYWRIGHT_IMAGE: &str = "mcr.microsoft.com/playwright:v1.60.0-noble";
const CARGO_BIN_DIR: &str = "/usr/local/cargo/bin";
const CARGO_REGISTRY_CACHE: &str = "mikrom-ci-cargo-registry";
const CARGO_GIT_CACHE: &str = "mikrom-ci-cargo-git";
const CARGO_TARGET_DEBUG_CACHE: &str = "mikrom-ci-cargo-target-debug";
const CARGO_TARGET_RELEASE_CACHE: &str = "mikrom-ci-cargo-target-release";
const CARGO_TARGET_EBPF_CACHE: &str = "mikrom-ci-cargo-target-ebpf";
const PNPM_STORE_CACHE: &str = "mikrom-ci-pnpm-store";
const PNPM_NODE_MODULES_CACHE: &str = "mikrom-ci-pnpm-node-modules";
const IMAGE_PREFIX_ENV: &str = "MIKROM_IMAGE_PREFIX";
const IMAGE_TAG_ENV: &str = "MIKROM_IMAGE_TAG";
const REGISTRY_ADDRESS_ENV: &str = "MIKROM_REGISTRY_ADDRESS";
const REGISTRY_USERNAME_ENV: &str = "MIKROM_REGISTRY_USERNAME";
const REGISTRY_TOKEN_ENV: &str = "MIKROM_REGISTRY_TOKEN";
const TEST_GROUP_ENV: &str = "MIKROM_TEST_GROUP";
const TEST_GROUP_FULL: &str = "full";
const TEST_GROUP_CORE: &str = "core";
const TEST_GROUP_API: &str = "api";
const TEST_GROUP_BINARIES: &str = "binaries";
const BASE_PACKAGES: &[&str] = &[
    "bash",
    "build-essential",
    "ca-certificates",
    "clang",
    "curl",
    "cmake",
    "git",
    "libbpf-dev",
    "libclang-dev",
    "libelf-dev",
    "libssl-dev",
    "librados-dev",
    "librbd-dev",
    "llvm",
    "netcat-openbsd",
    "postgresql-client",
    "pkg-config",
    "protobuf-compiler",
    "zlib1g-dev",
];
const APP_PACKAGES: &[&str] = &["bash", "ca-certificates", "git"];
const POSTGRES_IMAGE: &str = "postgres:16";
const NATS_IMAGE: &str = "nats:2";
const TEST_DATABASE_URL: &str = "postgres://mikrom:mikrom_password@postgres:5432/mikrom_test";
const TEST_NATS_URL: &str = "nats://nats:4222";

#[derive(Clone, Copy)]
struct TestSpec {
    package: &'static str,
    lib: bool,
    features: Option<&'static str>,
}

const CORE_TEST_SPECS: &[TestSpec] = &[
    TestSpec {
        package: "mikrom-proto",
        lib: true,
        features: None,
    },
    TestSpec {
        package: "mikrom-scheduler",
        lib: true,
        features: None,
    },
    TestSpec {
        package: "mikrom-agent",
        lib: true,
        features: None,
    },
    TestSpec {
        package: "mikrom-router",
        lib: true,
        features: None,
    },
    TestSpec {
        package: "mikrom-dns",
        lib: true,
        features: None,
    },
    TestSpec {
        package: "mikrom-network",
        lib: true,
        features: None,
    },
];

const API_TEST_SPECS: &[TestSpec] = &[TestSpec {
    package: "mikrom-api",
    lib: true,
    features: Some("test-utils"),
}];

const BINARY_TEST_SPECS: &[TestSpec] = &[TestSpec {
    package: "mikrom-cli",
    lib: false,
    features: None,
}];

const SERVICE_IMAGES: &[(&str, &str)] = &[
    ("mikrom-api", "mikrom-api/Dockerfile"),
    ("mikrom-agent", "mikrom-agent/Dockerfile"),
    ("mikrom-builder", "mikrom-builder/Dockerfile"),
    ("mikrom-cli", "mikrom-cli/Dockerfile"),
    ("mikrom-scheduler", "mikrom-scheduler/Dockerfile"),
];

#[derive(Clone)]
pub struct MikromCi {
    client: Query,
    source: Directory,
    base_container: Container,
    cargo_registry: CacheVolume,
    cargo_git: CacheVolume,
    cargo_target_debug: CacheVolume,
    cargo_target_release: CacheVolume,
    cargo_target_ebpf: CacheVolume,
    rustup: CacheVolume,
    pnpm_store: CacheVolume,
    pnpm_node_modules: CacheVolume,
}

impl MikromCi {
    #[must_use]
    pub fn new(client: &Query) -> Self {
        let client = client.clone();
        let source = client.host().directory_opts(
            ".",
            HostDirectoryOpts {
                exclude: Some(vec!["target", ".git", ".codex", "mikrom-app/node_modules"]),
                gitignore: Some(true),
                include: None,
                no_cache: None,
            },
        );
        let cargo_registry = client.cache_volume(CARGO_REGISTRY_CACHE);
        let cargo_git = client.cache_volume(CARGO_GIT_CACHE);
        let cargo_target_debug = client.cache_volume(CARGO_TARGET_DEBUG_CACHE);
        let cargo_target_release = client.cache_volume(CARGO_TARGET_RELEASE_CACHE);
        let cargo_target_ebpf = client.cache_volume(CARGO_TARGET_EBPF_CACHE);
        let rustup = client.cache_volume("mikrom-ci-rustup");
        let pnpm_store = client.cache_volume(PNPM_STORE_CACHE);
        let pnpm_node_modules = client.cache_volume(PNPM_NODE_MODULES_CACHE);

        let install_cmd = format!(
            "set -eux; \
             export DEBIAN_FRONTEND=noninteractive; \
             apt-get update; \
             apt-get install -y --no-install-recommends {}; \
             /usr/local/cargo/bin/rustup component add clippy rustfmt; \
             rm -rf /var/lib/apt/lists/*",
            BASE_PACKAGES.join(" ")
        );

        let base_container = client
            .container()
            .from(RUST_IMAGE)
            .with_env_variable("DEBIAN_FRONTEND", "noninteractive")
            .with_env_variable(
                "PATH",
                format!(
                    "{CARGO_BIN_DIR}:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
                ),
            )
            .with_exec(vec!["sh", "-lc", &install_cmd]);

        Self {
            client,
            source,
            base_container,
            cargo_registry,
            cargo_git,
            cargo_target_debug,
            cargo_target_release,
            cargo_target_ebpf,
            rustup,
            pnpm_store,
            pnpm_node_modules,
        }
    }

    pub async fn validate_smoke(&self) -> Result<()> {
        tokio::try_join!(self.fmt_check(), self.clippy_check(), self.app_validate())?;

        Ok(())
    }

    pub async fn validate(&self) -> Result<()> {
        self.validate_full().await
    }

    pub async fn validate_fast(&self) -> Result<()> {
        self.validate_smoke().await?;
        self.workspace_tests().await?;

        Ok(())
    }

    pub async fn validate_full(&self) -> Result<()> {
        self.validate_fast().await?;
        tokio::try_join!(self.release_build(), self.ebpf_check())?;

        Ok(())
    }

    pub async fn publish_release(&self) -> Result<()> {
        self.validate_full().await?;
        self.build_service_images().await?;
        self.publish_service_images().await
    }

    pub async fn app_validate(&self) -> Result<()> {
        info!(stage = "app-install", "starting");
        let prepared = self.prepare_app_container(NODE_IMAGE).await?;
        info!(stage = "app-install", "finished");
        tokio::try_join!(
            self.run_app_stage(&prepared, "app-check", "pnpm check"),
            self.run_app_stage(&prepared, "app-lint", "pnpm lint"),
            self.run_app_stage(&prepared, "app-test", "pnpm test:unit"),
            self.run_app_stage(&prepared, "app-build", "pnpm build"),
        )?;

        Ok(())
    }

    pub async fn app_e2e(&self) -> Result<()> {
        info!(stage = "app-install", "starting");
        let prepared = self.prepare_app_container(PLAYWRIGHT_IMAGE).await?;
        info!(stage = "app-install", "finished");
        self.run_app_stage(&prepared, "app-e2e", "pnpm test:e2e")
            .await
    }

    pub async fn fmt_check(&self) -> Result<()> {
        self.run_stage("fmt", "/usr/local/cargo/bin/cargo fmt --all -- --check")
            .await
    }

    pub async fn clippy_check(&self) -> Result<()> {
        self.run_stage(
            "clippy",
            "/usr/local/cargo/bin/cargo clippy --workspace --exclude mikrom-agent-ebpf --all-targets --all-features --locked -- -D warnings",
        )
        .await
    }

    pub async fn workspace_tests(&self) -> Result<()> {
        match env::var(TEST_GROUP_ENV)
            .unwrap_or_else(|_| TEST_GROUP_FULL.to_string())
            .as_str()
        {
            TEST_GROUP_CORE => {
                self.run_workspace_test_group("test-core", CORE_TEST_SPECS)
                    .await?;
            },
            TEST_GROUP_API => {
                self.run_workspace_test_group("test-api", API_TEST_SPECS)
                    .await?;
            },
            TEST_GROUP_BINARIES => {
                self.run_workspace_test_group("test-binaries", BINARY_TEST_SPECS)
                    .await?;
            },
            TEST_GROUP_FULL => {
                self.run_workspace_test_group("test-core", CORE_TEST_SPECS)
                    .await?;
                self.run_workspace_test_group("test-api", API_TEST_SPECS)
                    .await?;
                self.run_workspace_test_group("test-binaries", BINARY_TEST_SPECS)
                    .await?;
            },
            other => {
                return Err(eyre::eyre!(
                    "unknown {TEST_GROUP_ENV} value '{other}', expected one of: {TEST_GROUP_CORE}, {TEST_GROUP_API}, {TEST_GROUP_BINARIES}, {TEST_GROUP_FULL}"
                ));
            },
        }

        Ok(())
    }

    pub async fn workspace_external_tests(&self) -> Result<()> {
        let container = self
            .workspace_test_container()
            .with_env_variable("MIKROM_RUN_NATS_TESTS", "1");
        let wait_command = "set -eux; \
            until pg_isready -h postgres -p 5432 -U mikrom -d mikrom_test; do sleep 1; done; \
            until nc -z nats 4222; do sleep 1; done";

        container
            .with_exec(vec!["sh", "-lc", wait_command])
            .combined_output()
            .await
            .with_context(|| "external-tests readiness failed")?;

        let commands = [
            (
                "test-proto-external",
                "/usr/local/cargo/bin/cargo test -p mikrom-proto --locked --test nats_protobuf_tests -- --ignored",
            ),
            (
                "test-builder-external",
                "/usr/local/cargo/bin/cargo test -p mikrom-builder --locked --test nats_build_tests -- --ignored",
            ),
            (
                "test-dns-external",
                "/usr/local/cargo/bin/cargo test -p mikrom-dns --locked --test integration -- --ignored",
            ),
            (
                "test-api-external",
                "/usr/local/cargo/bin/cargo test -p mikrom-api --locked --features test-utils --tests -- --ignored",
            ),
            (
                "test-scheduler-external",
                "/usr/local/cargo/bin/cargo test -p mikrom-scheduler --locked --features scheduler-e2e --tests -- --ignored",
            ),
            (
                "test-agent-external",
                "/usr/local/cargo/bin/cargo test -p mikrom-agent --locked --test nats_agent_tests -- --ignored",
            ),
        ];

        for (stage, command) in commands {
            self.run_stage_with_container(stage, container.clone(), command)
                .await
                .with_context(|| format!("stage {stage} failed"))?;
        }

        Ok(())
    }

    pub async fn release_build(&self) -> Result<()> {
        self.run_stage_with_container(
            "build",
            self.workspace_release_container(),
            "/usr/local/cargo/bin/cargo build --profile release-ci --locked -p mikrom-api -p mikrom-agent -p mikrom-builder -p mikrom-cli -p mikrom-dns -p mikrom-network -p mikrom-router -p mikrom-scheduler",
        )
        .await
    }

    pub async fn ebpf_check(&self) -> Result<()> {
        let container = self.workspace_ebpf_container().with_exec(vec![
            "sh",
            "-lc",
            "/usr/local/cargo/bin/rustup toolchain install nightly --component rust-src",
        ]);

        info!(
            stage = "ebpf",
            command = "cargo +nightly check -p mikrom-agent-ebpf --target bpfel-unknown-none -Z build-std=core --locked",
            "starting"
        );

        container
            .with_exec(vec![
                "sh",
                "-lc",
                "/usr/local/cargo/bin/cargo +nightly check -p mikrom-agent-ebpf --target bpfel-unknown-none -Z build-std=core --locked",
            ])
            .combined_output()
            .await
            .with_context(|| "stage ebpf failed".to_string())?;

        info!(stage = "ebpf", "finished");

        Ok(())
    }

    pub async fn build_service_images(&self) -> Result<()> {
        for (service, dockerfile) in SERVICE_IMAGES {
            self.build_service_image(service, dockerfile).await?;
        }

        Ok(())
    }

    pub async fn publish_service_images(&self) -> Result<()> {
        let prefix =
            env::var(IMAGE_PREFIX_ENV).unwrap_or_else(|_| "ghcr.io/antpard/mikrom".to_string());
        let tag = env::var(IMAGE_TAG_ENV).unwrap_or_else(|_| "latest".to_string());
        let registry_address = env::var(REGISTRY_ADDRESS_ENV)
            .unwrap_or_else(|_| prefix.split('/').next().unwrap_or(&prefix).to_string());
        let registry_username = env::var(REGISTRY_USERNAME_ENV)
            .with_context(|| format!("set {REGISTRY_USERNAME_ENV} to publish service images"))?;
        let registry_token = env::var(REGISTRY_TOKEN_ENV)
            .with_context(|| format!("set {REGISTRY_TOKEN_ENV} to publish service images"))?;

        for (service, dockerfile) in SERVICE_IMAGES {
            let image = self.service_image(service, dockerfile);
            let authenticated = image.with_registry_auth(
                &registry_address,
                &registry_username,
                self.client
                    .set_secret("registry-token", registry_token.clone()),
            );
            let published_ref = format!("{prefix}/{service}:{tag}");
            let digest = authenticated.publish(&published_ref).await?;
            info!(service, published_ref = %published_ref, digest = %digest, "published");
        }

        Ok(())
    }

    async fn run_stage(&self, stage: &str, command: &str) -> Result<()> {
        self.run_stage_with_container(stage, self.workspace_container(), command)
            .await
    }

    async fn run_stage_with_container(
        &self,
        stage: &str,
        container: Container,
        command: &str,
    ) -> Result<()> {
        info!(stage, command, "starting");

        container
            .with_exec(vec!["sh", "-lc", command])
            .combined_output()
            .await
            .with_context(|| format!("stage {stage} failed"))?;

        info!(stage, "finished");

        Ok(())
    }

    async fn run_app_stage(&self, base: &Container, stage: &str, command: &str) -> Result<()> {
        info!(stage, command, "starting");

        base.clone()
            .with_exec(vec!["sh", "-lc", command])
            .combined_output()
            .await
            .with_context(|| format!("stage {stage} failed"))?;

        info!(stage, "finished");

        Ok(())
    }

    async fn run_workspace_test_group(&self, stage: &str, specs: &[TestSpec]) -> Result<()> {
        let container = self.workspace_test_container();
        let wait_command = "set -eux; \
            until pg_isready -h postgres -p 5432 -U mikrom -d mikrom_test; do sleep 1; done; \
            until nc -z nats 4222; do sleep 1; done";

        container
            .with_exec(vec!["sh", "-lc", wait_command])
            .combined_output()
            .await
            .with_context(|| format!("stage {stage} readiness failed"))?;

        for spec in specs {
            let mut command = format!(
                "/usr/local/cargo/bin/cargo test -p {} --locked",
                spec.package
            );

            if spec.lib {
                command.push_str(" --lib");
            }

            if let Some(features) = spec.features {
                command.push_str(" --features ");
                command.push_str(features);
            }

            if stage == "test-api" {
                // mikrom-api tests share a single database fixture; serialize them to avoid
                // cross-test interference while keeping the rest of the workspace parallel.
                command.push_str(" -- --test-threads=1");
            }

            self.run_stage_with_container(stage, container.clone(), &command)
                .await
                .with_context(|| format!("stage {stage} failed for {}", spec.package))?;
        }

        Ok(())
    }

    async fn prepare_app_container(&self, image: &str) -> Result<Container> {
        let prepared = self.app_container(image).with_exec(vec![
            "sh",
            "-lc",
            "PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1 corepack enable && corepack prepare pnpm@9 --activate && pnpm config set store-dir /pnpm/store && pnpm install --frozen-lockfile",
        ]);

        prepared
            .combined_output()
            .await
            .context("app-install failed")?;

        Ok(prepared)
    }

    async fn build_service_image(&self, service: &str, dockerfile: &str) -> Result<()> {
        let image = self.service_image(service, dockerfile);

        image
            .id()
            .await
            .with_context(|| format!("failed to build {service} image"))?;

        info!(service, "image built successfully");

        Ok(())
    }

    fn service_image(&self, service: &str, dockerfile: &str) -> Container {
        info!(service, dockerfile, "building service image");

        self.source
            .clone()
            .docker_build_opts(DirectoryDockerBuildOpts {
                build_args: None,
                dockerfile: Some(dockerfile),
                no_init: None,
                platform: None,
                secrets: None,
                ssh: None,
                target: None,
            })
    }

    fn workspace_container(&self) -> Container {
        self.workspace_container_with_target(
            self.cargo_target_debug.clone(),
            format!("{WORKDIR}/target"),
        )
    }

    fn workspace_release_container(&self) -> Container {
        self.workspace_container_with_target(
            self.cargo_target_release.clone(),
            format!("{WORKDIR}/target-release"),
        )
    }

    fn workspace_ebpf_container(&self) -> Container {
        self.workspace_container_with_target(
            self.cargo_target_ebpf.clone(),
            format!("{WORKDIR}/target-ebpf"),
        )
    }

    fn workspace_container_with_target(
        &self,
        cargo_target: CacheVolume,
        target_dir: String,
    ) -> Container {
        self.base_container
            .clone()
            .with_mounted_directory(WORKDIR, self.source.clone())
            .with_workdir(WORKDIR)
            .with_mounted_cache("/usr/local/cargo/registry", self.cargo_registry.clone())
            .with_mounted_cache("/usr/local/cargo/git", self.cargo_git.clone())
            .with_mounted_cache(target_dir.clone(), cargo_target)
            .with_mounted_cache("/root/.rustup", self.rustup.clone())
            .with_env_variable("PROTOC", "/usr/bin/protoc")
            .with_env_variable("CARGO_TARGET_DIR", target_dir)
            .with_env_variable("CARGO_TERM_COLOR", "always")
            .with_env_variable("RUST_BACKTRACE", "1")
    }

    fn workspace_test_container(&self) -> Container {
        self.workspace_container()
            .with_service_binding("postgres", self.postgres_service())
            .with_service_binding("nats", self.nats_service())
            .with_env_variable("TEST_DATABASE_URL", TEST_DATABASE_URL)
            .with_env_variable("DATABASE_URL", TEST_DATABASE_URL)
            .with_env_variable("NATS_URL", TEST_NATS_URL)
            .with_env_variable("TEST_NATS_URL", TEST_NATS_URL)
    }

    fn postgres_service(&self) -> Service {
        self.client
            .container()
            .from(POSTGRES_IMAGE)
            .with_env_variable("POSTGRES_USER", "mikrom")
            .with_env_variable("POSTGRES_PASSWORD", "mikrom_password")
            .with_env_variable("POSTGRES_DB", "mikrom_test")
            .with_exposed_port(5432)
            .as_service()
    }

    fn nats_service(&self) -> Service {
        self.client
            .container()
            .from(NATS_IMAGE)
            .with_exposed_port(4222)
            .as_service()
    }

    fn app_container(&self, image: &str) -> Container {
        self.app_base_container(image)
            .with_mounted_directory(WORKDIR, self.source.clone())
            .with_workdir(format!("{WORKDIR}/mikrom-app"))
            .with_mounted_cache("/pnpm/store", self.pnpm_store.clone())
            .with_mounted_cache(
                format!("{WORKDIR}/mikrom-app/node_modules"),
                self.pnpm_node_modules.clone(),
            )
            .with_env_variable("CI", "true")
            .with_env_variable("PNPM_HOME", "/pnpm")
            .with_env_variable("PROTOC", "/usr/bin/protoc")
            .with_env_variable("CARGO_TERM_COLOR", "always")
            .with_env_variable("PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD", "1")
    }

    fn app_base_container(&self, image: &str) -> Container {
        self.client
            .container()
            .from(image)
            .with_env_variable("DEBIAN_FRONTEND", "noninteractive")
            .with_env_variable(
                "PATH",
                "/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
            )
            .with_exec(vec![
                "sh",
                "-lc",
                &format!(
                    "set -eux; \
                     export DEBIAN_FRONTEND=noninteractive; \
                     apt-get update; \
                     apt-get install -y --no-install-recommends {}; \
                     rm -rf /var/lib/apt/lists/*",
                    APP_PACKAGES.join(" ")
                ),
            ])
    }
}
