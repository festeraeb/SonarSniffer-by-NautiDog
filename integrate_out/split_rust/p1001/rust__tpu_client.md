# integrate/unmapped/laptopdump_wreckhunter_build/tpu_client.py

## Verdict
PORT_TO_PIPELINES

## Rust path
cesarops-inference/src/integrate/tpu_client.rs

## Rust source
```rust
//! TPU Client for remote Coral TPU inference (glint/jitter detection)
//!
//! This module provides a production-ready Rust client for the Xenon TPU server,
//! converting various image inputs to base64-encoded PNG and sending inference requests.
//!
//! # Usage
//! ```rust
//! use cesarops_inference::tpu_client::TpuClient;
//!
//! let client = TpuClient::new("http://xenon:5001");
//! let result = client.check_glint_jitter(image_path, Some(meta)).await?;
//! ```

use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use image::{DynamicImage, ImageFormat};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::
