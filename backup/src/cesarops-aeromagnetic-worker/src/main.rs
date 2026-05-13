use bytemuck::{Pod, Zeroable};
mod discriminator;

use discriminator::{Wellhead, KnownWreck, CandidateMatch, cross_reference_candidate};
use std::borrow::Cow;
use ndarray::Array2;
use nauticuvs::{curvelet_forward, Scalar};

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct Params {
    width: u32,
    height: u32,
    inner_radius: u32,
    outer_radius: u32,
    pixel_size_m: f32, // meters per pixel
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct OutPixel {
    bg_mean: f32,
    peak_pos: f32,
    peak_neg: f32,
    dipole_separation_m: f32,
    score: f32,
}

async fn run_compute() {
    let instance = wgpu::Instance::default();
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
        .expect("Failed to find a suitable GPU adapter");

    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor::default())
        .await
        .expect("Failed to create device");

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Dipole Shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("dipole_shader.wgsl"))),
    });

    let width = 256;
    let height = 256;
    let num_pixels = (width * height) as usize;

    let params = Params {
        width,
        height,
        inner_radius: 10,   // approximate 2000yd inner
        outer_radius: 25,   // approximate 5000yd outer
        pixel_size_m: 200.0,
    };

    // Synthetic magnetic grid (mostly noise/background, with one synthetic dipole)
    let mut input_data = vec![0.0f32; num_pixels];
    for y in 0..height {
        for x in 0..width {
            input_data[(y * width + x) as usize] = 50000.0 + ((x + y) % 10) as f32 * 0.1;
        }
    }
    // Inject synthetic dipole
    let cx = width / 2;
    let cy = height / 2;
    input_data[((cy - 2) * width + cx) as usize] += 150.0;  // Positive lobe
    input_data[((cy + 2) * width + cx) as usize] -= 100.0;  // Negative lobe

    use wgpu::util::DeviceExt;
    let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Params Buffer"),
        contents: bytemuck::bytes_of(&params),
        usage: wgpu::BufferUsages::UNIFORM,
    });

    let input_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Input Grid Buffer"),
        contents: bytemuck::cast_slice(&input_data),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    });

    let output_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Output Buffer"),
        size: (num_pixels * std::mem::size_of::<OutPixel>()) as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Bind Group Layout"),
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
        label: Some("Bind Group"),
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

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Command Encoder"),
    });

    {
        let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Compute Pass"),
            timestamp_writes: None,
        });
        cpass.set_pipeline(&compute_pipeline);
        cpass.set_bind_group(0, &bind_group, &[]);
        
        let workgroup_count_x = (width + 15) / 16;
        let workgroup_count_y = (height + 15) / 16;
        cpass.dispatch_workgroups(workgroup_count_x, workgroup_count_y, 1);
    }

    // Read back results
    let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Staging Buffer"),
        size: output_buffer.size(),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    encoder.copy_buffer_to_buffer(&output_buffer, 0, &staging_buffer, 0, output_buffer.size());

    queue.submit(Some(encoder.finish()));
    
    let buffer_slice = staging_buffer.slice(..);
    let (sender, receiver) = flume::bounded(1);
    buffer_slice.map_async(wgpu::MapMode::Read, move |v| sender.send(v).unwrap());
    
    device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    }).unwrap();
    receiver.recv_async().await.unwrap().unwrap();

    let data = buffer_slice.get_mapped_range();
    let results: &[OutPixel] = bytemuck::cast_slice(&data);

    let mut best_score = 0.0;
    let mut best_idx = 0;
    for (i, p) in results.iter().enumerate() {
        if p.score > best_score {
            best_score = p.score;
            best_idx = i;
        }
    }

    let p_x = best_idx as u32 % width;
    let p_y = best_idx as u32 / width;
    println!("Best Dipole Found at x: {}, y: {}", p_x, p_y);
    println!("Features: {:#?}", results[best_idx]);

    // NauticUVs Refinement: Calculate FDCT energy ratio using the user's crate
    // Create an ndarray from the relevant window around the mag anomaly
    let mut energy_ratio = 0.0f32;
    let window_size = 64;
    let mut window = Array2::<Scalar>::zeros((window_size, window_size));
    let x_start = (p_x as i32 - (window_size / 2) as i32).max(0) as u32;
    let y_start = (p_y as i32 - (window_size / 2) as i32).max(0) as u32;

    for wy in 0..window_size {
        for wx in 0..window_size {
            let gx = (x_start + wx as u32).min(width - 1);
            let gy = (y_start + wy as u32).min(height - 1);
            window[[wy, wx]] = input_data[(gy * width + gx) as usize];
        }
    }

    if let Ok(coeffs) = curvelet_forward(&window, 4) {
        // Proxy energy ratio as sum of AC scales (detail + fine) vs background
        let mut total_ac_energy = 0.0f64;
        
        // Sum detail scale energy
        for scale in &coeffs.detail {
            for subband in scale {
                total_ac_energy += subband.iter().map(|c| c.norm_sqr()).sum::<f64>();
            }
        }
        
        // Sum fine scale energy
        total_ac_energy += coeffs.fine.iter().map(|c| c.norm_sqr()).sum::<f64>();

        energy_ratio = (total_ac_energy as f32 / 1000.0).min(10.0); 
        println!("NauticUVs Curvelet Energy Proxy: {:.3}", energy_ratio);
    }

    let mut candidate = CandidateMatch {
        label_id: 1,
        center_lat: 42.1,
        center_lon: -81.2,
        dipole_score: best_score,
        dipole_verdict: "Strong".to_string(),
        ground_truth: "".to_string(),
        ground_truth_name: "".to_string(),
        well_distance_m: None,
        nearest_wellhead: None,
        wreck_distance_m: None,
        nearest_known_wreck: None,
        bonus_score: 0.0,
        curvelet_energy_ratio: Some(energy_ratio),
    };
    
    let wellheads = vec![
        Wellhead {
            well_id: "W1".to_string(),
            name: "Lake Erie Well A".to_string(),
            lat: 42.099,
            lon: -81.201,
            status: "Active".to_string(),
            well_type: "Gas".to_string(),
            township: "".to_string(),
            county: "".to_string(),
            target: "".to_string(),
            is_lake_erie: true,
        }
    ];
    
    let wrecks = vec![
        KnownWreck {
            name: "Colgate".to_string(),
            lat: 42.105,
            lon: -81.195,
            vessel_type: "Schooner".to_string(),
            length_ft: 100.0,
            depth_ft: 50.0,
            source: "DB".to_string(),
        }
    ];
    
    cross_reference_candidate(&mut candidate, &wellheads, &wrecks);
    
println!("Candidate Verdict: {} - Nearest Well: {:?} ({}m) - Nearest Wreck: {:?} ({}m) - Bonus: {:.1}",
        candidate.ground_truth,
        candidate.nearest_wellhead,
        candidate.well_distance_m.unwrap_or(0.0).round(),
        candidate.nearest_known_wreck,
        candidate.wreck_distance_m.unwrap_or(0.0).round(),
        candidate.bonus_score,
    );
}

fn main() {
    env_logger::init();
    println!("Starting CESARO aeromagnetic WGPU compute worker...");
    pollster::block_on(run_compute());
}

