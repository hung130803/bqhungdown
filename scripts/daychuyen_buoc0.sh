#!/bin/bash
# BƯỚC 0 của Dây chuyền: chạy MỌI cổng test, ghi ra 1 file để so với nền.
# Dùng: bash scripts/daychuyen_buoc0.sh docs/daychuyen_baseline_<ten>.txt
# Cổng: cargo test --lib · cargo clippy --lib (deny chống panic của daychuyen.rs,
# exit != 0 là ĐỎ) · tsc · vitest · bot unittest · test-live-filter (đỏ sẵn từ
# trước vì was_live — chỉ cần GIỐNG nền).
OUT="$1"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
: > "$OUT"
run() { echo "===== $1 =====" >> "$OUT"; echo "(cwd: $2)" >> "$OUT"; (cd "$2" && eval "$1") >> "$OUT" 2>&1; echo "===== exit=$? =====" >> "$OUT"; echo >> "$OUT"; }
echo "Baseline chay luc $(date -Iseconds) tren nhanh $(cd "$ROOT" && git rev-parse --abbrev-ref HEAD) @ $(cd "$ROOT" && git rev-parse --short HEAD)" >> "$OUT"
run "cargo test --lib 2>&1" "$ROOT/src-tauri"
run "cargo clippy --lib --message-format=short 2>&1 | grep -E 'daychuyen\.rs|^error' ; test \${PIPESTATUS[0]} -eq 0" "$ROOT/src-tauri"
run "npx tsc -b" "$ROOT"
run "npx vitest run" "$ROOT"
run "python -m unittest discover -s tests" "$ROOT/telegram-bot"
run "python scripts/test-live-filter.py" "$ROOT"
echo DONE >> "$OUT"
