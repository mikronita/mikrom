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
const CARGO_TARGET_CACHE: &str = "mikrom-ci-cargo-target";
const PNPM_STORE_CACHE: &str = "mikrom-ci-pnpm-store";
const PNPM_NODE_MODULES_CACHE: &str = "mikrom-ci-pnpm-node-modules";
const IMAGE_PREFIX_ENV: &str = "MIKROM_IMAGE_PREFIX";
const IMAGE_TAG_ENV: &str = "MIKROM_IMAGE_TAG";
const REGISTRY_ADDRESS_ENV: &str = "MIKROM_REGISTRY_ADDRESS";
const REGISTRY_USERNAME_ENV: &str = "MIKROM_REGISTRY_USERNAME";
const REGISTRY_TOKEN_ENV: &str = "MIKROM_REGISTRY_TOKEN";
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
    cargo_target: CacheVolume,
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
        let cargo_target = client.cache_volume(CARGO_TARGET_CACHE);
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
            cargo_target,
            pnpm_store,
            pnpm_node_modules,
        }
    }

    pub async fn validate_smoke(&self) -> Result<()> {
        self.fmt_check().await?;
        self.clippy_check().await?;
        self.app_validate().await?;

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
        self.release_build().await?;
        self.ebpf_check().await?;

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
        self.run_app_stage(&prepared, "app-check", "pnpm check")
            .await?;
        self.run_app_stage(&prepared, "app-lint", "pnpm lint")
            .await?;
        self.run_app_stage(&prepared, "app-test", "pnpm test:unit")
            .await?;
        self.run_app_stage(&prepared, "app-build", "pnpm build")
            .await?;

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
        let container = self.workspace_test_container();
        let command = "set -eux; \
            until pg_isready -h postgres -p 5432 -U mikrom -d mikrom_test; do sleep 1; done; \
            until nc -z nats 4222; do sleep 1; done; \
            /usr/local/cargo/bin/cargo test --workspace --exclude mikrom-agent-ebpf --lib --all-features --locked";

        info!(stage = "test", command, "starting");
        container
            .with_exec(vec!["sh", "-lc", command])
            .combined_output()
            .await
            .with_context(|| "stage test failed".to_string())?;
        info!(stage = "test", "finished");
        Ok(())
    }

    pub async fn release_build(&self) -> Result<()> {
        self.run_stage(
            "build",
            "/usr/local/cargo/bin/cargo build --workspace --exclude mikrom-agent-ebpf --release --locked",
        )
        .await
    }

    pub async fn ebpf_check(&self) -> Result<()> {
        let container = self.workspace_container().with_exec(vec![
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
        info!(stage, command, "starting");

        self.workspace_container()
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
        let tar_path = format!("/tmp/{service}.tar");

        image
            .export(&tar_path)
            .await
            .with_context(|| format!("failed to export {service} image"))?;

        info!(service, tar_path = %tar_path, "image exported");

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
        self.base_container
            .clone()
            .with_mounted_directory(WORKDIR, self.source.clone())
            .with_workdir(WORKDIR)
            .with_mounted_cache("/usr/local/cargo/registry", self.cargo_registry.clone())
            .with_mounted_cache("/usr/local/cargo/git", self.cargo_git.clone())
            .with_mounted_cache(format!("{WORKDIR}/target"), self.cargo_target.clone())
            .with_env_variable("CARGO_TARGET_DIR", format!("{WORKDIR}/target"))
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
