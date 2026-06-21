mod commands;

use commands::{
    activate_license, check_dependencies, check_license, get_host_profile, get_suggested_settings,
    install_all_dependencies, install_dependency, open_dependency_url, pick_folder, pick_input_file,
    run_sonar_pipeline,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            check_license,
            activate_license,
            check_dependencies,
            get_host_profile,
            get_suggested_settings,
            install_dependency,
            install_all_dependencies,
            open_dependency_url,
            pick_input_file,
            pick_folder,
            run_sonar_pipeline,
        ])
        .run(tauri::generate_context!())
        .expect("error while running SonarSniffer desktop");
}
