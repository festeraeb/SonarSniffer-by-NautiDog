#!/usr/bin/env bash
# Skip bf16 WMMA MoE kernels on Pascal/Turing (sm_60/61/75) — they do not compile.
set -euo pipefail

CAP="${1:-}"
if [[ -z "$CAP" ]]; then
  echo "usage: $0 <compute_cap>   e.g. 61 75" >&2
  exit 1
fi

if [[ "$CAP" != "61" && "$CAP" != "75" && "$CAP" != "60" ]]; then
  exit 0
fi

BUILD_RS=$(find "${CARGO_HOME:-$HOME/.cargo}/registry/src" -path '*/candle-kernels-*/build.rs' 2>/dev/null | sort -V | tail -1)
if [[ -z "$BUILD_RS" || ! -f "$BUILD_RS" ]]; then
  echo "[patch_candle] candle-kernels build.rs not found" >&2
  exit 1
fi

BUILD_RS_PATCHED=0
if grep -q 'fn pascal_ptx_kernels' "$BUILD_RS" 2>/dev/null; then
  echo "[patch_candle] build.rs already patched: $BUILD_RS"
  BUILD_RS_PATCHED=1
fi

if [[ "$BUILD_RS_PATCHED" == "0" ]]; then
python3 <<PY
from pathlib import Path
path = Path("$BUILD_RS")
text = path.read_text()

helper = '''
fn pascal_ptx_kernels() -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap_or_else(|_| panic!("read_dir {:?}", dir)) {
            let entry = entry.expect("dir entry");
            let p = entry.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().map(|e| e == "cu").unwrap_or(false) {
                let s = p.to_string_lossy();
                if !s.contains("moe_wmma") {
                    out.push(p);
                }
            }
        }
    }
    walk(std::path::Path::new("src"), &mut out);
    out
}
'''

old_ptx = '''    let builder = bindgen_cuda::Builder::default()
        .arg("--expt-relaxed-constexpr")
        .arg("-std=c++17")
        .arg("-O3");
    let bindings = builder.build_ptx().unwrap();'''

new_ptx = '''    println!("cargo:rerun-if-env-changed=CANDLE_PASCAL_MOE_GGUF_ONLY");
    let mut builder = bindgen_cuda::Builder::default()
        .arg("--expt-relaxed-constexpr")
        .arg("-std=c++17")
        .arg("-O3");
    if std::env::var("CANDLE_PASCAL_MOE_GGUF_ONLY").is_ok() {
        builder = builder.kernel_paths(pascal_ptx_kernels());
    }
    let bindings = builder.build_ptx().unwrap();'''

old_moe = '''    let moe_builder = moe_builder.kernel_paths(vec![
        "src/moe/moe_gguf.cu",
        "src/moe/moe_wmma.cu",
        "src/moe/moe_wmma_gguf.cu",
    ]);'''

new_moe = '''    // CANDLE_PASCAL_MOE_GGUF_ONLY: WMMA MoE kernels fail on sm_60/61/75
    let moe_kernels = if std::env::var("CANDLE_PASCAL_MOE_GGUF_ONLY").is_ok() {
        vec!["src/moe/moe_gguf.cu"]
    } else {
        vec![
            "src/moe/moe_gguf.cu",
            "src/moe/moe_wmma.cu",
            "src/moe/moe_wmma_gguf.cu",
        ]
    };
    let moe_builder = moe_builder.kernel_paths(moe_kernels);'''

if 'fn pascal_ptx_kernels' not in text:
    text = text.replace('fn main() {', helper + '\nfn main() {', 1)

if old_ptx in text:
    text = text.replace(old_ptx, new_ptx, 1)
elif 'pascal_ptx_kernels()' in text:
    pass
else:
    raise SystemExit(f"PTX patch anchor missing in {path}")

if old_moe in text:
    text = text.replace(old_moe, new_moe, 1)
elif 'CANDLE_PASCAL_MOE_GGUF_ONLY' in text and 'moe_kernels' in text:
    pass
else:
    raise SystemExit(f"MOE patch anchor missing in {path}")

path.write_text(text)
print(f"[patch_candle] patched {path}")
PY
fi

# Pascal (sm_60/61): enable __half atomicAdd shim used by reduce.cu
if [[ "$CAP" == "60" || "$CAP" == "61" ]]; then
  COMPAT=$(dirname "$BUILD_RS")/src/compatibility.cuh
  if [[ -f "$COMPAT" ]] && ! grep -q 'CANDLE_PASCAL_HALF_ATOMIC' "$COMPAT"; then
    python3 <<PY
from pathlib import Path
path = Path("$COMPAT")
text = path.read_text()
old = '''#if __CUDA_ARCH__ < 700
// https://docs.nvidia.com/cuda/cuda-c-programming-guide/index.html#atomicadd
// The 16-bit __half floating-point version of atomicAdd() is only supported by devices of compute capability 7.x and higher.
// Solution adapted from https://github.com/torch/cutorch/blob/master/lib/THC/THCAtomics.cuh#L96-L119
//__device__ __half atomicAdd(__half *address, __half val) {
   //  unsigned int *address_as_ui = (unsigned int *) ((char *)address - ((size_t)address & 2));
   //  unsigned int old = *address_as_ui;
   //  unsigned int assumed;
   //  bool unaligned = (size_t) address & 2;
   //  do {
   //      assumed = old;
   //      unsigned int hsum;
   //      hsum = unaligned ? (old >> 16) : (old & 0xffff);
   //      hsum = __half_as_ushort(__ushort_as_half(hsum) + val); 
   //      old = atomicCAS(address_as_ui, assumed,
   //          unaligned ? (old & 0xffff) | (hsum << 16) : (old & 0xffff0000) | hsum
   //      );

   // } while (assumed != old);
   // return __ushort_as_half(unaligned ? (old >> 16) : (old & 0xffff));
//}
#endif'''
new = '''#if __CUDA_ARCH__ < 700
// CANDLE_PASCAL_HALF_ATOMIC — sm_60/61 lack native __half atomicAdd (see CUDA programming guide).
__device__ __half atomicAdd(__half *address, __half val) {
    unsigned int *address_as_ui = (unsigned int *)((char *)address - ((size_t)address & 2));
    unsigned int old = *address_as_ui;
    unsigned int assumed;
    bool unaligned = ((size_t)address & 2);
    do {
        assumed = old;
        unsigned int hsum = unaligned ? (old >> 16) : (old & 0xffff);
        __half tmp = __hadd(__ushort_as_half(hsum), val);
        hsum = __half_as_ushort(tmp);
        old = atomicCAS(address_as_ui, assumed,
            unaligned ? (old & 0xffff) | (hsum << 16) : (old & 0xffff0000) | hsum);
    } while (assumed != old);
    return __ushort_as_half(unaligned ? (old >> 16) : (old & 0xffff));
}
#endif'''
if old not in text:
    raise SystemExit(f"compatibility patch anchor missing in {path}")
path.write_text(text.replace(old, new, 1))
print(f"[patch_candle] patched {path}")
PY
  fi
fi
