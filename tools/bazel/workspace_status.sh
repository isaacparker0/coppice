#!/usr/bin/env bash
set -euo pipefail

# Bazel workspace status command
# https://bazel.build/docs/user-manual#workspace-status

commit_sha=$(git rev-parse HEAD)
echo "STABLE_COMMIT_SHA $commit_sha"
