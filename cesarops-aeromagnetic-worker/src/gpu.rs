//! WGPU dipole "pull" detector — annulus background + inner ± lobe pairing.
//! Uses Vulkan on NVIDIA (2060 / 1070 / P100) when available.

use bytemuck::{Pod, Zeroable};
use std::borrow::Cow;

/// Pick GPU: `MAG_WGPU_ADAPTER=2060|1070` or `MAG_WGPU_INDEX=0|1`.
async fn select_adapter(instance: &wgpu::Instance) -> wgpu::Adapter {
    let backends = wgpu::Backends::all();
    let adapters: Vec<_> = instance.enumerate_adapters(backends).into_iter().collect();
    if adapters.is_empty() {
        panic!("No WGPU adapters found");
    }
    for (i, a) in adapters.iter().enumerate() {
        let info = a.get_info();
        log::info!("WGPU adapter[{}]: {} ({:?})", i, info.name, info.backend);
    }
    let try_names: Vec<String> = std::env::var("MAG_WGPU_ADAPTER_TRY")
        .ok()
        .map(|s| s.split(',').map(|x| x.trim().to_lowercase()).filter(|x| !x.is_empty()).collect())
        .unwrap_or_default();
    let single = std::env::var("MAG_WGPU_ADAPTER").ok();
    let mut needles: Vec<String> = try_names;
    if let Some(one) = single {
        let l = one.to_lowercase();
        if !l.is_empty() && l != "auto" && l != "any" && !needles.contains(&l) {
            needles.insert(0, l);
        }
    }
    for needle in needles {
        let pick = adapters.iter().enumerate().find_map(|(i, a)| {
            let info = a.get_info();
            if info.name.to_lowercase().contains(&needle) {
                log::info!("Using adapter match '{}' -> [{}] {}", needle, i, info.name);
                Some(i)
            } else {
                None
            }
        });
        if let Some(i) = pick {
            return adapters.into_iter().nth(i).expect("adapter index");
        }
    }
    if let Ok(idx) = std::env::var("MAG_WGPU_INDEX") {
        if let Ok(i) = idx.parse::<usize>() {
            if let Some(a) = adapters.into_iter().nth(i) {
                let info = a.get_info();
                log::info!("Using MAG_WGPU_INDEX={}: {}", i, info.name);
                return a;
            }
        }
    }
    instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        })
        .await
        .expect("GPU adapter")
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct Params {
    pub width: u32,
    pub height: u32,
    pub inner_radius: u32,
    pub outer_radius: u32,
    pub pixel_size_m: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct OutPixel {
    pub bg_mean: f32,
    pub peak_pos: f32,
    pub peak_neg: f32,
    pub dipole_separation_m: f32,
    pub score: f32,
}

pub async fn dipole_scan_grid_try(
    input_data: &[f32],
    params: Params,
) -> Result<Vec<OutPixel>, String> {
    let result = dipole_scan_grid_inner(input_data, params).await;
    result
}

pub async fn dipole_scan_grid(
    input_data: &[f32],
    params: Params,
) -> Vec<OutPixel> {
    dipole_scan_grid_inner(input_data, params)
        .await
        .expect("dipole_scan_grid")
}

async fn dipole_scan_grid_inner(
    input_data: &[f32],
    params: Params,
) -> Result<Vec<OutPixel>, String> {
    let width = params.width;
    let height = params.height;
    let num_pixels = (width * height) as usize;
    assert_eq!(input_data.len(), num_pixels);

    let instance = wgpu::Instance::default();
    let adapter = select_adapter(&instance).await;
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor::default(), None)
        .await
        .map_err(|e| format!("GPU device: {e}"))?;

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Dipole Shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("dipole_shader.wgsl"))),
    });

    use wgpu::util::DeviceExt;
    let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Params"),
        contents: bytemuck::bytes_of(&params),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let input_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Input"),
        contents: bytemuck::cast_slice(input_data),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    });
    let output_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Output"),
        size: (num_pixels * std::mem::size_of::<OutPixel>()) as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("bg"),
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: params_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: input_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: output_buffer.as_entire_binding(),
            },
        ],
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("pl"),
        bind_group_layouts: &[&bind_group_layout],
        push_constant_ranges: &[],
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("dipole"),
        layout: Some(&pipeline_layout),
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("enc"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups((width + 15) / 16, (height + 15) / 16, 1);
    }

    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("staging"),
        size: output_buffer.size(),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_buffer_to_buffer(&output_buffer, 0, &staging, 0, output_buffer.size());
    queue.submit(Some(encoder.finish()));

    let slice = staging.slice(..);
    let (tx, rx) = flume::bounded(1);
    slice.map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
    device.poll(wgpu::Maintain::Wait);
    rx.recv_async().await.unwrap().unwrap();

    let data = slice.get_mapped_range();
    let results: Vec<OutPixel> = bytemuck::cast_slice(&data).to_vec();
    drop(data);
    staging.unmap();
    Ok(results)
}
