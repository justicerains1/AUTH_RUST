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
restore_directory=
trap 'rm -rf "$temporary_directory" "$restore_directory"' EXIT HUP INT TERM
backup_name=$(basename "$backup_file")
case "$backup_name" in
    ''|*[!A-Za-z0-9._-]*) echo 'invalid backup filename' >&2; exit 2 ;;
esac
manifest=$backup_file.sha256
if [ ! -f "$manifest" ] || [ "$(wc -l < "$manifest")" -ne 1 ]; then
    echo 'backup checksum requires one relative-basename record' >&2; exit 2
fi
checksum_line=$(cat "$manifest")
expected_hash=${checksum_line%% *}
if ! printf '%s' "$expected_hash" | LC_ALL=C grep -Eq '^[a-f0-9]{64}$' ||
    [ "$checksum_line" != "$expected_hash  $backup_name" ]; then
    echo 'backup checksum must name only this relative basename; legacy manifests require controlled conversion' >&2
    exit 2
fi
# Never follow a path from the manifest, including an old path that still exists.
actual_hash=$(sha256sum < "$backup_file" | cut -d ' ' -f 1)
if [ "$actual_hash" != "$expected_hash" ]; then
    echo 'selected backup checksum mismatch' >&2; exit 1
fi
"$age_binary" --decrypt --identity "$AGE_IDENTITY_FILE" --output "$temporary_directory/base.tar" "$backup_file"
restore_directory=$(mktemp -d "$(dirname "$destination")/.base-restore.XXXXXX")
tar -C "$restore_directory" --strip-components=1 -xf "$temporary_directory/base.tar"
pg_verifybackup "$restore_directory"
# Publish only verified data on the destination filesystem; never replace a raced-in destination.
mv -T -n "$restore_directory" "$destination"
if [ -d "$restore_directory" ]; then echo 'restore destination already exists' >&2; exit 2; fi
printf '%s\n' 'base restored and verified; configure recovery target and restore_command before startup'
