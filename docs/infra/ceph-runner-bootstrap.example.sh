#!/usr/bin/env bash
set -euo pipefail

# Example bootstrap script for a self-hosted GitHub Actions runner dedicated to Ceph tests.
# Replace the placeholder values before using it in automation.

: "${GITHUB_URL:?set GITHUB_URL to the GitHub org or repo URL}"
: "${RUNNER_NAME:=mikrom-ceph-runner-1}"
: "${RUNNER_LABELS:=self-hosted,linux,ceph}"
: "${RUNNER_WORKDIR:=_work}"
: "${RUNNER_HOME:=/opt/actions-runner}"
: "${RUNNER_VERSION:=2.322.0}"

if [ ! -r /etc/ceph/ceph.conf ]; then
  echo "Missing /etc/ceph/ceph.conf"
  exit 1
fi

if [ ! -r /etc/ceph/admin.secret ]; then
  echo "Missing /etc/ceph/admin.secret"
  exit 1
fi

sudo mkdir -p "$RUNNER_HOME"
sudo chown "$(id -u)":"$(id -g)" "$RUNNER_HOME"
cd "$RUNNER_HOME"

if [ ! -x ./bin/Runner.Listener ]; then
  curl -fsSL -o actions-runner.tar.gz \
    "https://github.com/actions/runner/releases/download/v${RUNNER_VERSION}/actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz"
  tar xzf actions-runner.tar.gz
fi

if [ ! -f .env ]; then
  cat > .env <<EOF
GITHUB_URL=${GITHUB_URL}
RUNNER_NAME=${RUNNER_NAME}
RUNNER_LABELS=${RUNNER_LABELS}
RUNNER_WORKDIR=${RUNNER_WORKDIR}
EOF
fi

if [ -z "${RUNNER_TOKEN:-}" ]; then
  echo "Set RUNNER_TOKEN to the short-lived registration token from GitHub before running config.sh"
  exit 1
fi

./config.sh \
  --url "$GITHUB_URL" \
  --token "$RUNNER_TOKEN" \
  --name "$RUNNER_NAME" \
  --labels "$RUNNER_LABELS" \
  --work "$RUNNER_WORKDIR" \
  --unattended \
  --replace

sudo ./svc.sh install
sudo ./svc.sh start

