#!/bin/sh
# PG* credentials must come from an owner-only PGPASSFILE, never command arguments or stdout.
set -eu
umask 077
: "${BACKUP_DIRECTORY:?Set independent backup storage}"
: "${AGE_RECIPIENT:?Set an age public recipient}"
: "${PGPASSFILE:?Set a controlled PostgreSQL password file}"
age_binary=${AGE_BINARY:-age}
backup_id=$(date -u +%Y%m%dT%H%M%SZ)-$$
temporary_directory=$(mktemp -d)
trap 'rm -rf "$temporary_directory"' EXIT HUP INT TERM
mkdir -p "$BACKUP_DIRECTORY"
pg_basebackup --pgdata "$temporary_directory/base" --format=plain --wal-method=stream --checkpoint=fast --no-password
pg_verifybackup "$temporary_directory/base"
tar -C "$temporary_directory" -cf "$temporary_directory/base.tar" base
"$age_binary" --encrypt --recipient "$AGE_RECIPIENT" --output "$BACKUP_DIRECTORY/$backup_id.tar.age" "$temporary_directory/base.tar"
sha256sum "$BACKUP_DIRECTORY/$backup_id.tar.age" > "$BACKUP_DIRECTORY/$backup_id.tar.age.sha256"
printf '%s\n' "base backup complete: $backup_id"
