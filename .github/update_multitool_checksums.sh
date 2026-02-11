#!/usr/bin/env bash
# Refreshes SHA256 checksums in multitool.lock.json after Renovate bumps the
# versions.
#
# Usage: update_multitool_checksums.sh <dep-name>
set -euo pipefail

tool="${1:?usage: update_multitool_checksums.sh <dep-name>}"
lockfile="multitool.lock.json"

if [[ ! -f "$lockfile" ]]; then
	echo "error: $lockfile not found" >&2
	exit 1
fi

urls=$(jq -r '
    to_entries[]
    | select(.value | type == "object")
    | .value.binaries[]?
    | select(.url | contains("'"$tool"'"))
    | .url
' "$lockfile")

if [[ -z "$urls" ]]; then
	echo "error: no binaries matched tool '$tool'" >&2
	exit 1
fi

matched=0
updated=0
while IFS= read -r url; do
	matched=$((matched + 1))
	old_sha=$(jq -r '
        to_entries[]
        | select(.value | type == "object")
        | .value.binaries[]?
        | select(.url == "'"$url"'")
        | .sha256
    ' "$lockfile")

	new_sha=$(curl -fsSL "$url" | sha256sum | cut -d' ' -f1)

	if [[ "$old_sha" != "$new_sha" ]]; then
		sed -i'' -e "s/$old_sha/$new_sha/g" "$lockfile"
		updated=$((updated + 1))
	fi
done <<<"$urls"

echo "updated $updated of $matched checksum(s) for $tool" >&2
