#!/usr/bin/env bash
set -euo pipefail

# Helper script to build the project inside the Ubuntu 20.04 builder image.
# Places build artifacts in target-docker/ and uses Docker volumes to cache
# cargo registry/git indices between runs.
#
# Usage:
#   ./ancillary/build-ubuntu20.sh                # runs: cargo build --release --locked
#   ./ancillary/build-ubuntu20.sh cargo test --verbose
#   ./ancillary/build-ubuntu20.sh "cargo build --release --features foo"

IMAGE_NAME=ubuntu20-rust-builder
DOCKERFILE=ancillary/Dockerfile.ubuntu20
CARGO_REGISTRY_VOL=ubuntu20-rust-cargo-registry
CARGO_GIT_VOL=ubuntu20-rust-cargo-git

HOST_WORKDIR=$(pwd)
USER_ID=${LOCAL_USER_ID:-$(id -u)}
GROUP_ID=${LOCAL_GROUP_ID:-$(id -g)}

# default command
if [ $# -eq 0 ]; then
  CMD="cargo build --release --locked"
else
  # join all args into a single shell command so callers can pass either
  # a single quoted command or multiple args
  CMD="$*"
fi

echo "Building Docker image '${IMAGE_NAME}' from ${DOCKERFILE}..."

docker build -t "${IMAGE_NAME}" -f "${DOCKERFILE}" .

echo "Running build inside container as UID:${USER_ID} GID:${GROUP_ID}..."

docker run --rm \
  -u "${USER_ID}:${GROUP_ID}" \
  -v "${HOST_WORKDIR}":/work \
  -v "${CARGO_REGISTRY_VOL}":/root/.cargo/registry \
  -v "${CARGO_GIT_VOL}":/root/.cargo/git \
  -e CARGO_HOME=/root/.cargo \
  -w /work \
  "${IMAGE_NAME}" sh -lc "${CMD}"

EXIT_CODE=$?

if [ ${EXIT_CODE} -eq 0 ]; then
  echo "Build finished. Artifacts are in target-docker/release/"
else
  echo "Build failed with exit code ${EXIT_CODE}" >&2
fi

exit ${EXIT_CODE}
