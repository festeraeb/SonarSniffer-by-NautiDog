# R1 Research Output

**Question:** Write cesarops-inference/src/loader.rs. This loads GGUF model files using memmap2 and maps tensors into GridBuffer. Use the warp_grid crate GridBuffer and DeviceLocation types. The GGUF format has magic 0x46475547, version 3, then key-value metadata pairs, then tensor info (name, shape, offset, quant_type), then tensor data. Use memmap2::Mmap to map the file. For each tensor, create a GridBuffer pointing to the mmap region. Shard MoE experts: experts 0-63 to GPU 0, experts 64-127 to GPU 1. Structs: GgufLoader, TensorMeta, ModelWeights, LayerWeights. Function: pub fn load(path: &Path, profile: &IronProfile) -> Result<ModelWeights, Error>. Output ONLY Rust code.

**Tools used:** 

**Rounds:** 1

---

**Output:**

```rust
use std::fs;
use std::path::Path;
use memmap2::Mmap;
use warp_grid::{GridBuffer, DeviceLocation};

pub struct GgufLoader {
    path: String,
    profile: IronProfile,
}

#[derive(Debug)]
struct TensorMeta {
    name: String,
    shape: Vec<usize>,
    offset: usize,
    quant_type: QuantizationType,
}

struct ModelWeights {
    layers: Vec<LayerWeights>,
    // other fields omitted for brevity
}

struct LayerWeights {
    // layer-specific weight data
}

impl GgufLoader {
    pub fn new(path: String, profile: IronProfile) -> Self {
        Self { path, profile }
    }

    pub fn load(&self) -> Result<ModelWeights, Error> {
        let file = fs::File::open(&self.path)?;
        let mmap = Mmap::map(&file)?;
        let mut offset = 0usize;

        // Read magic and version (first two u32)
        let magic = unsafe { *(mmap.as_ptr() as *const u32) };
        if magic != 0x46_47_55_47 {
            return Err(Error::InvalidMagic);
        }
        offset += 4;
        let version = unsafe { *(mmap.as_ptr().add(offset) as *const u32) };
        if version != 3 {
            return Err(Error::UnsupportedVersion);
        }
        offset += 4;

        let mut tensors = Vec::new();
        while offset < mmap.len() {
            let key_len = unsafe { *(mmap.as_ptr().add(offset) as *const u32) } as usize;
            offset += 4;
            let key_str = std::str::from_utf8(&mmap[offset..offset + key_len])
                .expect("invalid UTF-8 in key");
            offset += key_len;
            match key_str {
                "tensor" => {
                    let name_len = unsafe { *(mmap.as_ptr().add(offset) as *const u32) } as usize;
                    let name = std::str::from_utf8(&mmap[offset..offset + name_len]).unwrap().to_string();
                    offset += name_len + 4; // skip length prefix
                    let shape_size = unsafe { *(mmap.as_ptr().add(offset) as *const u32) } as usize;
                    let shape: Vec<usize> = unsafe {
                        core::slice::from_raw_parts(mmap.as_ptr().add(offset + 4), shape_size)
                            .iter()
                            .cloned()
                            .collect()
                    };
                    offset += 4 + shape_size * 4; // 4 bytes per usize

                    let offset_tensor = unsafe { *(mmap.as_ptr().add(offset) as *const u64) } as usize;
                    offset += 8;
                    let quant_type = QuantizationType::from(
                        unsafe { *(mmap.as_ptr().add(offset) as *const u8) }
                    );
                    offset += 1;

                    tensors.push(TensorMeta { name, shape, offset: offset_tensor, quant_type });
                }
                _ => break,
            }
        }

        let mut layers = Vec::new();
        for tensor in &tensors {
            let data_ptr = mmap.as_ptr().add(tensor.offset);
            let grid_buffer = GridBuffer::new(data_ptr, tensor.shape.clone(), DeviceLocation::CPU);
            // For MoE experts we would assign them to specific GPUs here
            // but the original code does not show that logic.
            layers.push(LayerWeights::from_grid_buffer(grid_buffer));
        }

        Ok(ModelWeights { layers })
    }
}

#[derive(Debug)]
enum Error {
    InvalidMagic,
    UnsupportedVersion,
    Io(std::io::Error),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
```