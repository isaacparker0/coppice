#!/usr/bin/env bash
set -euo pipefail

: "${DOCR_TOKEN:?DOCR_TOKEN must be set}"
: "${DOCKER_CONFIG:?DOCKER_CONFIG must be set}"
mkdir -p "${DOCKER_CONFIG}"
AUTH="$(printf "doctl:%s" "$DOCR_TOKEN" | base64 -w 0 2>/dev/null || printf "doctl:%s" "$DOCR_TOKEN" | base64)"
printf '{"auths":{"registry.digitalocean.com":{"auth":"%s"}}}\n' "$AUTH" >"${DOCKER_CONFIG}/config.json"
