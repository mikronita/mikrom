pipeline {
    agent any

    environment {
        DOCKER_HOST = "tcp://dind:2375"
    }

    stages {
        stage('Setup and Full Pipeline') {
            steps {
                checkout scm

                sh '''
                    set -eux

                    NET="mikrom-ci-net"

                    # Create a shared Docker network so the build container
                    # can reach the DinD sidecar by hostname.
                    docker network create "$NET" 2>/dev/null || true

                    # Start Docker-in-Docker sidecar for Dagger engine.
                    docker rm -f dind 2>/dev/null || true
                    docker run -d --privileged                        \
                        --network "$NET"                              \
                        --name dind                                   \
                        docker:dind

                    # Wait for DinD to be ready.
                    until docker exec dind docker info >/dev/null 2>&1; do
                        sleep 1
                    done

                    # Run the build container with the source mounted.
                    docker rm -f mikrom-ci-builder 2>/dev/null || true
                    docker run --rm                                  \
                        --network "$NET"                             \
                        --name mikrom-ci-builder                     \
                        -w /workspace                                \
                        -v "$(pwd):/workspace"                       \
                        -e DOCKER_HOST="tcp://dind:2375"             \
                        rust:1.96-trixie                             \
                        sh -c "
                            set -eux

                            # Install build dependencies (mirrors BASE_PACKAGES in pipeline.rs).
                            apt-get update && apt-get install -y --no-install-recommends \
                                docker.io \
                                build-essential \
                                clang \
                                cmake \
                                curl \
                                git \
                                libbpf-dev \
                                libclang-dev \
                                libelf-dev \
                                libssl-dev \
                                librados-dev \
                                librbd-dev \
                                llvm \
                                netcat-openbsd \
                                postgresql-client \
                                pkg-config \
                                protobuf-compiler \
                                zlib1g-dev

                            # Install Rust toolchain components needed by the pipeline.
                            rustup component add clippy rustfmt

                            # Run the full Dagger-based CI pipeline.
                            # Dagger SDK auto-detects DOCKER_HOST and starts its engine
                            # inside the DinD sidecar.
                            make ci-full
                        "

                    # Tear down the DinD sidecar.
                    docker rm -f dind 2>/dev/null || true
                    docker network rm "$NET" 2>/dev/null || true
                '''
            }
        }
    }

    post {
        always {
            cleanWs()
        }
    }
}
