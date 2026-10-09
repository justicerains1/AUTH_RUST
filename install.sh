#!/usr/bin/env bash
# Download one script and run it interactively. CI images only; no production compilation.
set -Eeuo pipefail
umask 077
REPOSITORY=justicerains1/AUTH_RUST
NODE_VERSION=22.22.1
NODE_SHA256=9a6bc82f9b491279147219f6a18add1e18424dce90d41d2a5fcd69d4924ba3aa
INSTALL_ROOT=/srv/auth-rust
RELEASE_TAG=
case "${1:-}" in
  --help|-h)
    cat <<'EOF'
CDNGOD production installer (Debian/Ubuntu x86_64).
Usage: sudo bash install.sh [--release ci-<40-character-commit>]
Requires interactive terminal, real HTTPS domains, SMTP and mounted backup storage.
Downloads tested CI images; never runs cargo/npm/docker build.
Existing installations retain data and keys and use the verified release upgrade path.
EOF
    exit 0 ;;
  --release)
    [[ $# == 2 && "$2" =~ ^ci-[a-f0-9]{40}$ ]] || { echo 'Invalid release tag.' >&2; exit 2; }
    RELEASE_TAG=$2 ;;
  '') [[ $# == 0 ]] || exit 2 ;;
  *) echo 'Use --help or --release ci-<SHA>.' >&2; exit 2 ;;
esac
[[ $(id -u) == 0 ]] || { echo 'Run sudo bash install.sh.' >&2; exit 1; }
[[ -t 0 && -t 1 ]] || { echo 'Interactive terminal required. Download the script, then run it; do not pipe it into bash.' >&2; exit 1; }
[[ $(uname -m) == x86_64 && $(uname -s) == Linux ]] || { echo 'This CI artifact supports Linux x86_64 only.' >&2; exit 1; }
[[ -f /etc/os-release ]] || exit 1
# shellcheck disable=SC1091
source /etc/os-release
[[ "$ID" == ubuntu || "$ID" == debian ]] || { echo 'Supported systems: Debian and Ubuntu.' >&2; exit 1; }
printf 'CDNGOD: install Docker/Compose, CI-built Caddy and verified Node runtime. Domains are entered interactively; existing Nginx will be checked before switching web ports.\n'
read -r -p 'Proceed? [y/N]: ' answer
[[ "$answer" == y || "$answer" == Y ]] || exit 0
apt-get update -qq
apt-get install -y --no-install-recommends ca-certificates curl gnupg jq xz-utils openssl util-linux tar
temporary=$(mktemp -d)
trap 'rm -rf "$temporary"; unset GH_TOKEN' EXIT
if ! command -v docker >/dev/null || ! docker compose version >/dev/null 2>&1; then
  install -d -m 0755 /etc/apt/keyrings
  curl --fail --silent --show-error --location "https://download.docker.com/linux/$ID/gpg" -o "$temporary/docker.asc"
  fingerprint=$(gpg --show-keys --with-colons "$temporary/docker.asc" | awk -F: '$1=="fpr" {print $10; exit}')
  [[ "$fingerprint" == 9DC858229FC7DD38854AE2D88D81803C0EBFCD88 ]] || { echo 'Docker repository signing key mismatch.' >&2; exit 1; }
  install -m 0644 "$temporary/docker.asc" /etc/apt/keyrings/docker.asc
  printf 'deb [arch=amd64 signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/%s %s stable\n' "$ID" "${VERSION_CODENAME:?}" > /etc/apt/sources.list.d/auth-rust-docker.list
  apt-get update -qq
  apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
fi
systemctl enable --now docker
docker info >/dev/null
if ! command -v node >/dev/null || [[ $(node --version) != v$NODE_VERSION ]]; then
  curl --fail --silent --show-error --location --retry 3 "https://nodejs.org/dist/v$NODE_VERSION/node-v$NODE_VERSION-linux-x64.tar.xz" -o "$temporary/node.tar.xz"
  printf '%s  %s\n' "$NODE_SHA256" "$temporary/node.tar.xz" | sha256sum --check
  install -d /opt/auth-rust-node
  tar -xJf "$temporary/node.tar.xz" -C /opt/auth-rust-node --strip-components=1
  ln -sfn /opt/auth-rust-node/bin/node /usr/local/bin/node
fi
export PATH=/usr/local/bin:/usr/bin:/bin
read -r -p "Install directory [$INSTALL_ROOT]: " directory_input
INSTALL_ROOT=${directory_input:-$INSTALL_ROOT}
[[ "$INSTALL_ROOT" =~ ^/[a-zA-Z0-9_./-]+$ && "$INSTALL_ROOT" != / && "$INSTALL_ROOT" != *'/../'* ]] || { echo 'Use an absolute path without spaces or traversal.' >&2; exit 1; }
mkdir -p "$INSTALL_ROOT"
[[ $(realpath "$INSTALL_ROOT") == "$INSTALL_ROOT" ]] || { echo 'Install path cannot contain symbolic links.' >&2; exit 1; }
exec 9>"$INSTALL_ROOT/.installer.lock"
flock -n 9 || { echo 'Another installation is running.' >&2; exit 1; }
read -r -s -p 'GitHub token (Enter for public downloads; token may need read:packages): ' GH_TOKEN
printf '\n'
curl_auth=()
if [[ -n "$GH_TOKEN" ]]; then
  # Keep tokens out of process arguments and logs. curl reads an owner-only config.
  [[ "$GH_TOKEN" =~ ^[a-zA-Z0-9_]+$ ]] || { echo 'Invalid GitHub token characters.' >&2; exit 1; }
  printf 'header = "Authorization: Bearer %s"\n' "$GH_TOKEN" > "$temporary/curl-auth"
  curl_auth=(--config "$temporary/curl-auth")
fi
curl --fail --silent --show-error --location "${curl_auth[@]}" -H 'Accept: application/vnd.github+json' "https://api.github.com/repos/$REPOSITORY/releases?per_page=100" -o "$temporary/releases.json"
if [[ -z "$RELEASE_TAG" ]]; then
  RELEASE_TAG=$(jq -r '[.[] | select(.draft==false and (.tag_name|test("^ci-[a-f0-9]{40}$"))) | select(any(.assets[]; .name=="auth-rust-deploy-linux-amd64.tar.gz"))][0].tag_name // empty' "$temporary/releases.json")
fi
[[ "$RELEASE_TAG" =~ ^ci-[a-f0-9]{40}$ ]] || { echo 'No completed CI deployment release exists yet. Wait for the publishing job; nothing will be built on this server.' >&2; exit 1; }
printf 'Selected completed CI release: %s\n' "$RELEASE_TAG"
for name in auth-rust-deploy-linux-amd64.tar.gz auth-rust-deploy-linux-amd64.tar.gz.sha256; do
  asset=$(jq -r --arg tag "$RELEASE_TAG" --arg name "$name" '.[]|select(.tag_name==$tag)|.assets[]|select(.name==$name)|.url' "$temporary/releases.json")
  [[ "$asset" =~ ^https://api.github.com/repos/justicerains1/AUTH_RUST/releases/assets/[0-9]+$ ]] || { echo 'Completed release asset missing.' >&2; exit 1; }
  curl --fail --silent --show-error --location --retry 3 "${curl_auth[@]}" -H 'Accept: application/octet-stream' "$asset" -o "$temporary/$name"
done
(cd "$temporary"; [[ $(cat auth-rust-deploy-linux-amd64.tar.gz.sha256) =~ ^[a-f0-9]{64}[[:space:]][[:space:]]auth-rust-deploy-linux-amd64.tar.gz$ ]]; sha256sum --check auth-rust-deploy-linux-amd64.tar.gz.sha256)
# Refuse traversal, links and special files before extraction as root.
tar -tzf "$temporary/auth-rust-deploy-linux-amd64.tar.gz" | awk '/(^\/|(^|\/)\.\.($|\/))/ {bad=1} END {exit bad}'
tar -tvzf "$temporary/auth-rust-deploy-linux-amd64.tar.gz" | awk 'substr($0,1,1)!="-" && substr($0,1,1)!="d" {bad=1} END {exit bad}'
mkdir "$temporary/bundle"
tar -xzf "$temporary/auth-rust-deploy-linux-amd64.tar.gz" -C "$temporary/bundle" --no-same-owner
node --input-type=module - "$temporary/bundle" "${RELEASE_TAG#ci-}" <<'EOF'
import {pathToFileURL} from 'node:url';
const root=process.argv[2];const {validateBundle}=await import(pathToFileURL(root+'/infra/ops/deploy-production.mjs'));
const manifest=await validateBundle(root);if(manifest.revision!==process.argv[3])throw Error('Downloaded revision does not match release');
EOF
if [[ -n "$GH_TOKEN" ]]; then
  read -r -p 'GitHub username for GHCR: ' registry_user
  printf '%s' "$GH_TOKEN" | docker login ghcr.io -u "$registry_user" --password-stdin
fi
unset GH_TOKEN
# Only CI deployment files are updated. Existing secrets, state and volumes are preserved.
node --input-type=module - "$temporary/bundle" "$INSTALL_ROOT" <<'EOF'
import {readFile,mkdir,copyFile,lstat} from 'node:fs/promises';import{dirname,join}from'node:path';
const [source,target]=process.argv.slice(2);const m=JSON.parse(await readFile(join(source,'release-manifest.json')));
for(const f of [...m.files,{path:'release-manifest.json'}]){let parent=target;for(const component of f.path.split('/').slice(0,-1)){parent=join(parent,component);try{const info=await lstat(parent);if(!info.isDirectory()||info.isSymbolicLink())throw Error('Unsafe installation directory');}catch(e){if(e.code!=='ENOENT')throw e;await mkdir(parent,{mode:0o700});}}const dest=join(target,f.path);try{const info=await lstat(dest);if(!info.isFile()||info.isSymbolicLink())throw Error('Unsafe installation file');}catch(e){if(e.code!=='ENOENT')throw e;}await copyFile(join(source,f.path),dest);}
EOF
rm -rf "$temporary"
trap - EXIT
exec node "$INSTALL_ROOT/infra/ops/install-production.mjs" "$INSTALL_ROOT"
