//! Shader Compiler — Runtime WGSL shader loading and pipeline management.
//!
//! Bridges the shaders/ directory to wgpu compute pipelines. Supports:
//! - Loading shaders from filesystem (hot-reload during development)
//! - Embedded shaders via include_str! (production builds)
//! - Shader variant selection based on GPU capabilities (f16 vs f32)
//! - Pipeline caching to avoid recompilation

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{info, warn};

/// A compiled shader pipeline ready for dispatch.
pub struct CompiledShader {
    pub name: String,
    pub pipeline: wgpu::ComputePipeline,
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub workgroup_size: [u32; 3],
    pub source_path: Option<PathBuf>,
}

/// Shader variant selection based on hardware capabilities.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShaderVariant {
    /// f32 only — works on all Vulkan GPUs (Maxwell, Pascal, AMD, Intel)
    F32,
    /// f16 (half precision) — 2x throughput on Pascal P100, requires SHADER_F16 feature
    F16,
    /// Tiled with shared memory — optimized for large matmuls
    TiledF32,
}

/// The shader compiler and pipeline cache.
pub struct ShaderCompiler {
    device: Arc<wgpu::Device>,
    /// Cached compiled pipelines: (shader_name, variant) → CompiledShader
    cache: HashMap<(String, ShaderVariant), Arc<CompiledShader>>,
    /// Base directory for shader source files
    shader_dir: PathBuf,
    /// Whether the device supports f16 shaders
    supports_f16: bool,
}

impl ShaderCompiler {
    /// Create a new shader compiler for the given device.
    pub fn new(device: Arc<wgpu::Device>, shader_dir: PathBuf) -> Self {
        let supports_f16 = device.features().contains(wgpu::Features::SHADER_F16);
        info!("ShaderCompiler: f16 support = {}", supports_f16);

        Self {
            device,
            cache: HashMap::new(),
            shader_dir,
            supports_f16,
        }
    }

    /// Get or compile a shader pipeline. Returns cached version if available.
    pub fn get_or_compile(
        &mut self,
        name: &str,
        variant: ShaderVariant,
        bind_group_layout_desc: &wgpu::BindGroupLayoutDescriptor,
    ) -> Result<Arc<CompiledShader>, String> {
        let key = (name.to_string(), variant);

        if let Some(cached) = self.cache.get(&key) {
            return Ok(Arc::clone(cached));
        }

        // Determine which variant to actually use
        let effective_variant = if variant == ShaderVariant::F16 && !self.supports_f16 {
            warn!("Requested f16 shader but device doesn't support SHADER_F16. Falling back to f32.");
            ShaderVariant::F32
        } else {
            variant
        };

        let source = self.load_shader_source(name, effective_variant)?;
        let compiled = self.compile(&source, name, bind_group_layout_desc)?;
        let compiled = Arc::new(compiled);
        self.cache.insert(key, Arc::clone(&compiled));

        Ok(compiled)
    }

    /// Load shader source from filesystem or embedded fallback.
    fn load_shader_source(&self, name: &str, variant: ShaderVariant) -> Result<String, String> {
        let filename = match variant {
            ShaderVariant::F32 => format!("{}_f32.wgsl", name),
            ShaderVariant::F16 => format!("{}_half2.wgsl", name),
            ShaderVariant::TiledF32 => format!("{}_tiled.wgsl", name),
        };

        let path = self.shader_dir.join(&filename);

        // Try filesystem first (allows hot-reload during development)
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(source) => {
                    info!("Loaded shader from filesystem: {}", path.display());
                    return Ok(source);
                }
                Err(e) => {
                    warn!("Failed to read shader file {}: {}", path.display(), e);
                }
            }
        }

        // Fallback to embedded shaders (production)
        match name {
            "matmul" => match variant {
                ShaderVariant::F32 => Ok(include_str!("../../shaders/matmul_f32.wgsl").to_string()),
                ShaderVariant::F16 => Ok(include_str!("../../shaders/matmul_half2.wgsl").to_string()),
                _ => Err(format!("No embedded shader for {}_{:?}", name, variant)),
            },
            "geo_filter" => Ok(include_str!("../../shaders/geo_filter.wgsl").to_string()),
            _ => Err(format!("Shader '{}' not found at {} and no embedded fallback", name, path.display())),
        }
    }

    /// Compile WGSL source into a compute pipeline.
    fn compile(
        &self,
        source: &str,
        name: &str,
        bind_group_layout_desc: &wgpu::BindGroupLayoutDescriptor,
    ) -> Result<CompiledShader, String> {
        let module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(name),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });

        let bind_group_layout = self.device.create_bind_group_layout(bind_group_layout_desc);

        let pipeline_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(&format!("{}_layout", name)),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(&format!("{}_pipeline", name)),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        // Parse workgroup size from source (look for @workgroup_size annotation)
        let workgroup_size = parse_workgroup_size(source);

        info!("Compiled shader '{}': workgroup_size={:?}", name, workgroup_size);

        Ok(CompiledShader {
            name: name.to_string(),
            pipeline,
            bind_group_layout,
            workgroup_size,
            source_path: Some(self.shader_dir.join(format!("{}.wgsl", name))),
        })
    }

    /// Invalidate cache for a specific shader (for hot-reload).
    pub fn invalidate(&mut self, name: &str) {
        self.cache.retain(|(n, _), _| n != name);
        info!("Invalidated shader cache for '{}'", name);
    }

    /// Invalidate all cached shaders.
    pub fn invalidate_all(&mut self) {
        self.cache.clear();
        info!("Invalidated all shader caches");
    }

    /// Get the best variant for the current hardware.
    pub fn best_variant(&self) -> ShaderVariant {
        if self.supports_f16 {
            ShaderVariant::F16
        } else {
            ShaderVariant::F32
        }
    }

    /// Check if a shader file has been modified since last compilation.
    pub fn is_stale(&self, name: &str, variant: ShaderVariant) -> bool {
        let key = (name.to_string(), variant);
        if let Some(cached) = self.cache.get(&key) {
            if let Some(ref path) = cached.source_path {
                if let Ok(metadata) = std::fs::metadata(path) {
                    if let Ok(modified) = metadata.modified() {
                        // Compare against a stored compile time (simplified: always stale if file exists)
                        let _ = modified;
                        return true; // TODO: store compile timestamp for proper staleness check
                    }
                }
            }
        }
        false
    }
}

/// Parse @workgroup_size(x, y, z) from WGSL source.
fn parse_workgroup_size(source: &str) -> [u32; 3] {
    // Look for @workgroup_size(X, Y, Z) or @workgroup_size(X, Y) or @workgroup_size(X)
    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(start) = trimmed.find("@workgroup_size(") {
            let after = &trimmed[start + 16..];
            if let Some(end) = after.find(')') {
                let params = &after[..end];
                let parts: Vec<u32> = params.split(',')
                    .map(|s| s.trim().parse().unwrap_or(1))
                    .collect();
                return [
                    *parts.first().unwrap_or(&1),
                    *parts.get(1).unwrap_or(&1),
                    *parts.get(2).unwrap_or(&1),
                ];
            }
        }
    }
    [1, 1, 1] // default if not found
}

/// Calculate dispatch dimensions for a given output size and workgroup size.
pub fn dispatch_size(output_width: usize, output_height: usize, workgroup: [u32; 3]) -> [u32; 3] {
    [
        ((output_width as u32 + workgroup[0] - 1) / workgroup[0]),
        ((output_height as u32 + workgroup[1] - 1) / workgroup[1]),
        1,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_workgroup_size() {
        assert_eq!(parse_workgroup_size("@workgroup_size(16, 16, 1)"), [16, 16, 1]);
        assert_eq!(parse_workgroup_size("@workgroup_size(8, 8)"), [8, 8, 1]);
        assert_eq!(parse_workgroup_size("@workgroup_size(256)"), [256, 1, 1]);
        assert_eq!(parse_workgroup_size("no annotation here"), [1, 1, 1]);
    }

    #[test]
    fn test_dispatch_size() {
        assert_eq!(dispatch_size(256, 256, [16, 16, 1]), [16, 16, 1]);
        assert_eq!(dispatch_size(257, 100, [16, 16, 1]), [17, 7, 1]);
        assert_eq!(dispatch_size(1, 1, [8, 8, 1]), [1, 1, 1]);
    }
}
