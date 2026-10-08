#!/bin/sh
set -eu
umask 077
if [ "$#" -ne 2 ]; then exit 2; fi
wal_name=$1
destination=$2
if ! printf '%s' "$wal_name" | LC_ALL=C grep -Eq '^([A-F0-9]{24}|[A-F0-9]{8}\.history|[A-F0-9]{24}\.[A-F0-9]{8}\.backup)$'; then exit 2; fi
: "${WAL_ARCHIVE_DIRECTORY:?Set the encrypted archive directory}"
: "${AGE_IDENTITY_FILE:?Set the separately controlled age identity file}"
age_binary=${AGE_BINARY:-age}
archive_directory=$WAL_ARCHIVE_DIRECTORY/$wal_name
(cd "$archive_directory" && sha256sum --check ciphertext.sha256 >/dev/null)
temporary_file=$destination.$$.tmp
trap 'rm -f "$temporary_file"' EXIT HUP INT TERM
"$age_binary" --decrypt --identity "$AGE_IDENTITY_FILE" --output "$temporary_file" "$archive_directory/wal.age"
test "$(sha256sum "$temporary_file" | cut -d ' ' -f 1)" = "$(cat "$archive_directory/plaintext.sha256")"
mv "$temporary_file" "$destination"
