fn main() {
    // Only link edgetpu when the feature is explicitly requested
    // AND the library is actually present on this machine.
    if std::env::var("CARGO_FEATURE_EDGETPU").is_err() {
        return;
    }

    let lib_dirs = [
        "/usr/lib/x86_64-linux-gnu",
        "/usr/lib",
        "/lib/x86_64-linux-gnu",
        "/lib",
    ];

    for dir in &lib_dirs {
        let versioned = format!("{}/libedgetpu.so.1", dir);
        let unversioned = format!("{}/libedgetpu.so", dir);
        if std::path::Path::new(&versioned).exists() {
            if !std::path::Path::new(&unversioned).exists() {
                let _ = std::process::Command::new("ln")
                    .args(["-sf", &versioned, &unversioned])
                    .status();
            }
            println!("cargo:rustc-link-search=native={}", dir);
            println!("cargo:rustc-link-lib=dylib=edgetpu");
            return;
        }
    }

    eprintln!("build.rs: edgetpu feature requested but libedgetpu.so.1 not found — skipping TPU link");
}
