#!/bin/sh
# PG* credentials must come from an owner-only PGPASSFILE, never command arguments or stdout.
set -eu
umask 077
BACKUP_DIRECTORY=${BACKUP_DIRECTORY:-${BACKUP_DESTINATION:+$BACKUP_DESTINATION/base}}
: "${BACKUP_DIRECTORY:?Set independent backup storage}"
: "${AGE_RECIPIENT:?Set an age public recipient}"
: "${PGPASSFILE:?Set a controlled PostgreSQL password file}"
age_binary=${AGE_BINARY:-age}
backup_id=$(date -u +%Y%m%dT%H%M%SZ)-$$
temporary_directory=$(mktemp -d)
publish_directory=
trap 'rm -rf "$temporary_directory" "$publish_directory"' EXIT HUP INT TERM
mkdir -p "$BACKUP_DIRECTORY"
publish_directory=$(mktemp -d "$BACKUP_DIRECTORY/.base-backup.XXXXXX")
pg_basebackup --pgdata "$temporary_directory/base" --format=plain --wal-method=stream --checkpoint=fast --no-password
pg_verifybackup "$temporary_directory/base"
tar -C "$temporary_directory" -cf "$temporary_directory/base.tar" base
backup_name=$backup_id.tar.age
"$age_binary" --encrypt --recipient "$AGE_RECIPIENT" --output "$publish_directory/$backup_name" "$temporary_directory/base.tar"
(cd "$publish_directory" && sha256sum "$backup_name" > "$backup_name.sha256")
sync -f "$publish_directory/$backup_name"
sync -f "$publish_directory/$backup_name.sha256"
test ! -e "$BACKUP_DIRECTORY/$backup_name"
test ! -e "$BACKUP_DIRECTORY/$backup_name.sha256"
mv "$publish_directory/$backup_name" "$BACKUP_DIRECTORY/$backup_name"
# Publish the checksum last: a ciphertext without its manifest is not a complete backup.
mv "$publish_directory/$backup_name.sha256" "$BACKUP_DIRECTORY/$backup_name.sha256"
sync -f "$BACKUP_DIRECTORY"
# A trusted completion record is published only after verification, encryption and both objects are durable.
# Monitoring checks this record and the selected ciphertext; file mtime never means backup success.
completed_at=$(date -u +%s)
ciphertext_bytes=$(wc -c < "$BACKUP_DIRECTORY/$backup_name")
ciphertext_sha256=$(cut -d ' ' -f 1 "$BACKUP_DIRECTORY/$backup_name.sha256")
printf '{"version":1,"completed_at":%s,"backup_name":"%s","ciphertext_bytes":%s,"ciphertext_sha256":"%s"}\n' \
    "$completed_at" "$backup_name" "$ciphertext_bytes" "$ciphertext_sha256" > "$publish_directory/last-success.json"
sync -f "$publish_directory/last-success.json"
mv -T "$publish_directory/last-success.json" "$BACKUP_DIRECTORY/last-success.json"
sync -f "$BACKUP_DIRECTORY"
printf '%s\n' "base backup complete: $backup_id"
