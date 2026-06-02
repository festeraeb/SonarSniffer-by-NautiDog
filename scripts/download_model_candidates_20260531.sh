#!/usr/bin/env bash
set -euo pipefail

OUT_DIR="${1:-/data/models/candidates/20260531}"
mkdir -p "$OUT_DIR"

# Candidate set tuned for this host (2x8GB + 1x6GB GPUs, 78GB RAM)
# Roles:
# - thinker/general: Qwen3-8B-Q4_K_M
# - coder: Qwen2.5-Coder-7B-Instruct-Q4_K_M
# - reviewer/reasoning: DeepSeek-R1-Distill-Qwen-14B-IQ4_XS
# - reviewer-alt: gemma-2-9b-it-IQ4_XS
# - fast helper/tooling: Phi-4-mini-instruct-Q4_K_M

cat > "$OUT_DIR/manifest.tsv" <<'EOF'
role	model	filename	url	bytes
coder	Qwen2.5-Coder-7B-Instruct	qwen2.5-coder-7b-instruct-q4_k_m.gguf	https://huggingface.co/Qwen/Qwen2.5-Coder-7B-Instruct-GGUF/resolve/main/qwen2.5-coder-7b-instruct-q4_k_m.gguf	4683073536
thinker	Qwen3-8B	Qwen3-8B-Q4_K_M.gguf	https://huggingface.co/Qwen/Qwen3-8B-GGUF/resolve/main/Qwen3-8B-Q4_K_M.gguf	5027783488
reviewer	DeepSeek-R1-Distill-Qwen-14B	DeepSeek-R1-Distill-Qwen-14B-IQ4_XS.gguf	https://huggingface.co/bartowski/DeepSeek-R1-Distill-Qwen-14B-GGUF/resolve/main/DeepSeek-R1-Distill-Qwen-14B-IQ4_XS.gguf	8119840160
reviewer-alt	gemma-2-9b-it	gemma-2-9b-it-IQ4_XS.gguf	https://huggingface.co/bartowski/gemma-2-9b-it-GGUF/resolve/main/gemma-2-9b-it-IQ4_XS.gguf	5183030208
fast-helper	Phi-4-mini-instruct	Phi-4-mini-instruct.Q4_K_M.gguf	https://huggingface.co/MaziyarPanahi/Phi-4-mini-instruct-GGUF/resolve/main/Phi-4-mini-instruct.Q4_K_M.gguf	2491874624
EOF

awk -F '\t' 'NR>1{sum+=$5} END{printf("planned_total_bytes=%d\nplanned_total_gib=%.2f\n", sum, sum/1024/1024/1024)}' "$OUT_DIR/manifest.tsv"

while IFS=$'\t' read -r role model filename url bytes; do
  [[ "$role" == "role" ]] && continue
  echo "[download] role=$role model=$model file=$filename"
  wget -c -O "$OUT_DIR/$filename" "$url"
done < "$OUT_DIR/manifest.tsv"

echo "[done] model candidate downloads completed at $OUT_DIR"
ls -lh "$OUT_DIR" | cat
