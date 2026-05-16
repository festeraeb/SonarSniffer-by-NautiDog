 ```rust
use std::collections::HashMap;
use std::fs::{create_dir_all, File};
use std::hash::{Hash, Hasher};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, AtomicUsize};
use std::sync::atomic::Ordering;

pub struct PipelineCache {
    dir: PathBuf,
    hits: AtomicUsize,
    misses: AtomicUsize,
    map: Mutex<HashMap<(u64, String), Arc<wgpu::ComputePipeline>>>,
}

impl PipelineCache {
    pub fn new(cache_dir: impl Into<PathBuf>) -> io::Result<Self> {
        let dir = cache_dir.into();
        create_dir_all(&dir)?;
        Ok(PipelineCache {
            dir,
            hits: AtomicUsize::new(0),
            misses: AtomicUsize::new(0),
            map: Mutex::new(HashMap::new()),
        })
    }

    pub fn get_or_create_compute_pipeline(
        &self,
        device: &wgpu::Device,
        label: &str,
        shader_source: &str,
        entry_point: &str,
        layout: Option<&wgpu::PipelineLayout>,
    ) -> Arc<wgpu::ComputePipeline> {
        let key = (self.hash_shader(shader_source), entry_point.to_string());
        if let Some(pipeline) = self.map.lock().unwrap().get(&key) {
            self.hits.fetch_add(1, Ordering::Relaxed);
            return pipeline.clone();
        }

        self.misses.fetch_add(1, Ordering::Relaxed);
        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(shader_source.into()),
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(label),
            layout: layout.cloned(),
            module: &shader_module,
            entry_point,
        });

        let pipeline = Arc::new(pipeline);
        self.map.lock().unwrap().insert(key, pipeline.clone());

        let file_path = self.dir.join(format!("{}_{}.wgsl", key.0, key.1));
        if let Err(e) = File::create(&file_path).and_then(|mut f| f.write_all(shader_source.as_bytes())) {
            eprintln!("Failed to write shader source to disk: {}", e);
        }

        pipeline
    }

    pub fn stats(&self) -> (usize, usize) {
        (
            self.hits.load(Ordering::Relaxed),
            self.misses.load(Ordering::Relaxed),
        )
    }

    fn hash_shader(&self, shader_source: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        shader_source.hash(&mut hasher);
        hasher.finish()
    }
}
``` ```rust
use std::collections::HashMap;
use std::fs::{create_dir_all, File};
use std::hash::{Hash, Hasher};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, AtomicUsize};
use std::sync::atomic::Ordering;

pub struct PipelineCache {
    dir: PathBuf,
    hits: AtomicUsize,
    misses: AtomicUsize,
    map: Mutex<HashMap<(u64, String), Arc<wgpu::ComputePipeline>>>,
}

impl PipelineCache {
    pub fn new(cache_dir: impl Into<PathBuf>) -> io::Result<Self> {
        let dir = cache_dir.into();
        create_dir_all(&dir)?;
        Ok(PipelineCache {
            dir,
            hits: AtomicUsize::new(0),
            misses: AtomicUsize::new(0),
            map: Mutex::new(HashMap::new()),
        })
    }

    pub fn get_or_create_compute_pipeline(
        &self,
        device: &wgpu::Device,
        label: &str,
        shader_source: &str,
        entry_point: &str,
        layout: Option<&wgpu::PipelineLayout>,
    ) -> Arc<wgpu::ComputePipeline> {
        let key = (self.hash_shader(shader_source), entry_point.to_string());
        if let Some(pipeline) = self.map.lock().unwrap().get(&key) {
            self.hits.fetch_add(1, Ordering::Relaxed);
            return pipeline.clone();
        }

        self.misses.fetch_add(1, Ordering::Relaxed);
        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(shader_source.into()),
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(label),
            layout: layout.cloned(),
            module: &shader_module,
            entry_point,
        });

        let pipeline = Arc::new(pipeline);
        self.map.lock().unwrap().insert(key, pipeline.clone());

        let file_path = self.dir.join(format!("{}_{}.wgsl", key.0, key.1));
        if let Err(e) = File::create(&file_path).and_then(|mut f| f.write_all(shader_source.as_bytes())) {
            eprintln!("Failed to write shader source to disk: {}", e);
        }

        pipeline
    }

    pub fn stats(&self) -> (usize, usize) {
        (
            self.hits.load(Ordering::Relaxed),
            self.misses.load(Ordering::Relaxed),
        )
    }

    fn hash_shader(&self, shader_source: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        shader_source.hash(&mut hasher);
        hasher.finish()
    }
}
``` ```rust
use std::collections::HashMap;
use std::fs::{create_dir_all, File};
use std::hash::{Hash, Hasher};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, AtomicUsize};
use std::sync::atomic::Ordering;

pub struct PipelineCache {
    dir: PathBuf,
    hits: AtomicUsize,
    misses: AtomicUsize,
    map: Mutex<HashMap<(u64, String), Arc<wgpu::ComputePipeline>>>,
}

impl PipelineCache {
    pub fn new(cache_dir: impl Into<PathBuf>) -> io::Result<Self> {
        let dir = cache_dir.into();
        create_dir_all(&dir)?;
        Ok(PipelineCache {
            dir,
            hits: AtomicUsize::new(0),
            misses: AtomicUsize::new(0),
            map: Mutex::new(HashMap::new()),
        })
    }

    pub fn get_or_create_compute_pipeline(
        &self,
        device: &wgpu::Device,
        label: &str,
        shader_source: &str,
        entry_point: &str,
        layout: Option<&wgpu::PipelineLayout>,
    ) -> Arc<wgpu::ComputePipeline> {
        let key = (self.hash_shader(shader_source), entry_point.to_string());
        if let Some(pipeline) = self.map.lock().unwrap().get(&key) {
            self.hits.fetch_add(1, Ordering::Relaxed);
            return pipeline.clone();
        }

        self.misses.fetch_add(1, Ordering::Relaxed);
        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(shader_source.into()),
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(label),
            layout: layout.cloned(),
            module: &shader_module,
            entry_point,
        });

        let pipeline = Arc::new(pipeline);
        self.map.lock().unwrap().insert(key, pipeline.clone());

        let file_path = self.dir.join(format!("{}_{}.wgsl", key.0, key.1));
        if let Err(e) = File::create(&file_path).and_then(|mut f| f.write_all(shader_source.as_bytes())) {
            eprintln!("Failed to write shader source to disk: {}", e);
        }

        pipeline
    }

    pub fn stats(&self) -> (usize, usize) {
        (
            self.hits.load(Ordering::Relaxed),
            self.misses.load(Ordering::Relaxed),
        )
    }

    fn hash_shader(&self, shader_source: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        shader_source.hash(&mut hasher);
        hasher.finish()
    }
}
``` ```rust
use std::collections::HashMap;
use std::fs::{create_dir_all, File};
use std::hash::{Hash, Hasher};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, AtomicUsize};
use std::sync::atomic::Ordering;

pub struct PipelineCache {
    dir: PathBuf,
    hits: AtomicUsize,
    misses: AtomicUsize,
    map: Mutex<HashMap<(u64, String), Arc<wgpu::ComputePipeline>>>,
}

impl PipelineCache {
    pub fn new(cache_dir: impl Into<PathBuf>) -> io::Result<Self> {
        let dir = cache_dir.into();
        create_dir_all(&dir)?;
        Ok(PipelineCache {
            dir,
            hits: AtomicUsize::new(0),
            misses: AtomicUsize::new(0),
            map: Mutex::new(HashMap::new()),
        })
    }

    pub fn get_or_create_compute_pipeline(
        &self,
        device: &wgpu::Device,
        label: &str,
        shader_source: &str,
        entry_point: &str,

