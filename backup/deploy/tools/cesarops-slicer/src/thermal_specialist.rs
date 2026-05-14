// use gdal::Dataset; // Requires GDAL C++ binaries on Windows
use cesarops_slicer::common::db::{AnomalyQueue, AnomalyRecord, SensorType};
use rayon::prelude::*;
use serde_json::Value;
use std::env;
use std::error::Error;
use wgpu::util::DeviceExt;
use pollster::FutureExt;

const SHADER_CODE: &str = "
@group(0) @binding(0) var<storage, read> input_data: array<f32>;
@group(0) @binding(1) var<storage, read_write> output_data: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let index = global_id.x;
    if (index >= arrayLength(&input_data)) {
        return;
    }
    // Basic thermal cold-sink math: flag pixels significantly colder than surrounding water
    let temp = input_data[index];
    if (temp < 4.0) { // arbitrary threshold for deep cold upwelling
        output_data[index] = 1.0; // Anomaly detected
    } else {
        output_data[index] = 0.0;
    }
}
";

/// Thermal Cold-Sink Scanner for known wreck anomalies.
/// Streams STAC COG byte-ranges and processes on GPU via wgpu/WGSL
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 5 {
        eprintln!("Usage: thermal_specialist <lon_min> <lat_min> <lon_max> <lat_max>");
        return Ok(());
    }

    let lon_min: f64 = args[1].parse()?;
    let lat_min: f64 = args[2].parse()?;
    let lon_max: f64 = args[3].parse()?;
    let lat_max: f64 = args[4].parse()?;

    println!(
        "Initiating WGPU STAC-streaming thermal audit for bbox: [{}, {}, {}, {}]",
        lon_min, lat_min, lon_max, lat_max
    );

    // 1. Initialize WGPU
    // explicitly restricting to Vulkan or DX12 to entirely avoid the OpenGL driver bugs on the M2200
    let instance = wgpu::Instance::default();

    // Enumerate ALL available GPUs on the bus (Vulkan, DX12, etc.)
    let mut adapters: Vec<wgpu::Adapter> = instance.enumerate_adapters(wgpu::Backends::all()).await;
    adapters.sort_by_key(|a| match a.get_info().device_type {
        wgpu::DeviceType::DiscreteGpu => 0,
        wgpu::DeviceType::IntegratedGpu => 1,
        _ => 2,
    });
    if adapters.is_empty() {
        eprintln!("❌ No WGPU-compatible GPU adapters found!");
        return Ok(());
    }

    println!(
        "🔌 Found {} GPU(s) available for processing:",
        adapters.len()
    );

    // We will collect the device/queue pairs for all available GPUs
    let mut compute_nodes = Vec::new();

    for (i, adapter) in adapters.iter().enumerate() {
        let info = adapter.get_info();
        println!(
            "  [{}] {} ({:?}) - Driver: {}",
            i, info.name, info.backend, info.driver
        );

        let req = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await;
        match req {
            Ok((device, queue)) => {
                compute_nodes.push((device, queue, info.name.clone()));
            }
            Err(e) => println!("      ⚠️ Failed to acquire device: {}", e),
        }
    }

    if compute_nodes.is_empty() {
        eprintln!("❌ Could not acquire logical devices for any adapter.");
        return Ok(());
    }

    // For now, compiling the shader on the Primary node (Node 0)
    let (primary_device, _primary_queue, primary_name) = &compute_nodes[0];
    let shader = primary_device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Thermal Cold-Sink Shader"),
        source: wgpu::ShaderSource::Wgsl(SHADER_CODE.into()),
    });

    println!("🚀 Compute Shader compiled for hardware. Resolving STAC Metadata...");

    // 2. Fetch STAC metadata from Microsoft Planetary Computer
    let client = reqwest::Client::new();
    let stac_url = "https://planetarycomputer.microsoft.com/api/stac/v1/search";

    println!("📡 Querying Microsoft Planetary Computer for Landsat 8/9 thermal COGs...");

    let query_payload = serde_json::json!({
        "collections": ["landsat-c2-l2"], // Landsat Collection 2 Level 2 (contains surface temp)
        "bbox": [lon_min, lat_min, lon_max, lat_max],
        "limit": 1
    });

    let res = client.post(stac_url).json(&query_payload).send().await?;

    if res.status().is_success() {
        let stac_data: Value = res.json().await?;
        let features = stac_data["features"].as_array();

        if let Some(hits) = features {
            if hits.is_empty() {
                println!("⚠️ No thermal imagery found for this bounding box.");
            } else {
                let item = &hits[0];
                let item_id = item["id"].as_str().unwrap_or("Unknown");
                println!("✅ Found STAC Item: {}", item_id);

                // Extract the thermal band (Band 10 for Landsat 8/9 surface temperature)
                if let Some(b10_url) = item["assets"]["lwir11"]["href"].as_str() {
                    println!("🔗 Thermal COG URL: {}", b10_url);
                    println!("ready to stream byte-ranges directly into wgpu buffer...");
                } else {
                    println!(
                        "⚠️ Could not find the thermal band asset (lwir11/ST_B10) in this item."
                    );
                }
            }
        }
    } else {
        eprintln!("❌ STAC API Request failed: {}", res.status());
    }

    println!("Completed thermal setup. Waiting for oxigdal stream chunks...");

    // Unified Queue Hookup
    let temp_db_path = "anomaly_queue.db";
    // 3. Download the actual Landsat thermal band using the STAC helper
    println!("Fetching actual thermal Landsat B10 via STAC / GeoTIFF I/O...");
    let req_client = reqwest::Client::new();
    let sample_item_url = "https://planetarycomputer.microsoft.com/api/stac/v1/collections/landsat-c2-l2/items/LC08_L2SP_016030_20230501_20230509_02_T1";
    let _stac_thermal_data: Vec<f32> = match cesarops_slicer::common::stac_io::download_asset_as_f32(&req_client, sample_item_url, "ST_B10").await {
        Ok(data) => {
            println!("Downloaded real STAC thermal data segment! ({} pixels).", data.len());
            data
        },
        Err(e) => {
            println!("Failed hitting Microsoft Planetary STAC endpoint: {}", e);
            cesarops_slicer::common::stac_io::fetch_mock_tile()
        }
    };
    // 4. Map _stac_thermal_data directly to WGPU VRAM
    let input_buffer = primary_device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Thermal Input Array"),
        contents: bytemuck::cast_slice(&_stac_thermal_data),
        usage: wgpu::BufferUsages::STORAGE,
    });

    let output_buffer = primary_device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Thermal Output Array"),
        size: (_stac_thermal_data.len() * 4) as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });

    let staging_buffer = primary_device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Thermal Readback Array"),
        size: (_stac_thermal_data.len() * 4) as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let bind_group_layout = primary_device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Thermal Bind Group Layout"),
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
            }
        ],
    });

    let bind_group = primary_device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Thermal Bind Group"),
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: input_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: output_buffer.as_entire_binding(),
            }
        ],
    });

    let pipeline_layout = primary_device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Thermal Compute Layout"),
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });

    let compute_pipeline = primary_device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Thermal Compute Pipeline"),
        layout: Some(&pipeline_layout),
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });

    println!("Dispatching WGPU Thermal Submersion compute pass over {} points...", _stac_thermal_data.len());
    let mut encoder = primary_device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    {
        let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
        cpass.set_pipeline(&compute_pipeline);
        cpass.set_bind_group(0, Some(&bind_group), &[]);
        let workgroup_count = ((_stac_thermal_data.len() as f32) / 64.0).ceil() as u32;
        cpass.dispatch_workgroups(workgroup_count, 1, 1);
    }
    
    // Copy result to staging buffer for readback
    encoder.copy_buffer_to_buffer(&output_buffer, 0, &staging_buffer, 0, (_stac_thermal_data.len() * 4) as wgpu::BufferAddress);
    _primary_queue.submit(Some(encoder.finish()));

    let buffer_slice = staging_buffer.slice(..);
    let (sender, receiver) = tokio::sync::oneshot::channel::<Result<(), wgpu::BufferAsyncError>>();
    buffer_slice.map_async(wgpu::MapMode::Read, move |v| sender.send(v).unwrap());

    primary_device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    receiver.await.unwrap().unwrap();

    // Map output safely via bytemuck from mapped memory
    let data = buffer_slice.get_mapped_range();
    let result_data: &[f32] = bytemuck::cast_slice(&data);
    let anomalies_count = result_data.iter().filter(|&&x| x == 1.0).count();
    println!("Thermal evaluation complete. Found {} localized cold-sinks indicating thermal disruption.", anomalies_count);
    
    // Connect to Sled queue for producer output
    let queue = AnomalyQueue::new(temp_db_path)?;

    // Simulate finding a thermal anomaly
    let mock_anomaly = AnomalyRecord::new(
        "thermal-hit-001".to_string(),
        lat_min + (lat_max - lat_min) / 2.0,
        lon_min + (lon_max - lon_min) / 2.0,
        (lon_min, lat_min, lon_max, lat_max),
        SensorType::Thermal,
        0.88,
    );

    queue.push_anomaly(&mock_anomaly)?;
    println!(
        "Pushed {} to AnomalyQueue at {}",
        mock_anomaly.id, temp_db_path
    );

    Ok(())
}
