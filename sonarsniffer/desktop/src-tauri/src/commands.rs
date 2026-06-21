//! Tauri IPC commands — thin wrappers around `sonarsniffer_lib`.

use serde::Serialize;
use sonarsniffer::deps;
use sonarsniffer::format_detector;
use sonarsniffer::host_profile::{self, SettingsTier, SonarSurveyHint, SuggestedSettings};
use sonarsniffer::license::{self, LicenseStatus};
use sonarsniffer::outputs::{build_outputs, OutputSummary, PipelineOptions};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LicenseUiStatus {
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub days_remaining: Option<u64>,
    pub private_build: bool,
    pub contact_email: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineRunResult {
    pub outputs: OutputSummary,
    pub layout_confirmation_required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stitch_layout: Option<sonarsniffer::channel_discovery::StitchLayoutProposal>,
    pub video_rendering: bool,
}

fn private_build_enabled() -> bool {
    env!("SONARSNIFFER_PRIVATE_BUILD") == "1"
}

fn contact_email() -> String {
    env!("SONARSNIFFER_LICENSE_EMAIL").to_string()
}

fn license_ui_status() -> LicenseUiStatus {
    if private_build_enabled() || cfg!(feature = "full-license") {
        return LicenseUiStatus {
            state: "unlocked".into(),
            days_remaining: None,
            private_build: private_build_enabled(),
            contact_email: contact_email(),
        };
    }

    let state = license::load_license();
    match license::check_license(&state) {
        LicenseStatus::Licensed => LicenseUiStatus {
            state: "unlocked".into(),
            days_remaining: None,
            private_build: false,
            contact_email: contact_email(),
        },
        LicenseStatus::Trial { days_remaining } => LicenseUiStatus {
            state: "trial".into(),
            days_remaining: Some(days_remaining),
            private_build: false,
            contact_email: contact_email(),
        },
        LicenseStatus::Expired => LicenseUiStatus {
            state: "expired".into(),
            days_remaining: Some(0),
            private_build: false,
            contact_email: contact_email(),
        },
    }
}

fn pipeline_allowed() -> Result<(), String> {
    if private_build_enabled() || cfg!(feature = "full-license") {
        return Ok(());
    }
    let state = license::load_license();
    match license::check_license(&state) {
        LicenseStatus::Licensed | LicenseStatus::Trial { .. } => Ok(()),
        LicenseStatus::Expired => Err(
            "Trial expired. Enter a license key on the License tab to continue.".into(),
        ),
    }
}

fn run_pipeline_blocking(
    app: AppHandle,
    file_name: String,
    options: PipelineOptions,
) -> Result<PipelineRunResult, String> {
    pipeline_allowed()?;

    let path = PathBuf::from(&file_name);
    if !path.exists() {
        return Err(format!("File not found: {file_name}"));
    }

    host_profile::init_runtime();

    let detected = format_detector::detect_and_parse(&path);
    if detected.parse.pings.is_empty() {
        return Err(format!(
            "No pings parsed from {} ({})",
            file_name, detected.format
        ));
    }

    let app_progress = app.clone();
    let summary = build_outputs(
        &path,
        &detected.parse,
        &options,
        None,
        Some(&move |step: &str, pct: u8| {
            let _ = app_progress.emit(
                "pipeline-progress",
                serde_json::json!({ "step": step, "pct": pct }),
            );
        }),
    )
    .map_err(|e| format!("Pipeline failed: {e:#}"))?;

    Ok(PipelineRunResult {
        layout_confirmation_required: summary.layout_confirmation_required,
        stitch_layout: summary.stitch_layout.clone(),
        video_rendering: false,
        outputs: summary,
    })
}

#[tauri::command]
pub fn check_license() -> LicenseUiStatus {
    license_ui_status()
}

#[tauri::command]
pub fn activate_license(key: String) -> Result<(), String> {
    if private_build_enabled() {
        return Ok(());
    }
    if license::activate_license(key.trim()) {
        Ok(())
    } else {
        Err("Invalid license key.".into())
    }
}

#[tauri::command]
pub fn check_dependencies() -> deps::PreflightReport {
    deps::preflight_report()
}

#[tauri::command]
pub fn install_dependency(id: String) -> Result<String, String> {
    deps::install_dependency(&id)
}

#[tauri::command]
pub fn install_all_dependencies() -> Result<String, String> {
    deps::install_all_required()
}

#[tauri::command]
pub fn open_dependency_url(id: String) -> Result<String, String> {
    deps::open_dependency_url(&id)
}

#[tauri::command]
pub fn pick_input_file() -> Option<String> {
    rfd::FileDialog::new()
        .add_filter(
            "Sonar files",
            &["rsd", "sl2", "sl3", "dat", "son", "xtf", "jsf", "svlog", "bin"],
        )
        .pick_file()
        .map(|p| p.display().to_string())
}

#[tauri::command]
pub fn pick_folder() -> Option<String> {
    rfd::FileDialog::new()
        .pick_folder()
        .map(|p| p.display().to_string())
}

#[tauri::command]
pub fn get_host_profile() -> host_profile::HostProfile {
    host_profile::probe_host()
}

#[tauri::command]
pub fn get_suggested_settings(
    file_name: Option<String>,
    output_dir: Option<String>,
    tier: Option<String>,
) -> Result<SuggestedSettings, String> {
    host_profile::init_runtime();
    let tier = tier
        .as_deref()
        .map(SettingsTier::parse)
        .unwrap_or(SettingsTier::Auto);

    let survey = if let Some(ref path_str) = file_name {
        let path = PathBuf::from(path_str);
        if path.exists() {
            let detected = format_detector::detect_and_parse(&path);
            SonarSurveyHint {
                ping_count: detected.parse.pings.len(),
                format: detected.format.to_string(),
                hardware: None,
            }
        } else {
            SonarSurveyHint::default()
        }
    } else {
        SonarSurveyHint::default()
    };

    let out = output_dir.as_deref().map(Path::new);
    Ok(host_profile::suggest_settings(tier, &survey, out))
}

#[tauri::command]
pub async fn run_sonar_pipeline(
    app: AppHandle,
    file_name: String,
    options: PipelineOptions,
) -> Result<PipelineRunResult, String> {
    tauri::async_runtime::spawn_blocking(move || run_pipeline_blocking(app, file_name, options))
        .await
        .map_err(|e| format!("Pipeline thread failed: {e}"))?
}
