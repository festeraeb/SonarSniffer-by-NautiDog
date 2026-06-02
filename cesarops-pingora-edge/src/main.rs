//! Role-based HTTP reverse proxy (Rust-first edge scaffold for Nomad `pingora-edge` job).

use anyhow::{Context, Result};
use axum::{
    extract::{Request, State},
    http::{Method, StatusCode},
    response::Response,
    routing::any,
    Router,
};
use clap::Parser;
use serde::Deserialize;
use std::{net::SocketAddr, sync::Arc};
use tower_http::trace::TraceLayer;
use tracing::info;

#[derive(Parser, Debug)]
#[command(name = "cesarops-pingora-edge")]
struct Cli {
    #[arg(long)]
    config: String,
}

#[derive(Debug, Deserialize)]
struct Config {
    server: ServerConfig,
    routes: Vec<RouteConfig>,
}

#[derive(Debug, Deserialize)]
struct ServerConfig {
    listen: String,
}

#[derive(Debug, Deserialize, Clone)]
struct RouteConfig {
    path_prefix: String,
    upstream: String,
}

#[derive(Clone)]
struct AppState {
    routes: Arc<Vec<RouteConfig>>,
    client: reqwest::Client,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "cesarops_pingora_edge=info".into()),
        )
        .init();

    let cli = Cli::parse();
    let raw = std::fs::read_to_string(&cli.config)
        .with_context(|| format!("read config {}", cli.config))?;
    let cfg: Config = toml::from_str(&raw).context("parse routes.toml")?;

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let routes = Arc::new(cfg.routes);
    info!(listen = %cfg.server.listen, routes = routes.len(), "starting edge router");

    let state = AppState { routes, client };
    let app = Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }))
        .fallback(any(proxy_handler))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr: SocketAddr = cfg.server.listen.parse().context("parse listen address")?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!(%addr, "listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn proxy_handler(State(state): State<AppState>, req: Request) -> Response {
    let path = req.uri().path().to_string();
    let route = match state
        .routes
        .iter()
        .find(|r| path.starts_with(r.path_prefix.trim_end_matches('/')))
    {
        Some(r) => r.clone(),
        None => {
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(axum::body::Body::from("no route"))
                .unwrap();
        }
    };

    let prefix = route.path_prefix.trim_end_matches('/');
    let remainder = path.strip_prefix(prefix).unwrap_or("").trim_start_matches('/');
    let upstream_base = route.upstream.trim_end_matches('/');
    let mut target = if remainder.is_empty() {
        upstream_base.to_string()
    } else {
        format!("{upstream_base}/{remainder}")
    };
    if let Some(q) = req.uri().query() {
        target = format!("{target}?{q}");
    }

    let method = req.method().clone();
    let headers = req.headers().clone();
    let body = match axum::body::to_bytes(req.into_body(), usize::MAX).await {
        Ok(b) => b,
        Err(e) => {
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(axum::body::Body::from(format!("body error: {e}")))
                .unwrap();
        }
    };

    let mut rb = state.client.request(method.clone(), &target);
    for (k, v) in headers.iter() {
        if k == axum::http::header::HOST {
            continue;
        }
        if let Ok(s) = v.to_str() {
            rb = rb.header(k.as_str(), s);
        }
    }
    if method != Method::GET && method != Method::HEAD {
        rb = rb.body(body.to_vec());
    }

    match rb.send().await {
        Ok(upstream) => {
            let status = StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let resp_headers: Vec<(String, String)> = upstream
                .headers()
                .iter()
                .filter_map(|(k, v)| v.to_str().ok().map(|s| (k.as_str().to_string(), s.to_string())))
                .collect();
            let bytes = upstream.bytes().await.unwrap_or_default();
            let mut builder = Response::builder().status(status);
            for (k, v) in resp_headers {
                builder = builder.header(k, v);
            }
            builder
                .body(axum::body::Body::from(bytes))
                .unwrap_or_else(|_| {
                    Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(axum::body::Body::empty())
                        .unwrap()
                })
        }
        Err(e) => Response::builder()
            .status(StatusCode::BAD_GATEWAY)
            .body(axum::body::Body::from(format!("upstream error: {e}")))
            .unwrap(),
    }
}
