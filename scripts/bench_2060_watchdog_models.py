#!/usr/bin/env python3
import json
import subprocess
import time
from pathlib import Path

import requests

HOST = "10.0.0.201"
PORT = 5200
BASE = f"http://{HOST}:{PORT}"
SSH = ["ssh", "cesarops@10.0.0.201"]
LLAMA = "/home/cesarops/src/llama.cpp/build/bin/llama-server"
MODELS_DIR = "/mnt/t440/models"
OUT = Path("/codebase/repos/wreckhunter2000-1/integrate_out/bench_2060")
OUT.mkdir(parents=True, exist_ok=True)

# <=8GB plus one turbo offload stretch
MODEL_SET = [
    "TinyLlama-1.1B-Chat-v1.0-Q4_K_M.gguf",
    "qwen2.5-coder-1.5b-instruct-q6_k.gguf",
    "Phi-3-mini-4k-instruct-Q4_K_M.gguf",
    "DeepSeek-R1-Distill-Qwen-7B-Uncensored.Q4_K_M.gguf",
    "Qwen3.5-9B-DeepSeek-V4-Flash-MTP-Q4_K_M.gguf",
    "gemma-4-E4B-it-Q4_K_M.gguf",
    # stretch case requested
    "Qwen3.6-35B-A3B-MXFP4_MOE.gguf",
]


def run(cmd: str, timeout: int = 120):
    return subprocess.run(cmd, shell=True, text=True, capture_output=True, timeout=timeout)


def ssh_script(script: str, timeout: int = 240):
    return subprocess.run(
        SSH + ["bash", "-s"],
        input=script,
        text=True,
        capture_output=True,
        timeout=timeout,
    )


def restart_model(model_name: str) -> tuple[bool, str]:
    model = f"{MODELS_DIR}/{model_name}"
    reasoning = "off"
    extra = "-ngl 99 -fa off -ctk f16 -ctv f16 -ub 384"
    if "35B-A3B-MXFP4_MOE" in model_name:
        # offload fit for stretch run on 2060
        extra = "--fit on --fit-target 7680 -sm layer -fa auto -ctk q8_0 -ctv q8_0 -ub 384 --no-mmap"
    remote = f"""#!/usr/bin/env bash
set -euo pipefail
pkill -f "llama-server.*--port {PORT}" 2>/dev/null || true
pkill -f "llama-server.*-port {PORT}" 2>/dev/null || true
fuser -k {PORT}/tcp 2>/dev/null || true
sleep 1
[[ -f "{model}" ]] || (echo "missing:{model}" && exit 3)
CUDA_VISIBLE_DEVICES=0 setsid "{LLAMA}" -m "{model}" --host 0.0.0.0 --port {PORT} -dev CUDA0 {extra} -c 4096 -t 4 --reasoning {reasoning} -np 1 </dev/null >>/tmp/bench-2060-{PORT}.log 2>&1 &
echo "started:{model_name}"
"""
    r = ssh_script(remote, timeout=120)
    if r.returncode != 0:
        return False, (r.stderr or r.stdout).strip()
    return True, (r.stdout or "").strip()


def wait_ready(max_wait=240) -> bool:
    deadline = time.time() + max_wait
    while time.time() < deadline:
        try:
            rr = requests.get(f"{BASE}/v1/models", timeout=4)
            if rr.ok:
                return True
        except Exception:
            pass
        time.sleep(3)
    return False


def ask(messages, max_tokens=220, temperature=0.2):
    t0 = time.time()
    r = requests.post(
        f"{BASE}/v1/chat/completions",
        json={
            "messages": messages,
            "max_tokens": max_tokens,
            "temperature": temperature,
        },
        timeout=90,
    )
    elapsed = time.time() - t0
    if not r.ok:
        return {"ok": False, "elapsed_s": elapsed, "error": r.text[:600]}
    data = r.json()
    ch = (data.get("choices") or [{}])[0].get("message", {})
    txt = (ch.get("content") or "").strip()
    if not txt and ch.get("reasoning_content"):
        txt = ch["reasoning_content"].strip()
    return {
        "ok": True,
        "elapsed_s": elapsed,
        "text": txt,
        "timings": data.get("timings", {}),
        "usage": data.get("usage", {}),
    }


def score_code_output(text: str) -> int:
    s = 0
    if "```rust" in text:
        s += 2
    if "fn " in text:
        s += 2
    if "#[test]" in text:
        s += 2
    if "Result<" in text or "Option<" in text:
        s += 1
    if "TODO" not in text and len(text) > 120:
        s += 1
    return s


def score_tool_correction(text: str) -> int:
    s = 0
    needles = ["match", "None =>", "Some(", "Err(", "Ok(", "trim()", "parse::<f64>"]
    for n in needles:
        if n in text:
            s += 1
    if "```rust" in text:
        s += 2
    return s


def bench_one(model: str):
    ok, note = restart_model(model)
    row = {"model": model, "boot_ok": ok, "boot_note": note}
    if not ok:
        return row
    if not wait_ready():
        row["boot_ok"] = False
        row["boot_note"] = "timeout waiting for /v1/models"
        return row

    code_prompt = [
        {"role": "system", "content": "You are a concise Rust coding assistant."},
        {
            "role": "user",
            "content": "Write a Rust function `parse_bbox(s: &str) -> Result<(f64,f64,f64,f64), String>` and 2 unit tests.",
        },
    ]
    c = ask(code_prompt, max_tokens=280, temperature=0.1)
    row["code_ok"] = c.get("ok", False)
    row["code_elapsed_s"] = round(c.get("elapsed_s", 0), 2)
    row["code_score"] = score_code_output(c.get("text", "")) if c.get("ok") else 0
    row["code_preview"] = (c.get("text", "") or c.get("error", ""))[:280]

    tool_prompt = [
        {"role": "system", "content": "You are a corrective coding assistant. Use tool context."},
        {
            "role": "user",
            "content": (
                "Task: fix this Rust parser bug.\n"
                "Broken code:\n"
                "```rust\n"
                "fn parse_lat(x: &str) -> f64 { x.parse::<f64>().unwrap() }\n"
                "```\n\n"
                "[TOOL CONTEXT]\n"
                "- read_file: parser.rs has panic on invalid numeric input.\n"
                "- run_tests: failing test expects Err(\"invalid latitude\") on bad input.\n"
                "- style: avoid unwrap in parser path.\n\n"
                "Return corrected Rust code only."
            ),
        },
    ]
    t = ask(tool_prompt, max_tokens=260, temperature=0.1)
    row["tool_ok"] = t.get("ok", False)
    row["tool_elapsed_s"] = round(t.get("elapsed_s", 2), 2)
    row["tool_score"] = score_tool_correction(t.get("text", "")) if t.get("ok") else 0
    row["tool_preview"] = (t.get("text", "") or t.get("error", ""))[:280]
    row["total_score"] = row["code_score"] + row["tool_score"]
    return row


def main():
    rows = []
    for m in MODEL_SET:
        print(f"\n=== {m} ===", flush=True)
        try:
            row = bench_one(m)
        except Exception as e:
            row = {"model": m, "boot_ok": False, "boot_note": f"exception: {e}"}
        rows.append(row)
        print(json.dumps(row, indent=2), flush=True)

    rows_sorted = sorted(rows, key=lambda r: r.get("total_score", -1), reverse=True)
    report = {
        "generated_at": time.strftime("%Y-%m-%d %H:%M:%S"),
        "host": HOST,
        "port": PORT,
        "results": rows,
        "ranking": [
            {
                "model": r["model"],
                "total_score": r.get("total_score", 0),
                "code_elapsed_s": r.get("code_elapsed_s"),
                "tool_elapsed_s": r.get("tool_elapsed_s"),
            }
            for r in rows_sorted
        ],
    }
    out_json = OUT / "bench_2060_small_models.json"
    out_md = OUT / "bench_2060_small_models.md"
    out_json.write_text(json.dumps(report, indent=2))

    lines = [
        "# 2060 Small Model Benchmark",
        "",
        f"- Host: `{HOST}:{PORT}`",
        "- Scope: <=8GB models + turbo-MoE offload stretch",
        "",
        "| Model | Score | Code s | Tool s | Boot |",
        "|---|---:|---:|---:|---|",
    ]
    for r in rows_sorted:
        lines.append(
            f"| `{r['model']}` | {r.get('total_score',0)} | {r.get('code_elapsed_s','-')} | {r.get('tool_elapsed_s','-')} | {'ok' if r.get('boot_ok') else 'fail'} |"
        )
    out_md.write_text("\n".join(lines) + "\n")
    print(f"\nWrote:\n- {out_json}\n- {out_md}")


if __name__ == "__main__":
    main()
