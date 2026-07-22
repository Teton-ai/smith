#!/usr/bin/env bash
# Build the device image (smithd + smith-updater) from the latest release tag,
# tagged as smith-device:latest so the compose stack picks it up. Lets the e2e
# suite run the *released* daemon against the HEAD API — the version skew the
# fleet actually experiences when the API deploys.
#
# Usage: ./scripts/build-released-device.sh
#        SMITH_DAEMON_TAG=v0.2.170 ./scripts/build-released-device.sh
# Then:  E2E_UP_FLAGS=--no-build make test.e2e   (api image must already exist)
set -euo pipefail

tag="${SMITH_DAEMON_TAG:-}"
if [ -z "$tag" ]; then
  git fetch --tags --quiet
  tag=$(git tag --list 'v*' --sort=-v:refname | head -1)
fi
if [ -z "$tag" ]; then
  echo "No release tag found; set SMITH_DAEMON_TAG explicitly" >&2
  exit 1
fi

echo "Building device image from release ${tag}"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
git archive "$tag" | tar -x -C "$tmp"

docker build \
  -f "$tmp/smithd/dev.Dockerfile" \
  --build-arg BASE_IMAGE="${DEVICE_BASE_IMAGE:-ubuntu:22.04}" \
  -t smith-device:latest \
  "$tmp"
