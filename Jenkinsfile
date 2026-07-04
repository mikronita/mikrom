pipeline {
    agent any

    environment {
        NET       = "mikrom-ci-net"
        WORKSPACE = "/workspace"
    }

    stages {
        // ── Bootstrap ────────────────────────────────────────────────────────
        stage('Setup') {
            steps {
                checkout scm

                sh '''#!/bin/bash
                    set -eux

                    # Shared network for DinD and job containers
                    docker network create "$NET" 2>/dev/null || true

                    # DinD sidecar (Dagger engine provider)
                    docker rm -f dind 2>/dev/null || true
                    docker run -d --privileged           \
                        --network "$NET"                 \
                        --name dind                      \
                        docker:dind

                    until docker exec dind docker info >/dev/null 2>&1; do
                        sleep 1
                    done

                    # Long-lived builder container (Rust toolchain + system deps)
                    docker rm -f builder 2>/dev/null || true
                    docker run -d --network "$NET"        \
                        --name builder                    \
                        -w "$WORKSPACE"                   \
                        -v "$(pwd):$WORKSPACE"            \
                        -e DOCKER_HOST=tcp://dind:2375    \
                        -e CARGO_TERM_COLOR=always        \
                        -e RUST_BACKTRACE=1               \
                        rust:1.96-trixie                  \
                        bash -c "tail -f /dev/null"

                    docker exec builder bash -c "
                        set -eux
                        export DEBIAN_FRONTEND=noninteractive
                        apt-get update
                        apt-get install -y --no-install-recommends \
                            docker.io                           \
                            build-essential                     \
                            clang                               \
                            cmake                               \
                            curl                                \
                            git                                 \
                            libbpf-dev                          \
                            libclang-dev                        \
                            libelf-dev                          \
                            libssl-dev                          \
                            librados-dev                        \
                            librbd-dev                          \
                            llvm                                \
                            netcat-openbsd                      \
                            postgresql-client                   \
                            pkg-config                          \
                            protobuf-compiler                   \
                            zlib1g-dev
                        rm -rf /var/lib/apt/lists/*
                        rustup component add clippy rustfmt
                    "
                '''
            }
        }

        // ── Smoke ────────────────────────────────────────────────────────────
        stage('Smoke') {
            parallel {
                stage('fmt') {
                    steps {
                        sh '''#!/bin/bash
                            docker exec builder bash -c "
                                cd \"$WORKSPACE\"
                                cargo fmt --all -- --check
                            "
                        '''
                    }
                }

                stage('clippy') {
                    steps {
                        sh '''#!/bin/bash
                            docker exec -e CARGO_TARGET_DIR=/tmp/target-clippy builder bash -c "
                                cd \"$WORKSPACE\"
                                cargo clippy --workspace --exclude mikrom-agent-ebpf \
                                    --all-targets --all-features --locked -- -D warnings
                            "
                        '''
                    }
                }

                stage('app') {
                    steps {
                        sh '''#!/bin/bash
                            set -eux

                            # Node frontend builder (separate image)
                            docker rm -f app-builder 2>/dev/null || true
                            docker run -d --network "$NET"        \
                                --name app-builder                \
                                -w "$WORKSPACE/mikrom-app"        \
                                -v "$(pwd):$WORKSPACE"            \
                                node:24-trixie                    \
                                bash -c "tail -f /dev/null"

                            # Install pnpm and dependencies
                            docker exec app-builder bash -c "
                                set -eux
                                PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1 corepack enable
                                corepack prepare pnpm@9 --activate
                                pnpm config set store-dir /pnpm/store
                                pnpm install --frozen-lockfile
                            "

                            # Run app checks in parallel inside the container
                            docker exec app-builder pnpm check
                            docker exec app-builder pnpm lint
                            docker exec app-builder pnpm test:unit
                            docker exec app-builder pnpm build

                            docker rm -f app-builder
                        '''
                    }
                }
            }
        }

        // ── Tests ────────────────────────────────────────────────────────────
        stage('Test') {
            parallel {
                stage('test-core') {
                    steps {
                        sh '''#!/bin/bash
                            docker exec -e CARGO_TARGET_DIR=/tmp/target-test-core builder bash -c "
                                cd \"$WORKSPACE\"

                                # Serial: mikrom-proto, scheduler, agent, router, dns, network
                                cargo test -p mikrom-proto     --locked --lib
                                cargo test -p mikrom-scheduler  --locked --lib
                                cargo test -p mikrom-agent      --locked --lib
                                cargo test -p mikrom-router     --locked --lib
                                cargo test -p mikrom-dns        --locked --lib
                                cargo test -p mikrom-network    --locked --lib
                            "
                        '''
                    }
                }

                stage('test-api') {
                    steps {
                        sh '''#!/bin/bash
                            docker exec -e CARGO_TARGET_DIR=/tmp/target-test-api builder bash -c "
                                cd \"$WORKSPACE\"
                                cargo test -p mikrom-api --locked --lib --features test-utils \
                                    -- --test-threads=1
                            "
                        '''
                    }
                }

                stage('test-binaries') {
                    steps {
                        sh '''#!/bin/bash
                            docker exec -e CARGO_TARGET_DIR=/tmp/target-test-binaries builder bash -c "
                                cd \"$WORKSPACE\"
                                cargo test -p mikrom-cli --locked
                            "
                        '''
                    }
                }
            }
        }

        // ── Build ────────────────────────────────────────────────────────────
        stage('Build') {
            parallel {
                stage('release-build') {
                    steps {
                        sh '''#!/bin/bash
                            docker exec -e CARGO_TARGET_DIR=/tmp/target-release builder bash -c "
                                cd \"$WORKSPACE\"
                                cargo build --profile release-ci --locked \
                                    -p mikrom-api    -p mikrom-agent  \
                                    -p mikrom-builder -p mikrom-cli    \
                                    -p mikrom-dns    -p mikrom-network \
                                    -p mikrom-router -p mikrom-scheduler
                            "
                        '''
                    }
                }

                stage('ebpf-check') {
                    steps {
                        sh '''#!/bin/bash
                            docker exec -e CARGO_TARGET_DIR=/tmp/target-ebpf builder bash -c "
                                set -eux
                                cd \"$WORKSPACE\"
                                rustup toolchain install nightly --component rust-src
                                cargo +nightly check -p mikrom-agent-ebpf \
                                    --target bpfel-unknown-none \
                                    -Z build-std=core --locked
                            "
                        '''
                    }
                }
            }
        }

        // ── External integration tests (ignored by default, needs infra) ────
        stage('external-tests') {
            when {
                expression { env.RUN_EXTERNAL_TESTS ==~ /(?i)yes|true|1/ }
            }
            steps {
                sh '''#!/bin/bash
                    set -eux

                    # Start required services
                    docker rm -f test-postgres 2>/dev/null || true
                    docker run -d --network "$NET" --name test-postgres  \
                        -e POSTGRES_USER=mikrom                         \
                        -e POSTGRES_PASSWORD=mikrom_password            \
                        -e POSTGRES_DB=mikrom_test                      \
                        postgres:16

                    docker rm -f test-nats 2>/dev/null || true
                    docker run -d --network "$NET" --name test-nats nats:2

                    # Wait for readiness
                    docker exec builder bash -c "
                        set -eux
                        until pg_isready -h test-postgres -p 5432 -U mikrom -d mikrom_test; do
                            sleep 1
                        done
                        until nc -z test-nats 4222; do
                            sleep 1
                        done
                    "

                    # Run external tests
                    docker exec -e CARGO_TARGET_DIR=/tmp/target-ext       \
                        -e TEST_DATABASE_URL=postgres://mikrom:mikrom_password@test-postgres:5432/mikrom_test \
                        -e DATABASE_URL=postgres://mikrom:mikrom_password@test-postgres:5432/mikrom_test \
                        -e NATS_URL=nats://test-nats:4222                \
                        -e TEST_NATS_URL=nats://test-nats:4222           \
                        -e MIKROM_RUN_NATS_TESTS=1                       \
                        builder bash -c "
                            cd \"$WORKSPACE\"
                            cargo test -p mikrom-proto     --locked --test nats_protobuf_tests -- --ignored
                            cargo test -p mikrom-builder   --locked --test nats_build_tests -- --ignored
                            cargo test -p mikrom-dns       --locked --test integration -- --ignored
                            cargo test -p mikrom-api       --locked --features test-utils --tests -- --ignored
                            cargo test -p mikrom-scheduler --locked --features scheduler-e2e --tests -- --ignored
                            cargo test -p mikrom-agent     --locked --test nats_agent_tests -- --ignored
                        "

                    docker rm -f test-postgres test-nats
                '''
            }
        }

        // ── Service images ───────────────────────────────────────────────────
        stage('images') {
            when {
                expression { env.BUILD_IMAGES ==~ /(?i)yes|true|1/ }
            }
            steps {
                sh '''#!/bin/bash
                    set -eux
                    for df in mikrom-api/Dockerfile    \
                               mikrom-agent/Dockerfile  \
                               mikrom-builder/Dockerfile \
                               mikrom-cli/Dockerfile    \
                               mikrom-scheduler/Dockerfile; do
                        svc="${df%/Dockerfile}"
                        echo "Building $svc..."
                        docker exec builder bash -c "
                            docker build -f \"$WORKSPACE/$df\" -t \"$svc:latest\" \"$WORKSPACE\"
                        "
                    done
                '''
            }
        }

        // ── Publish ──────────────────────────────────────────────────────────
        stage('publish') {
            when {
                allOf {
                    expression { env.BUILD_IMAGES ==~ /(?i)yes|true|1/ }
                    expression { env.PUBLISH_IMAGES ==~ /(?i)yes|true|1/ }
                    expression { env.MIKROM_REGISTRY_USERNAME != null }
                    expression { env.MIKROM_REGISTRY_TOKEN != null }
                }
            }
            steps {
                sh '''#!/bin/bash
                    set -eux
                    PREFIX="${MIKROM_IMAGE_PREFIX:-ghcr.io/antpard/mikrom}"
                    TAG="${MIKROM_IMAGE_TAG:-latest}"

                    echo "$MIKROM_REGISTRY_TOKEN" | docker login \
                        --username "$MIKROM_REGISTRY_USERNAME"    \
                        --password-stdin                          \
                        "${MIKROM_REGISTRY_ADDRESS:-ghcr.io}"

                    for svc in mikrom-api mikrom-agent mikrom-builder \
                               mikrom-cli mikrom-scheduler; do
                        ref="$PREFIX/$svc:$TAG"
                        docker tag "$svc:latest" "$ref"
                        docker push "$ref"
                    done
                '''
            }
        }
    }

    post {
        always {
            sh '''#!/bin/bash
                set -eux
                docker rm -f builder app-builder test-postgres test-nats 2>/dev/null || true
                docker rm -f dind 2>/dev/null || true
                docker network rm "$NET" 2>/dev/null || true
            '''
            cleanWs()
        }
    }
}
