//! Shared nautivecs / WSO URLs for think_harder and remember.

pub fn nautivecs_query_url() -> String {
    std::env::var("NAUTIVECS_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:5003/query".to_string())
}

pub fn nautivecs_add_url() -> String {
    let base = std::env::var("NAUTIVECS_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:5003/query".to_string());
    let base = base.trim_end_matches("/query").trim_end_matches("/search");
    format!("{}/add", base)
}

pub fn wso_url() -> String {
    std::env::var("WSO_URL").unwrap_or_else(|_| "http://127.0.0.1:5010/search".to_string())
}
