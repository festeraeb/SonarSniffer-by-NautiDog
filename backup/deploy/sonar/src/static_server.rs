use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

pub struct StaticServer {
    pub url: String,
    running: Arc<AtomicBool>,
}

impl StaticServer {
    pub fn start(root: &Path) -> Result<Self, String> {
        let listener =
            TcpListener::bind("127.0.0.1:0").map_err(|e| format!("bind: {e}"))?;
        let port = listener
            .local_addr()
            .map_err(|e| format!("addr: {e}"))?
            .port();
        let url = format!("http://127.0.0.1:{port}");
        let running = Arc::new(AtomicBool::new(true));
        let run_flag = running.clone();
        let root_buf: PathBuf = root.to_path_buf();

        // Non-blocking so the accept loop can check `running`
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("nonblocking: {e}"))?;

        thread::spawn(move || {
            while run_flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let root = root_buf.clone();
                        thread::spawn(move || {
                            let _ = handle_request(&mut stream, &root);
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(std::time::Duration::from_millis(50));
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self { url, running })
    }

    pub fn shutdown(&self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

fn handle_request(stream: &mut std::net::TcpStream, root: &Path) -> std::io::Result<()> {
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf)?;
    let request = String::from_utf8_lossy(&buf[..n]);

    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");

    // Sanitize: prevent path traversal
    let decoded = percent_decode(path);
    let clean = decoded.trim_start_matches('/');
    let file_path = if clean.is_empty() {
        root.join("index.html")
    } else {
        root.join(clean)
    };

    // Ensure the resolved path is inside root
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let canonical_file = file_path
        .canonicalize()
        .unwrap_or_else(|_| file_path.clone());
    if !canonical_file.starts_with(&canonical_root) {
        let resp = "HTTP/1.1 403 Forbidden\r\nContent-Length: 9\r\n\r\nForbidden";
        stream.write_all(resp.as_bytes())?;
        return Ok(());
    }

    if file_path.is_file() {
        let body = std::fs::read(&file_path)?;
        let mime = guess_mime(&file_path);
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\n\r\n",
            body.len()
        );
        stream.write_all(header.as_bytes())?;
        stream.write_all(&body)?;
    } else {
        let resp = "HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\n\r\nNot Found";
        stream.write_all(resp.as_bytes())?;
    }
    Ok(())
}

fn guess_mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("js") => "application/javascript",
        Some("css") => "text/css",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("svg") => "image/svg+xml",
        Some("mbtiles") => "application/octet-stream",
        Some("pbf") => "application/x-protobuf",
        _ => "application/octet-stream",
    }
}

fn percent_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let hi = chars.next().unwrap_or(b'0');
            let lo = chars.next().unwrap_or(b'0');
            let val = hex_val(hi) * 16 + hex_val(lo);
            out.push(val as char);
        } else {
            out.push(b as char);
        }
    }
    out
}

fn hex_val(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}
