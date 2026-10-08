#!/bin/sh
# PostgreSQL archive_command helper: encrypted immutable WAL objects, never plaintext off-host.
set -eu
umask 077
if [ "$#" -ne 2 ]; then exit 2; fi
source_file=$1
wal_name=$2
if ! printf '%s' "$wal_name" | LC_ALL=C grep -Eq '^([A-F0-9]{24}|[A-F0-9]{8}\.history|[A-F0-9]{24}\.[A-F0-9]{8}\.backup)$'; then exit 2; fi
: "${WAL_ARCHIVE_DIRECTORY:?Set an independent encrypted archive directory}"
: "${AGE_RECIPIENT:?Set an age public recipient}"
age_binary=${AGE_BINARY:-age}
mkdir -p "$WAL_ARCHIVE_DIRECTORY"
target_directory=$WAL_ARCHIVE_DIRECTORY/$wal_name
source_hash=$(sha256sum "$source_file" | cut -d ' ' -f 1)
if [ -d "$target_directory" ]; then
    test "$(cat "$target_directory/plaintext.sha256")" = "$source_hash"
    (cd "$target_directory" && sha256sum --check ciphertext.sha256 >/dev/null)
    exit 0
fi
lock_directory=$WAL_ARCHIVE_DIRECTORY/.$wal_name.lock
if ! mkdir "$lock_directory" 2>/dev/null; then exit 1; fi
temporary_file=$WAL_ARCHIVE_DIRECTORY/.$wal_name.$$.tmp
trap 'rm -f "$temporary_file"; rm -rf "$lock_directory"' EXIT HUP INT TERM
if [ -d "$target_directory" ]; then exit 1; fi
"$age_binary" --encrypt --recipient "$AGE_RECIPIENT" --output "$temporary_file" "$source_file"
chmod 600 "$temporary_file"
mv "$temporary_file" "$lock_directory/wal.age"
printf '%s\n' "$source_hash" > "$lock_directory/plaintext.sha256"
(cd "$lock_directory" && sha256sum wal.age > ciphertext.sha256)
sync -f "$lock_directory/wal.age"
sync -f "$lock_directory"
mv "$lock_directory" "$target_directory"
sync -f "$WAL_ARCHIVE_DIRECTORY"
