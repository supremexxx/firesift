#!/usr/bin/env bash
set -Eeuo pipefail

: "${PYRORISK_SSH_TARGET:?Set PYRORISK_SSH_TARGET, for example pyrorisk@server.example}"

deploy_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd -- "${deploy_dir}/../.." && pwd)"
remote_dir="${PYRORISK_REMOTE_DIR:-/opt/pyrorisk}"
if ! git -C "${project_dir}" diff --quiet || \
  ! git -C "${project_dir}" diff --cached --quiet; then
  printf 'Refusing to deploy tracked, uncommitted changes. Commit them first.\n' >&2
  exit 1
fi
untracked_build_inputs="$(git -C "${project_dir}" ls-files --others --exclude-standard -- \
  Cargo.toml Cargo.lock Dockerfile crates migrations testdata)"
if [[ -n "${untracked_build_inputs}" ]]; then
  printf 'Refusing to deploy untracked build inputs:\n%s\n' "${untracked_build_inputs}" >&2
  exit 1
fi
revision="$(git -C "${project_dir}" rev-parse HEAD)"
created_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
ssh_options=(-o BatchMode=yes -o ConnectTimeout=15)
if [[ -n "${PYRORISK_SSH_KEY:-}" ]]; then
  ssh_options+=(-i "${PYRORISK_SSH_KEY}")
fi

rsync -az \
  -e "ssh ${ssh_options[*]}" \
  --no-owner \
  --no-group \
  --no-perms \
  --chmod='Du=rwx,Dgo=rx,Fu=rw,Fgo=r,a+X' \
  --exclude '.env' \
  --exclude '.git' \
  --exclude '.playwright-cli' \
  --exclude 'backups' \
  --exclude 'data' \
  --exclude 'out' \
  --exclude 'target' \
  "${project_dir}/" "${PYRORISK_SSH_TARGET}:${remote_dir}/"

ssh "${ssh_options[@]}" "${PYRORISK_SSH_TARGET}" bash -s -- \
  "${remote_dir}" "${revision}" "${created_at}" <<'REMOTE'
set -Eeuo pipefail
remote_dir="$1"
revision="$2"
created_at="$3"
deploy_dir="${remote_dir}/deploy/oracle"
cd "${deploy_dir}"
env_file="./.env"

env_file_value() {
  local name="$1"
  local value
  value="$(awk -F= -v key="${name}" '$1 == key {sub(/^[^=]*=/, ""); print; exit}' "${env_file}")"
  if [[ "${value}" == \"*\" || "${value}" == \'*\' ]]; then
    value="${value:1:${#value}-2}"
  fi
  printf '%s' "${value}"
}

set_env_file_value() {
  local name="$1"
  local value="$2"
  local temporary
  temporary="$(mktemp "${env_file}.tmp.XXXXXX")"
  awk -F= -v key="${name}" -v replacement="${value}" '
    BEGIN { replaced = 0 }
    $1 == key { print key "=" replacement; replaced = 1; next }
    { print }
    END { if (!replaced) print key "=" replacement }
  ' "${env_file}" >"${temporary}"
  chmod --reference="${env_file}" "${temporary}"
  if [[ "$(id -u)" -eq 0 ]]; then
    chown --reference="${env_file}" "${temporary}"
  fi
  mv -- "${temporary}" "${env_file}"
}

image="$(env_file_value PYRORISK_IMAGE)"
if [[ "${image}" == \"*\" || "${image}" == \'*\' ]]; then
  image="${image:1:${#image}-2}"
fi
: "${image:?PYRORISK_IMAGE is missing from deploy/oracle/.env}"
docker build \
  --build-arg "FIRESIFT_GIT_COMMIT=${revision}" \
  --build-arg "OCI_REVISION=${revision}" \
  --build-arg "OCI_CREATED=${created_at}" \
  --tag "${image}" \
  "${remote_dir}"
image_digest="$(docker image inspect "${image}" --format '{{.Id}}')"
set_env_file_value ERYTHEON_GIT_REVISION "${revision}"
set_env_file_value ERYTHEON_IMAGE_REFERENCE "${image}"
set_env_file_value ERYTHEON_IMAGE_DIGEST "${image_digest}"
docker compose --env-file "${env_file}" -f compose.yml up -d --force-recreate app
docker compose --env-file "${env_file}" -f compose.yml up -d caddy
docker image prune --force
docker compose --env-file "${env_file}" -f compose.yml ps
REMOTE
