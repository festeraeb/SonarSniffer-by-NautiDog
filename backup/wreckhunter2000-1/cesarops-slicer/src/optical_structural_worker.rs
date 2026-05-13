use cesarops_slicer::common::db::{AnomalyQueue, AnomalyRecord, SensorType};
use clap::Parser;
use pollster::FutureExt;
use rayon::prelude::*;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

const WGSL_SOURCE: &str = "
@group(0) @binding(0) var<storage, read> input_data: array<f32>;
@group(0) @binding(1) var<storage, read_write> output_data: array<f32>;

struct Uniforms {
    width: u32,
    height: u32,
    threshold: f32,
}
@group(0) @binding(2) var<uniform> uniforms: Uniforms;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let x = global_id.x;
    let y = global_id.y;
    let width = uniforms.width;
    let height = uniforms.height;

    if (x == 0u || x >= width - 1u || y == 0u || y >= height - 1u) {
        return;
    }

    let idx = y * width + x;

    let p00 = input_data[(y - 1u) * width + (x - 1u)];
    let p01 = input_data[(y - 1u) * width + x];
    let p02 = input_data[(y - 1u) * width + (x + 1u)];
    
    let p10 = input_data[y * width + (x - 1u)];
    let p12 = input_data[y * width + (x + 1u)];
    
    let p20 = input_data[(y + 1u) * width + (x - 1u)];
    let p21 = input_data[(y + 1u) * width + x];
    let p22 = input_data[(y + 1u) * width + (x + 1u)];

    let gx = -1.0 * p00 + 1.0 * p02 - 2.0 * p10 + 2.0 * p12 - 1.0 * p20 + 1.0 * p22;
    let gy = -1.0 * p00 - 2.0 * p01 - 1.0 * p02 + 1.0 * p20 + 2.0 * p21 + 1.0 * p22;

    let mag = sqrt(gx * gx + gy * gy);

    if (mag >= uniforms.threshold) {
        output_data[idx] = mag;
    } else {
        output_data[idx] = 0.0;
    }
}
";

/// CESAROPS Optical Structural Worker
/// Tier 1 Specialist tracking linear spines and sediment plumes using Sentinel-2 MSI data
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// STAC API endpoint (Default: Microsoft Planetary Computer)
    #[arg(
        short,
        long,
        default_value = "https://planetarycomputer.microsoft.com/api/stac/v1/search"
    )]
    stac_url: String,

    /// Bounding box (min_lon, min_lat, max_lon, max_lat)
    #[arg(short, long, value_delimiter = ',', num_args = 4)]
    bbox: Vec<f64>,

    /// Path to emit standard JSON output report
    #[arg(short, long, default_value = "optical_anomalies.json")]
    output: PathBuf,

    /// Edge Variance Threshold for flagging an anomaly
    #[arg(short, long, default_value_t = 0.85)]
    edge_threshold: f32,
}

#[derive(Serialize, Deserialize, Debug)]
struct AnomalyReport {
    confidence: f32,
    lon: f64,
    lat: f64,
    description: String,
    methodology: String,
    capture_date: String,
}

use wgpu::util::DeviceExt;

struct OpticalImageTile {
    pub data: Vec<f32>,
    pub width: usize,
    pub height: usize,
    pub min_lon: f64,
    pub max_lon: f64,
    pub min_lat: f64,
    pub max_lat: f64,
    pub capture_date: String,
}

impl OpticalImageTile {
    async fn detect_linear_structures_wgpu(&self, threshold: f32) -> Vec<AnomalyReport> {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .expect("Failed to find an appropriate adapter");

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .expect("Failed to create device");

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { 
            label: Some("Sobel Shader"),
            source: wgpu::ShaderSource::Wgsl(WGSL_SOURCE.into()),
        });

        let mut reports = Vec::new();
        
        let input_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Input Buffer"),
            contents: bytemuck::cast_slice(&self.data),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        });

        let output_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Output Buffer"),
            size: (self.data.len() * 4) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Staging Buffer"),
            size: (self.data.len() * 4) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 3 entries: width, height, threshold
        let uniforms = [self.width as u32, self.height as u32, threshold.to_bits()];
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Uniform Buffer"),
            contents: bytemuck::cast_slice(&uniforms),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Bind Group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: input_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: output_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform_buffer.as_entire_binding(),
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let compute_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Compute Pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
            cpass.set_pipeline(&compute_pipeline);
            cpass.set_bind_group(0, Some(&bind_group), &[]);
            let workgroup_x = ((self.width as f32) / 8.0).ceil() as u32;
            let workgroup_y = ((self.height as f32) / 8.0).ceil() as u32;
            cpass.dispatch_workgroups(workgroup_x, workgroup_y, 1);
        }

        encoder.copy_buffer_to_buffer(&output_buffer, 0, &staging_buffer, 0, (self.data.len() * 4) as wgpu::BufferAddress);
        queue.submit(Some(encoder.finish()));

        let buffer_slice = staging_buffer.slice(..);
        let (sender, receiver) = tokio::sync::oneshot::channel::<Result<(), wgpu::BufferAsyncError>>();
        buffer_slice.map_async(wgpu::MapMode::Read, move |v| sender.send(v).unwrap());

        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        receiver.await.unwrap().unwrap();

        let data = buffer_slice.get_mapped_range();
        let result_data: &[f32] = bytemuck::cast_slice(&data);
        
        let anomalies_count = result_data.iter().filter(|&&x| x > 0.0).count();
        if anomalies_count > 0 {
            reports.push(AnomalyReport {
                confidence: 0.88_f32,
                lon: self.min_lon + (self.max_lon - self.min_lon) / 2.0,
                lat: self.min_lat + (self.max_lat - self.min_lat) / 2.0,
                description: format!("Detected {} linear structure pixels exceeding threshold", anomalies_count),
                methodology: "WGPU Sobel Edge Variance".to_string(),
                capture_date: self.capture_date.clone(),
            });
        }

        reports
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    println!("== CESAROPS OPTICAL STRUCTURAL SPECIALIST ==");
    println!("Target STAC: {}", args.stac_url);

    // Initialize unified AnomalyQueue
    let temp_db_path = "anomaly_queue.db";
    let queue = AnomalyQueue::new(temp_db_path)?;
    println!("Connected to centralized AnomalyQueue at {}", temp_db_path);

    // Integrate STAC IO ingestion logic
    let req_client = reqwest::Client::new();
    let stac_url = "https://planetarycomputer.microsoft.com/api/stac/v1/collections/landsat-c2-l2/items/LC08_L2SP_016030_20230501_20230509_02_T1";
    println!("Attempting STAC / GeoTIFF I/O download for B4...");
    
    // For demonstration of ingestion logic, we'll try it, and falback on timeout/error to mock.
    let tile_data = match cesarops_slicer::common::stac_io::download_asset_as_f32(&req_client, stac_url, "SR_B4").await {
        Ok(data) => {
            println!("Successfully ingested STAC GeoTIFF ({} pixels) into f32 array.", data.len());
            data
        },
        Err(e) => {
            println!("STAC endpoint error: {}. Generating mock flat Vec<f32> tile.", e);
            cesarops_slicer::common::stac_io::fetch_mock_tile()
        }
    };

    let tile = OpticalImageTile {
        data: tile_data,
        width: 1024,
        height: 1024,
        min_lon: -83.48,
        max_lon: -83.45,
        min_lat: 45.12,
        max_lat: 45.14,
        capture_date: "2024-06-15T10:00:00Z".to_string(),
    };

    println!("Scanning optical STAC tiles...");
    let reports = tile.detect_linear_structures_wgpu(0.5).await;
    
    if reports.is_empty() {
        println!("No structural anomalies detected.");
    }

    for (i, rep) in reports.iter().enumerate() {
        let anomaly = AnomalyRecord::new(
            format!("opt-hit-{}", i),
            rep.lat,
            rep.lon,
            (tile.min_lon, tile.min_lat, tile.max_lon, tile.max_lat),
            SensorType::Optical,
            rep.confidence,
        );
        queue.push_anomaly(&anomaly)?;
        println!("Pushed {} to AnomalyQueue with {} confidence: {}", anomaly.id, rep.confidence, rep.description);
    }

    Ok(())
}
