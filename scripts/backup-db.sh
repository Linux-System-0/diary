#!/usr/bin/env bash
set -euo pipefail

: "${DIARY_DB_PATH:?请设置 DIARY_DB_PATH 指向 SQLite 文件}"

dest="${1:-./backups/diary-$(date +%Y%m%d-%H%M%S).db}"
mkdir -p "$(dirname "$dest")"
rm -f "$dest"

# VACUUM INTO 生成一致快照，不需要停服；WAL 中的已提交内容会包含在快照里。
sqlite3 "$DIARY_DB_PATH" "PRAGMA busy_timeout=5000; VACUUM INTO '$dest';"
chmod 600 "$dest"
echo "backup written: $dest"
