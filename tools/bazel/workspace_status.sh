#!/usr/bin/env bash
set -euo pipefail

if git_sha="$(git rev-parse HEAD 2>/dev/null)"; then
	echo "STABLE_COMMIT_SHA ${git_sha}"
else
	echo "STABLE_COMMIT_SHA unknown"
fi
