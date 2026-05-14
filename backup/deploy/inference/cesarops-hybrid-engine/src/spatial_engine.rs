//! Spatial Engine — Nauticus sub-surface scanner using wgpu compute shaders.
//!
//! Loads the pre-compiled SPIR-V / WGSL shader, dispatches tile scans across
//! the P100 GPUs, runs in-shader reduction to produce sparse anomaly metadata,
//! and streams results to the Tier 3 supervisor node over TCP.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tracing::info;

use crate::cluster::HybridClusterCoordinator;

// ── Anomaly metadata ──────────────────────────────────────────────────────────

/// Sparse anomaly record produced by the in-shader reduction pass.
/// Only anomalous pixels are included — raw raster data is never transmitted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubSurfaceAnomalyMetadata {
    /// WGS-84 longitude of the anomaly centroid.
    pub geographic_anchor_x: f64,
    /// WGS-84 latitude of the anomaly centroid.
    pub geographic_anchor_y: f64,
    /// Confidence score [0.0, 1.0] from the dipole signature repetition test.
    pub signature_repetition_confidence: f32,
    /// Estimated footprint radius in metres.
    pub footprint_radius_meters: f32,
    /// Dipole separation in metres — key discriminator for hull vs geological.
    pub dipole_separation_m: f32,
    /// Phase coherence from nauticuvs curvelet pass [0.0, 1.0].
    pub phase_coherence: f32,
}

// ── Nauticus compute pipeline ─────────────────────────────────────────────────

/// Wraps the wgpu compute pipeline for the Nauticus sub-surface scanner.
/// Loads the WGSL shader (compiled to SPIR-V by naga at build time).
pub struct NauticusPipeline {
    pub pipeline:          wgpu::ComputePipeline,
    pub bind_group_layout: wgpu::BindGroupLayout,
}

impl NauticusPipeline {
    pub fn new(device: &wgpu::Device) -> Self {
        // WGSL shader — compiled to SPIR-V by naga (wgpu's built-in compiler).
        // This is the P100-optimised variant: 32×32 workgroups, large tile support.
        // The dipole_shader.wgsl from cesarops-aeromagnetic-worker is the prototype;
        // this version adds the in-shader reduction pass for sparse output.
        let cs_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label:  Some("Nauticus Sub-surface Signature Shader"),
            source: wgpu::ShaderSource::Wgsl(
                std::borrow::Cow::Borrowed(include_str!("shaders/nauticus_scanner.wgsl"))
            ),
        });

        let bind_group_layout = device.create_bind_group_layout(
            &wgpu::BindGroupLayoutDescriptor {
                label:   Some("Nauticus Layout"),
                entries: &[
                    // Binding 0: input magnetic/thermal/spectral grid (read-only)
                    wgpu::BindGroupLayoutEntry {
                        binding:    0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty:                 wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size:   None,
                        },
                        count: None,
                    },
                    // Binding 1: sparse anomaly output (read-write, in-shader reduction)
                    wgpu::BindGroupLayoutEntry {
                        binding:    1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty:                 wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size:   None,
                        },
                        count: None,
                    },
                    // Binding 2: scan parameters uniform (grid size, pixel_size_m, thresholds)
                    wgpu::BindGroupLayoutEntry {
                        binding:    2,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty:                 wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size:   None,
                        },
                        count: None,
                    },
                ],
            }
        );

        let pipeline_layout = device.create_pipeline_layout(
            &wgpu::PipelineLayoutDescriptor {
                label:                Some("Nauticus Pipeline Layout"),
                bind_group_layouts:   &[Some(&bind_group_layout)],
                immediate_size:       0,
            }
        );

        let pipeline = device.create_compute_pipeline(
            &wgpu::ComputePipelineDescriptor {
                label:               Some("Nauticus Compute Pipeline"),
                layout:              Some(&pipeline_layout),
                module:              &cs_module,
                entry_point:         Some("main"),
                compilation_options: Default::default(),
                cache:               None,
            }
        );

        Self { pipeline, bind_group_layout }
    }
}

// ── Scan parameters uniform ───────────────────────────────────────────────────

/// Uniform buffer passed to the compute shader — describes the tile geometry.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ScanParams {
    pub width:          u32,
    pub height:         u32,
    pub inner_radius:   u32,   // dipole inner annulus in pixels
    pub outer_radius:   u32,   // dipole outer annulus in pixels
    pub pixel_size_m:   f32,   // metres per pixel
    pub score_threshold: f32,  // minimum score to include in sparse output
    pub geo_origin_x:   f32,   // WGS-84 longitude of tile origin
    pub geo_origin_y:   f32,   // WGS-84 latitude of tile origin
}

// ── Anomaly extraction and network transport ──────────────────────────────────

/// Read back the sparse anomaly output from GPU memory and stream to supervisor.
///
/// The in-shader reduction has already filtered out background pixels —
/// only anomalous detections are in the output buffer. This keeps PCIe
/// traffic minimal and avoids locking the bus with raw raster data.
pub async fn extract_and_send_anomalies(
    device:               &wgpu::Device,
    output_buffer:        &wgpu::Buffer,
    network_stream:       &mut tokio::net::TcpStream,
    coordinator:          &HybridClusterCoordinator,
) -> Result<()> {
    // Map the output buffer slice from P100 HBM2 to CPU host space asynchronously.
    let buffer_slice = output_buffer.slice(..);
    let (tx, rx) = tokio::sync::oneshot::channel();

    buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });

    // Poll the device to drive the async map to completion.
    // wgpu 29.x: use PollType::Wait for blocking completion.
    device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout:          None,
    }).unwrap();

    rx.await??;

    let data = buffer_slice.get_mapped_range();

    // Deserialise the sparse anomaly records from the GPU output buffer.
    // The shader writes a packed array of fixed-size anomaly structs.
    let detected: Vec<SubSurfaceAnomalyMetadata> = if data.len() >= 4 {
        // First 4 bytes = count of valid anomalies written by the shader.
        let count = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        let record_size = std::mem::size_of::<[f32; 6]>(); // 6 f32 fields
        let mut anomalies = Vec::with_capacity(count);

        for i in 0..count {
            let offset = 4 + i * record_size;
            if offset + record_size > data.len() { break; }
            let chunk = &data[offset..offset + record_size];
            let fields: [f32; 6] = bytemuck::pod_read_unaligned(chunk);
            anomalies.push(SubSurfaceAnomalyMetadata {
                geographic_anchor_x:            fields[0] as f64,
                geographic_anchor_y:            fields[1] as f64,
                signature_repetition_confidence: fields[2],
                footprint_radius_meters:         fields[3],
                dipole_separation_m:             fields[4],
                phase_coherence:                 fields[5],
            });
        }
        anomalies
    } else {
        Vec::new()
    };

    // Unmap immediately to release the GPU buffer for the next dispatch.
    drop(data);
    output_buffer.unmap();

    info!("Spatial: {} anomalies detected — streaming to supervisor", detected.len());

    // Serialise with postcard (no-std compatible, compact binary format).
    // This is the sparse payload — typically kilobytes, not megabytes.
    let payload = postcard::to_allocvec(&detected)?;

    // Write length-prefixed frame so the supervisor can read complete messages.
    let len = payload.len() as u32;
    network_stream.write_all(&len.to_le_bytes()).await?;
    network_stream.write_all(&payload).await?;

    // Signal the coordinator that this spatial dispatch is complete.
    coordinator.spatial_dispatch_complete();

    Ok(())
}

// ── Spatial engine run loop ───────────────────────────────────────────────────

/// Main spatial engine loop — processes tiles from the scan queue.
pub async fn run(
    coordinator:     Arc<HybridClusterCoordinator>,
    supervisor_addr: &str,
) -> Result<()> {
    info!("Spatial engine: connecting to supervisor at {}", supervisor_addr);

    let mut stream = tokio::net::TcpStream::connect(supervisor_addr).await
        .unwrap_or_else(|e| {
            // Supervisor not available — continue without network transport.
            // Anomalies will be logged locally until connection is established.
            panic!("Cannot connect to supervisor {}: {}", supervisor_addr, e);
        });

    // Build the Nauticus pipeline on GPU 0 (primary P100).
    let node = coordinator.nodes.first()
        .expect("No GPU nodes available");
    let pipeline = NauticusPipeline::new(&node.device);

    info!("Spatial engine: Nauticus pipeline ready on P100-0");
    info!("Spatial engine: waiting for tile jobs from scan queue...");

    // TODO: wire into the scan_queue job consumer.
    // For now, demonstrate a synthetic tile dispatch.
    demo_synthetic_tile_scan(&coordinator, &pipeline, &mut stream).await?;

    Ok(())
}

/// Demonstrate a synthetic tile scan — replace with real scan_queue consumer.
async fn demo_synthetic_tile_scan(
    coordinator: &HybridClusterCoordinator,
    pipeline:    &NauticusPipeline,
    stream:      &mut tokio::net::TcpStream,
) -> Result<()> {
    let node = coordinator.nodes.first().expect("No GPU nodes");

    // Synthetic 256×256 magnetic grid with one injected dipole.
    let width  = 256u32;
    let height = 256u32;
    let n      = (width * height) as usize;
    let mut grid = vec![0.0f32; n];

    // Background field
    for y in 0..height {
        for x in 0..width {
            grid[(y * width + x) as usize] = 50_000.0 + ((x + y) % 10) as f32 * 0.1;
        }
    }
    // Inject synthetic dipole at centre
    let cx = width / 2;
    let cy = height / 2;
    grid[((cy - 2) * width + cx) as usize] += 150.0;
    grid[((cy + 2) * width + cx) as usize] -= 100.0;

    let grid_bytes: &[u8] = bytemuck::cast_slice(&grid);

    // Stage the tile on GPU-0.
    coordinator.page_out_llm_and_stage_spatial(0, grid_bytes).await?;

    // Build scan params uniform.
    let params = ScanParams {
        width,
        height,
        inner_radius:    10,
        outer_radius:    25,
        pixel_size_m:    200.0,
        score_threshold: 0.1,
        geo_origin_x:    -83.5,
        geo_origin_y:    42.5,
    };

    use wgpu::util::DeviceExt;
    let params_buf = node.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label:    Some("Scan Params"),
        contents: bytemuck::bytes_of(&params),
        usage:    wgpu::BufferUsages::UNIFORM,
    });

    // Build bind group.
    let bind_group = node.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label:   Some("Nauticus Bind Group"),
        layout:  &pipeline.bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: node.spatial_staging_buffer.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: node.anomaly_output_buffer.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: params_buf.as_entire_binding() },
        ],
    });

    // Dispatch compute — 32×32 workgroups for P100 SM utilisation.
    let mut encoder = node.device.create_command_encoder(
        &wgpu::CommandEncoderDescriptor { label: Some("Nauticus Encoder") }
    );
    {
        let mut cpass = encoder.begin_compute_pass(
            &wgpu::ComputePassDescriptor { label: Some("Nauticus Pass"), timestamp_writes: None }
        );
        cpass.set_pipeline(&pipeline.pipeline);
        cpass.set_bind_group(0, &bind_group, &[]);
        // 32×32 workgroup size — optimal for P100's 56 SMs
        let wg_x = (width  + 31) / 32;
        let wg_y = (height + 31) / 32;
        cpass.dispatch_workgroups(wg_x, wg_y, 1);
    }
    node.queue.submit(Some(encoder.finish()));

    // Read back anomalies and stream to supervisor.
    extract_and_send_anomalies(
        &node.device,
        &node.anomaly_output_buffer,
        stream,
        coordinator,
    ).await?;

    Ok(())
}
