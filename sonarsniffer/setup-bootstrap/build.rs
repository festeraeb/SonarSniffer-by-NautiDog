use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR");
    let out = Path::new(&out_dir);

    if let Ok(src) = env::var("SONARSNIFFER_SETUP_PAYLOAD") {
        let src = Path::new(&src);
        if src.is_file() {
            let dest = out.join("payload.zip");
            fs::copy(src, &dest).expect("copy payload.zip to OUT_DIR");
            fs::write(
                out.join("embedded_payload.rs"),
                r#"include_bytes!("payload.zip")"#,
            )
            .expect("write embedded_payload.rs");
            println!("cargo:rerun-if-changed={}", src.display());
            println!("cargo:rustc-cfg=embed_payload");
        }
    }
}
