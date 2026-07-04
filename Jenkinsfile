pipeline {
    agent any

    environment {
        // Tell Dagger SDK to connect to the DinD sidecar
        DOCKER_HOST = "tcp://dind:2375"
    }

    stages {
        stage('Full Pipeline') {
            steps {
                script {
                    // Create a shared Docker network so the build container
                    // can reach the DinD sidecar by hostname.
                    sh 'docker network create mikrom-ci-net 2>/dev/null || true'

                    // Start Docker-in-Docker sidecar for Dagger engine.
                    def dind = docker.image('docker:dind').run(
                        '--privileged --network mikrom-ci-net --name dind'
                    )

                    try {
                        // Run the build container on the same network.
                        docker.image('rust:1.96-trixie').inside(
                            '--network mikrom-ci-net'
                        ) {
                            checkout scm

                            sh '''
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
                            '''
                        }
                    } finally {
                        // Tear down the DinD sidecar.
                        dind.stop()
                        sh 'docker network rm mikrom-ci-net 2>/dev/null || true'
                    }
                }
            }
        }
    }

    post {
        always {
            cleanWs()
        }
    }
}
