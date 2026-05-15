use axum::{routing::get, Router, Json};
use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter("cesarops=info").init();
    info!("CESARops SAR Platform starting...");

    cesarops_lib::dispatch::init();
    cesarops_lib::mapping::init();
    cesarops_lib::tracking::init();
    cesarops_lib::reporting::init();
    cesarops_lib::admin::init();

    let app = Router::new()
        .route("/health", get(|| async { Json(serde_json::json!({"status": "ok", "service": "cesarops"})) }));

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 9200));
    info!("CESARops listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
