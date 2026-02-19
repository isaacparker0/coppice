#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
	echo "usage: bazel run //playground/deploy:deploy_from_buildbuddy" >&2
	exit 2
fi

: "${PLAYGROUND_DEPLOY_SSH_PRIVATE_KEY:?PLAYGROUND_DEPLOY_SSH_PRIVATE_KEY must be set}"

commit_sha_file="$1"
commit_sha="$(tr -d '[:space:]' <"$commit_sha_file")"
if [[ -z "$commit_sha" || "$commit_sha" == "__COMMIT_SHA__" ]]; then
	echo "invalid stamped commit sha: '$commit_sha'" >&2
	exit 2
fi

deploy_host="142.93.195.217"

ssh_dir="${HOME}/.ssh"
key_path="${ssh_dir}/id_ed25519"
known_hosts_path="${ssh_dir}/known_hosts"

mkdir -p "$ssh_dir"
chmod 700 "$ssh_dir"
printf "%s\n" "$PLAYGROUND_DEPLOY_SSH_PRIVATE_KEY" >"$key_path"
chmod 600 "$key_path"
ssh-keyscan -H "$deploy_host" >>"$known_hosts_path"

exec ssh -p 22 \
	-o BatchMode=yes \
	-o StrictHostKeyChecking=accept-new \
	"root@${deploy_host}" \
	"sudo /usr/local/bin/coppice-playground-deploy ${commit_sha}"
