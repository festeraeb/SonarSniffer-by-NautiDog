//! Video encoding from processed frames.
//!
//! Supports two backends:
//! - **GStreamer** (feature: video-gstreamer): MP4/H.264 output
//! - **GIF fallback** (no feature): Animated GIF

use crate::video_enhanced::{EnhancedVideoResult, ProcessedFrame, ProcessingStats, SonarProcessingParams};
use std::path::Path;

#[cfg(feature = "video-gstreamer")]
pub fn encode_to_video<F>(
    frames: Vec<ProcessedFrame>,
    output_dir: &Path,
    params: &SonarProcessingParams,
    on_progress: F,
) -> anyhow::Result<EnhancedVideoResult>
where
    F: Fn(u32, u32) + Send + 'static,
{
    use anyhow::Context;
    use gstreamer as gst;
    use gstreamer::prelude::*;
    use gstreamer_app as gst_app;
    
    if frames.is_empty() {
        return Ok(EnhancedVideoResult {
            success: false,
            output_path: None,
            status: "No frames to encode".to_string(),
            processing_stats: None,
        });
    }
    
    gst::init().context("GStreamer initialization failed")?;
    
    let width = frames[0].width;
    let height = frames[0].height;
    let n_frames = frames.len() as u32;
    let fps = params.fps;
    let frame_ns = 1_000_000_000u64 / fps as u64;
    
    // Output path
    let mp4_path = output_dir.join("sonar_waterfall_enhanced.mp4");
    let mp4_location = mp4_path.to_string_lossy().replace('\\', "/");
    
    // Select encoder (prefer hardware if requested)
    let encoder = if params.prefer_hardware_encoding {
        // Try NVENC first, fall back to x264
        "nvh264enc"
    } else {
        "x264enc speed-preset=ultrafast tune=zerolatency"
    };
    
    // Build pipeline
    let pipeline_str = format!(
        "appsrc name=src \
        ! videoconvert \
        ! {encoder} \
        ! mp4mux \
        ! filesink name=fsink sync=false"
    );
    
    let element = gst::parse::launch(&pipeline_str).map_err(|e| {
        if params.prefer_hardware_encoding {
            // Try fallback to software encoding
            let fallback_str = format!(
                "appsrc name=src \
                ! videoconvert \
                ! x264enc speed-preset=ultrafast tune=zerolatency \
                ! mp4mux \
                ! filesink name=fsink sync=false"
            );
            gst::parse::launch(&fallback_str).map_err(|_| anyhow::anyhow!("Pipeline parse failed: {e}"))
        } else {
            Err(anyhow::anyhow!("Pipeline parse failed: {e}"))
        }
    })?;
    
    let pipeline = element
        .dynamic_cast::<gst::Pipeline>()
        .map_err(|_| anyhow::anyhow!("Not a pipeline"))?;
    
    // Configure filesink
    pipeline
        .by_name("fsink")
        .context("filesink not found")?
        .set_property("location", mp4_location.as_str());
    
    // Configure appsrc
    let appsrc = pipeline
        .by_name("src")
        .context("appsrc not found")?
        .dynamic_cast::<gst_app::AppSrc>()
        .map_err(|_| anyhow::anyhow!("src not an AppSrc"))?;
    
    let caps = gst::Caps::builder("video/x-raw")
        .field("format", "RGB")
        .field("width", width as i32)
        .field("height", height as i32)
        .field("framerate", gst::Fraction::new(fps as i32, 1))
        .build();
    appsrc.set_caps(Some(&caps));
    appsrc.set_property("format", gst::Format::Time);
    appsrc.set_property("is-live", false);
    appsrc.set_property("block", true);
    
    // Start pipeline
    pipeline
        .set_state(gst::State::Playing)
        .map_err(|e| anyhow::anyhow!("set_state(Playing) failed: {e:?}"))?;
    
    // Push frames
    for (i, frame) in frames.iter().enumerate() {
        let frame_size = frame.pixels.len();
        let mut buf = gst::Buffer::with_size(frame_size)
            .map_err(|_| anyhow::anyhow!("Buffer allocation failed"))?;
        
        {
            let buf_ref = buf.get_mut().unwrap();
            buf_ref.set_pts(gst::ClockTime::from_nseconds(i as u64 * frame_ns));
            buf_ref.set_duration(gst::ClockTime::from_nseconds(frame_ns));
            
            let mut map = buf_ref
                .map_writable()
                .map_err(|_| anyhow::anyhow!("Buffer map failed"))?;
            map.copy_from_slice(&frame.pixels);
        }
        
        if appsrc.push_buffer(buf).is_err() {
            break;
        }
        on_progress((i + 1) as u32, n_frames);
    }
    
    let _ = appsrc.end_of_stream();
    
    // Wait for EOS
    let bus = pipeline.bus().context("No bus")?;
    let export_ok = loop {
        match bus.timed_pop(gst::ClockTime::from_seconds(300)) {
            Some(msg) => match msg.view() {
                gst::MessageView::Eos(..) => break true,
                gst::MessageView::Error(err) => {
                    pipeline.set_state(gst::State::Null).ok();
                    anyhow::bail!("GStreamer error: {} — {}", err.error(), err.debug().unwrap_or_default());
                }
                _ => {}
            },
            None => {
                pipeline.set_state(gst::State::Null).ok();
                return Ok(EnhancedVideoResult {
                    success: false,
                    output_path: None,
                    status: "Video encoding timed out".to_string(),
                    processing_stats: None,
                });
            }
        }
    };
    
    pipeline.set_state(gst::State::Null).ok();
    
    if !export_ok {
        return Ok(EnhancedVideoResult {
            success: false,
            output_path: None,
            status: "Video encoding failed".to_string(),
            processing_stats: None,
        });
    }
    
    let file_mb = std::fs::metadata(&mp4_path)
        .map(|m| m.len() as f64 / 1_048_576.0)
        .unwrap_or(0.0);
    
    Ok(EnhancedVideoResult {
        success: true,
        output_path: Some(mp4_path.display().to_string()),
        status: format!(
            "Enhanced video export complete: {} frames, {}x{} @ {}fps ({:.1} MB)",
            n_frames, width, height, fps, file_mb
        ),
        processing_stats: None, // TODO: populate
    })
}

#[cfg(not(feature = "video-gstreamer"))]
pub fn encode_to_video<F>(
    frames: Vec<ProcessedFrame>,
    output_dir: &Path,
    params: &SonarProcessingParams,
    on_progress: F,
) -> anyhow::Result<EnhancedVideoResult>
where
    F: Fn(u32, u32) + Send + 'static,
{
    use gif::{Encoder, Frame, Repeat};
    use std::fs::File;
    
    if frames.is_empty() {
        return Ok(EnhancedVideoResult {
            success: false,
            output_path: None,
            status: "No frames to encode".to_string(),
            processing_stats: None,
        });
    }
    
    let width = frames[0].width as u16;
    let height = frames[0].height as u16;
    let n_frames = frames.len() as u32;
    
    let gif_path = output_dir.join("sonar_waterfall_enhanced.gif");
    let mut file = File::create(&gif_path)?;
    
    // Build palette from first frame (simple approach)
    // For better quality, compute global palette across all frames
    let mut palette = Vec::with_capacity(256 * 3);
    for i in 0..=255u8 {
        palette.extend_from_slice(&[i, i, i]); // Grayscale
    }
    
    let mut encoder = Encoder::new(&mut file, width, height, &palette)?;
    encoder.set_repeat(Repeat::Infinite)?;
    
    for (i, processed_frame) in frames.iter().enumerate() {
        // Convert RGB to indexed (grayscale palette)
        let indexed: Vec<u8> = processed_frame
            .pixels
            .chunks(3)
            .map(|rgb| {
                // Simple grayscale: average RGB
                ((rgb[0] as u32 + rgb[1] as u32 + rgb[2] as u32) / 3) as u8
            })
            .collect();
        
        let mut frame = Frame::default();
        frame.width = width;
        frame.height = height;
        frame.buffer = indexed.into();
        frame.delay = (100 / params.fps) as u16; // delay in 1/100s
        
        encoder.write_frame(&frame)?;
        on_progress((i + 1) as u32, n_frames);
    }
    
    Ok(EnhancedVideoResult {
        success: true,
        output_path: Some(gif_path.display().to_string()),
        status: format!(
            "Enhanced GIF export complete: {} frames (install GStreamer for MP4)",
            n_frames
        ),
        processing_stats: None,
    })
}
