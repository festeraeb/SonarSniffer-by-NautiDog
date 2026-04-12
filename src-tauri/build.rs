fn main() {
    println!("cargo:rerun-if-env-changed=SONARSNIFFER_PRIVATE_BUILD");
    println!("cargo:rerun-if-env-changed=SONARSNIFFER_LICENSE_EMAIL");

    let private_build =
        std::env::var("SONARSNIFFER_PRIVATE_BUILD").unwrap_or_else(|_| "0".to_string());
    let license_email = std::env::var("SONARSNIFFER_LICENSE_EMAIL")
        .unwrap_or_else(|_| "support@nautidogsailing.com".to_string());

    println!("cargo:rustc-env=SONARSNIFFER_PRIVATE_BUILD={private_build}");
    println!("cargo:rustc-env=SONARSNIFFER_LICENSE_EMAIL={license_email}");

    tauri_build::build()
}
