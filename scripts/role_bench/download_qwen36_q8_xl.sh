#!/usr/bin/env bash
# Largest practical Qwen3.6-35B-A3B GGUF for quality vs Q4 (T440 CPU or P100+fit).
set -euo pipefail

DEST="${DEST:-/mnt/t440/models/Qwen3.6-35B-A3B-UD-Q8_K_XL.gguf}"
EXPECTED="${EXPECTED:-38450000000}"
URL="${URL:-https://huggingface.co/unsloth/Qwen3.6-35B-A3B-GGUF/resolve/main/Qwen3.6-35B-A3B-UD-Q8_K_XL.gguf}"
LOG="${LOG:-/data/codebase/repos/wreckhunter2000-1/scripts/role_bench/var/role_bench/qwen_q8_xl_download.log}"

mkdir -p "$(dirname "$DEST")" "$(dirname "$LOG")"

if [[ -f "$DEST" ]]; then
  SZ=$(stat -c%s "$DEST")
  if [[ "$SZ" -ge "$EXPECTED" ]]; then
    echo "already complete: $DEST ($SZ bytes)" | tee -a "$LOG"
    exit 0
  fi
fi

echo "downloading $URL -> $DEST" | tee -a "$LOG"
wget -c --progress=dot:giga -O "$DEST" "$URL" >>"$LOG" 2>&1
SZ=$(stat -c%s "$DEST")
echo "done size=$SZ" | tee -a "$LOG"
