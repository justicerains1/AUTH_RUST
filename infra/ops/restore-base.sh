#!/bin/sh
# Restores only into a new empty directory. Starting PostgreSQL remains an explicit separate step.
set -eu
umask 077
if [ "$#" -ne 2 ]; then exit 2; fi
backup_file=$1
destination=$2
: "${AGE_IDENTITY_FILE:?Set the separately controlled backup identity}"
age_binary=${AGE_BINARY:-age}
if [ -e "$destination" ]; then echo 'restore requires a new empty destination' >&2; exit 2; fi
temporary_directory=$(mktemp -d)
trap 'rm -rf "$temporary_directory"' EXIT HUP INT TERM
sha256sum --check "$backup_file.sha256"
"$age_binary" --decrypt --identity "$AGE_IDENTITY_FILE" --output "$temporary_directory/base.tar" "$backup_file"
mkdir -m 700 "$destination"
tar -C "$destination" --strip-components=1 -xf "$temporary_directory/base.tar"
pg_verifybackup "$destination"
printf '%s\n' 'base restored and verified; configure recovery target and restore_command before startup'
