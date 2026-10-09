#!/bin/sh
# PostgreSQL archive_command: pre-mounted encrypted storage, immutable receipts, no credential logs.
set -eu
umask 077
[ "$#" -eq 2 ] || exit 2
source_file=$1
wal_name=$2
printf '%s' "$wal_name" | LC_ALL=C grep -Eq '^([A-F0-9]{24}|[A-F0-9]{8}\.history|[A-F0-9]{24}\.[A-F0-9]{8}\.backup)$' || exit 2
WAL_ARCHIVE_DIRECTORY=${WAL_ARCHIVE_DIRECTORY:-${BACKUP_DESTINATION:+$BACKUP_DESTINATION/wal}}
: "${WAL_ARCHIVE_DIRECTORY:?Pre-mounted independent archive required}"
: "${WAL_ARCHIVE_DEVICE:?Explicit verified archive filesystem device required}"
: "${AGE_RECIPIENT:?Public age recipient required}"
age_binary=${AGE_BINARY:-age}
printf '%s' "$WAL_ARCHIVE_DEVICE" | LC_ALL=C grep -Eq '^[0-9]+$' || exit 2
controlled_directory() {
    [ -d "$1" ] && [ ! -L "$1" ] && [ "$(realpath -e "$1")" = "$1" ] &&
    [ "$(stat -c %u "$1")" = "$(id -u)" ] && [ "$((0$(stat -c %a "$1") & 022))" -eq 0 ]
}
archive_root() { controlled_directory "$WAL_ARCHIVE_DIRECTORY" && [ "$(stat -c %d "$WAL_ARCHIVE_DIRECTORY")" = "$WAL_ARCHIVE_DEVICE" ]; }
controlled_file() { [ -f "$1" ] && [ ! -L "$1" ] && [ "$(stat -c %h "$1")" -eq 1 ] && [ "$(stat -c %u "$1")" = "$(id -u)" ] && [ "$((0$(stat -c %a "$1") & 077))" -eq 0 ]; }
# Missing/unmounted roots are failures. Never create a fallback directory and mark a segment archived.
archive_root || exit 1
[ -f "$source_file" ] && [ ! -L "$source_file" ] || exit 1
source_hash=$(sha256sum < "$source_file" | cut -d ' ' -f 1)
target_directory=$WAL_ARCHIVE_DIRECTORY/$wal_name
lock_directory=$WAL_ARCHIVE_DIRECTORY/.archive.lock
mkdir "$lock_directory" 2>/dev/null || exit 1
staging_directory=$lock_directory/object
receipt_temporary=$lock_directory/pointer.json
trap 'rm -rf "$lock_directory"' EXIT HUP INT TERM
verify_object() {
    controlled_directory "$1" || return 1
    [ "$(stat -c %d "$1")" = "$WAL_ARCHIVE_DEVICE" ] || return 1
    for file in wal.age plaintext.sha256 ciphertext.sha256; do controlled_file "$1/$file" || return 1; done
    [ "$(wc -l < "$1/plaintext.sha256")" -eq 1 ] && [ "$(cat "$1/plaintext.sha256")" = "$source_hash" ] || return 1
    object_hash=$(sha256sum < "$1/wal.age" | cut -d ' ' -f 1)
    [ "$(wc -l < "$1/ciphertext.sha256")" -eq 1 ] && [ "$(cat "$1/ciphertext.sha256")" = "$object_hash  wal.age" ] || return 1
}
verify_receipt() {
    controlled_file "$1" && [ "$(wc -l < "$1")" -eq 1 ] || return 1
    LC_ALL=C grep -Eq '^\{"version":1,"wal_name":"[A-F0-9]{24}","started_at":[1-9][0-9]{0,9},"completed_at":[1-9][0-9]{0,9},"duration_seconds":[0-9]{1,10},"ciphertext_bytes":[1-9][0-9]{0,15},"ciphertext_sha256":"[a-f0-9]{64}"\}$' "$1" || return 1
    receipt_name=$(sed -n 's/.*"wal_name":"\([A-F0-9]*\)".*/\1/p' "$1")
    receipt_start=$(sed -n 's/.*"started_at":\([0-9]*\),.*/\1/p' "$1")
    receipt_end=$(sed -n 's/.*"completed_at":\([0-9]*\),.*/\1/p' "$1")
    receipt_duration=$(sed -n 's/.*"duration_seconds":\([0-9]*\),.*/\1/p' "$1")
    receipt_bytes=$(sed -n 's/.*"ciphertext_bytes":\([0-9]*\),.*/\1/p' "$1")
    receipt_hash=$(sed -n 's/.*"ciphertext_sha256":"\([a-f0-9]*\)".*/\1/p' "$1")
    [ "$receipt_name" = "$2" ] && [ "$receipt_start" -le "$receipt_end" ] && [ "$receipt_duration" -eq "$((receipt_end-receipt_start))" ] && [ "$receipt_end" -le "$(($(date -u +%s)+60))" ] || return 1
    [ "$receipt_bytes" -eq "$(stat -c %s "$3/wal.age")" ] && [ "$receipt_hash  wal.age" = "$(cat "$3/ciphertext.sha256")" ] || return 1
}
durable_object() {
    controlled_directory "$1" && [ "$(stat -c %d "$1")" = "$WAL_ARCHIVE_DEVICE" ] || return 1
    for file in wal.age plaintext.sha256 ciphertext.sha256; do
        controlled_file "$1/$file" && sync -f "$1/$file" || return 1
    done
    if [ -e "$1/completion.json" ] || [ -L "$1/completion.json" ]; then
        controlled_file "$1/completion.json" && sync -f "$1/completion.json" || return 1
    fi
    sync -f "$1" && archive_root && sync -f "$WAL_ARCHIVE_DIRECTORY"
}
if [ -e "$target_directory" ] || [ -L "$target_directory" ]; then
    verify_object "$target_directory" || exit 1
    # Existence after a failed rename/root sync is not proof of durability.
    durable_object "$target_directory" || exit 1
    # A legacy verified object may be retried, but never receives invented fresh completion metadata.
    if [ ! -e "$target_directory/completion.json" ] && [ ! -L "$target_directory/completion.json" ]; then exit 0; fi
else
    started_at=$(date -u +%s)
    mkdir "$staging_directory"
    "$age_binary" --encrypt --recipient "$AGE_RECIPIENT" --output "$staging_directory/wal.age" "$source_file"
    printf '%s\n' "$source_hash" > "$staging_directory/plaintext.sha256"
    object_hash=$(sha256sum < "$staging_directory/wal.age" | cut -d ' ' -f 1)
    printf '%s  wal.age\n' "$object_hash" > "$staging_directory/ciphertext.sha256"
    [ "$(sha256sum < "$source_file" | cut -d ' ' -f 1)" = "$source_hash" ] || exit 1
    for file in wal.age plaintext.sha256 ciphertext.sha256; do sync -f "$staging_directory/$file"; done
    sync -f "$staging_directory"
    completed_at=$(date -u +%s)
    [ "$completed_at" -ge "$started_at" ] || exit 1
    ciphertext_bytes=$(stat -c %s "$staging_directory/wal.age")
    printf '{"version":1,"wal_name":"%s","started_at":%s,"completed_at":%s,"duration_seconds":%s,"ciphertext_bytes":%s,"ciphertext_sha256":"%s"}\n' "$wal_name" "$started_at" "$completed_at" "$((completed_at-started_at))" "$ciphertext_bytes" "$object_hash" > "$staging_directory/completion.json"
    sync -f "$staging_directory/completion.json"
    sync -f "$staging_directory"
    archive_root || exit 1
    mv -T "$staging_directory" "$target_directory"
    sync -f "$WAL_ARCHIVE_DIRECTORY"
fi
# Timeline history/backup labels remain immutable, but do not advance full WAL segment freshness.
printf '%s' "$wal_name" | LC_ALL=C grep -Eq '^[A-F0-9]{24}$' || exit 0
verify_receipt "$target_directory/completion.json" "$wal_name" "$target_directory" || exit 1
completed_at=$receipt_end
if [ -e "$WAL_ARCHIVE_DIRECTORY/last-success.json" ] || [ -L "$WAL_ARCHIVE_DIRECTORY/last-success.json" ]; then
    controlled_file "$WAL_ARCHIVE_DIRECTORY/last-success.json" || exit 1
    previous_name=$(sed -n 's/.*"wal_name":"\([A-F0-9]*\)".*/\1/p' "$WAL_ARCHIVE_DIRECTORY/last-success.json")
    printf '%s' "$previous_name" | LC_ALL=C grep -Eq '^[A-F0-9]{24}$' || exit 1
    controlled_directory "$WAL_ARCHIVE_DIRECTORY/$previous_name" || exit 1
    [ "$(stat -c %d "$WAL_ARCHIVE_DIRECTORY/$previous_name")" = "$WAL_ARCHIVE_DEVICE" ] || exit 1
    for file in wal.age ciphertext.sha256 completion.json; do controlled_file "$WAL_ARCHIVE_DIRECTORY/$previous_name/$file" || exit 1; done
    [ "$(wc -l < "$WAL_ARCHIVE_DIRECTORY/$previous_name/ciphertext.sha256")" -eq 1 ] || exit 1
    [ "$(sha256sum < "$WAL_ARCHIVE_DIRECTORY/$previous_name/wal.age" | cut -d ' ' -f 1)  wal.age" = "$(cat "$WAL_ARCHIVE_DIRECTORY/$previous_name/ciphertext.sha256")" ] || exit 1
    # Only validate pointer fields here; the collector verifies the selected object's SHA independently.
    verify_receipt "$WAL_ARCHIVE_DIRECTORY/last-success.json" "$previous_name" "$WAL_ARCHIVE_DIRECTORY/$previous_name" || exit 1
    cmp -s "$WAL_ARCHIVE_DIRECTORY/last-success.json" "$WAL_ARCHIVE_DIRECTORY/$previous_name/completion.json" || exit 1
    previous_at=$receipt_end
    # A prior pointer rename may have succeeded before its root sync failed.
    durable_object "$WAL_ARCHIVE_DIRECTORY/$previous_name" || exit 1
    sync -f "$WAL_ARCHIVE_DIRECTORY/last-success.json" || exit 1
    archive_root && sync -f "$WAL_ARCHIVE_DIRECTORY" || exit 1
    if [ "$previous_at" -gt "$completed_at" ]; then exit 0; fi
    if [ "$previous_at" -eq "$completed_at" ] && [ "$(printf '%s\n%s\n' "$previous_name" "$wal_name" | LC_ALL=C sort | tail -n 1)" = "$previous_name" ]; then exit 0; fi
fi
cp "$target_directory/completion.json" "$receipt_temporary"
sync -f "$receipt_temporary"
archive_root || exit 1
mv -T "$receipt_temporary" "$WAL_ARCHIVE_DIRECTORY/last-success.json"
sync -f "$WAL_ARCHIVE_DIRECTORY"
