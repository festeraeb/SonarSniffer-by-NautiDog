#!/usr/bin/env bash
# Fire the H/I/J/K/L/M batch to the cluster in one go.
#
# Usage:
#   ./fleet_prompts/dispatch_batch_HIJKLM.sh <endpoint>
#
# Example:
#   ./fleet_prompts/dispatch_batch_HIJKLM.sh http://10.0.0.41:5001
#
# Each prompt fires sequentially against the same endpoint. Responses
# land at fleet_prompts/{H,I,J,K,L,M}_response.md.
#
# After all six complete, run conductor.sh per task to extract code +
# build-check + auto-fix loop.

set -e

ENDPOINT="$1"
if [ -z "$ENDPOINT" ]; then
    echo "Usage: $0 <endpoint>"
    echo "Example: $0 http://10.0.0.41:5001"
    exit 1
fi

DISPATCH="$(dirname "$0")/dispatch.sh"

declare -A TASKS=(
    [H]="H_diagnostic_gate_and_ubo_pool.md"
    [I]="I_kv_prefix_cache.md"
    [J]="J_speculative_verify_correct.md"
    [K]="K_q6k_native_dispatch_unblock.md"
    [L]="L_dual_p100_layer_split.md"
    [M]="M_web_compute_mvp.md"
)

# Sequential dispatch — same endpoint can't handle parallel requests
# without queue serialization. Each call blocks until response arrives.
for TASK in H I J K L M; do
    PROMPT="$(dirname "$0")/${TASKS[$TASK]}"
    OUT="$(dirname "$0")/${TASK}_response.md"

    if [ ! -f "$PROMPT" ]; then
        echo "MISSING: $PROMPT — skipping"
        continue
    fi

    echo "═══════════════════════════════════════════════════════════"
    echo "  TASK $TASK → $ENDPOINT"
    echo "  prompt: $PROMPT"
    echo "  out:    $OUT"
    echo "═══════════════════════════════════════════════════════════"

    bash "$DISPATCH" "$ENDPOINT" "$PROMPT" "$OUT" || {
        echo "DISPATCH FAILED for $TASK — continuing"
    }

    # Brief pause to avoid hammering the endpoint
    sleep 2
done

echo ""
echo "Batch complete. Run conductor on each task to extract+build:"
for TASK in H I J K L M; do
    echo "  ./fleet_prompts/conductor.sh $TASK $ENDPOINT fleet_prompts/${TASKS[$TASK]}"
done
