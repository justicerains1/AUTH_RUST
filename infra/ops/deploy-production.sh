#!/bin/sh
# Production host: Docker/Compose + Node only; never installs dependencies or builds code.
set -eu
script_directory=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
exec node "$script_directory/deploy-production.mjs" "$@"
