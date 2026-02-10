#!/usr/bin/env bash
set -euo pipefail

curl -fsSL -o /usr/local/bin/bazel \
	https://github.com/bazelbuild/bazelisk/releases/download/v1.20.0/bazelisk-linux-amd64
chmod +x /usr/local/bin/bazel

exec renovate
