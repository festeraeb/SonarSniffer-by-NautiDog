//! Research Ingestion Specialist
//!
//! Scrapes arXiv and Semantic Scholar for papers on SAR, remote sensing,
//! shipwreck detection, and maritime search & rescue. Stores findings in
//! a Sled database (research.db), separate from the anomaly pipeline.
//!
//! During idle / --synthesize mode it calls KoboldCpp (REASONING_BASE_URL)
//! to extract actionable insights: detector improvements, WGSL shader ideas,
//! fine-tuning datasets, and new pipeline passes for CESAROPS.
//!
//! Usage:
//!   research_ingestion_specialist                  # fetch new papers
//!   research_ingestion_specialist --synthesize     # LLM pass on stored papers
//!   research_ingestion_specialist --daemon         # fetch + synthesize loop

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::Parser;
use reqwest::Client;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use sled::Db;
use std::time::Duration;
use tracing::{info, warn};

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(about = "CESAROPS Research Ingestion Specialist")]
struct Args {
    /// Run LLM synthesis on stored papers (idle-time mode)
    #[arg(long)]
    synthesize: bool,

    /// Continuous daemon: fetch every --interval-minutes, synthesize in idle gaps
    #[arg(long)]
    daemon: bool,

    /// Fetch interval in daemon mode (minutes)
    #[arg(long, default_value_t = 60)]
    interval_minutes: u64,

    /// Sled database path
    #[arg(long, default_value = "research.db")]
    db_path: String,

    /// Max results per query
    #[arg(long, default_value_t = 20)]
    max_results: usize,

    /// KoboldCpp / reasoning LLM endpoint
    #[arg(long, env = "REASONING_BASE_URL", default_value = "http://localhost:5001/v1")]
    llm_url: String,

    /// Extra webpage URLs to scrape for papers (can be specified multiple times)
    /// Example: --url http://192.168.1.50:8080/papers  --url https://example.org/sar-research
    #[arg(long = "url", value_name = "URL")]
    extra_urls: Vec<String>,
}

// ── Data model ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ResearchSource {
    ArXiv,
    SemanticScholar,
    /// Generic webpage scrape — base URL stored as string
    Webpage(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ResearchStatus {
    Fetched,
    PdfExtracted,
    SynthesisComplete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchRecord {
    pub id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub abstract_text: String,
    pub pdf_url: Option<String>,
    /// First 8 000 chars of extracted PDF text (None if PDF unavailable)
    pub pdf_text: Option<String>,
    pub source: ResearchSource,
    pub ingested_at: DateTime<Utc>,
    pub keywords: Vec<String>,
    /// 0.0–1.0 relevance heuristic based on keyword matching
    pub relevance_score: f32,
    pub synthesis_notes: Option<String>,
    pub status: ResearchStatus,
}

// ── Sled wrapper ──────────────────────────────────────────────────────────────

pub struct ResearchDb {
    db: Db,
}

impl ResearchDb {
    pub fn open(path: &str) -> Result<Self> {
        Ok(Self { db: sled::open(path)? })
    }

    pub fn insert(&self, rec: &ResearchRecord) -> Result<()> {
        let val = serde_json::to_vec(rec)?;
        self.db.insert(rec.id.as_bytes(), val)?;
        self.db.flush()?;
        Ok(())
    }

    pub fn contains(&self, id: &str) -> Result<bool> {
        Ok(self.db.contains_key(id.as_bytes())?)
    }

    pub fn all_pending_synthesis(&self) -> Result<Vec<ResearchRecord>> {
        let mut out = Vec::new();
        for item in self.db.iter() {
            let (_, val) = item?;
            if let Ok(rec) = serde_json::from_slice::<ResearchRecord>(&val) {
                if rec.status != ResearchStatus::SynthesisComplete {
                    out.push(rec);
                }
            }
        }
        Ok(out)
    }

    pub fn len(&self) -> usize {
        self.db.len()
    }
}

// ── Relevance scoring ─────────────────────────────────────────────────────────

fn score_relevance(text: &str) -> f32 {
    let t = text.to_lowercase();
    let mut score = 0.0f32;

    // Directly mission-critical
    for kw in &[
        "shipwreck", "ship wreck", "wreck detection", "submerged vessel",
        "underwater wreck", "galvanic", "corner reflector wreck",
    ] {
        if t.contains(kw) { score += 0.40; }
    }
    // Core sensor/technique overlap
    for kw in &[
        "synthetic aperture radar", "sentinel-1", "sar slick",
        "maritime anomaly", "oil slick detection", "bathymetry",
        "icesat", "swot", "hyperspectral", "multispectral anomaly",
        "search and rescue", "maritime surveillance", "seafloor",
        "subsurface detection", "underwater object",
    ] {
        if t.contains(kw) { score += 0.15; }
    }
    // Broader supporting topics
    for kw in &[
        "remote sensing", "anomaly detection", "sar", "sentinel",
        "spectral analysis", "edge detection", "curvelet",
        "fine-tuning", "quantization", "gguf", "wgpu", "wgsl",
        "deep learning", "marine", "coastal",
    ] {
        if t.contains(kw) { score += 0.05; }
    }

    score.min(1.0)
}

fn extract_keywords(text: &str) -> Vec<String> {
    let t = text.to_lowercase();
    [
        "SAR", "Sentinel-1", "Sentinel-2", "Landsat", "ICESat-2", "SWOT",
        "synthetic aperture radar", "multispectral", "hyperspectral",
        "bathymetry", "shipwreck", "anomaly detection", "deep learning",
        "fine-tuning", "search and rescue", "maritime", "oil slick",
        "corner reflector", "curvelet", "spectral", "InSAR", "LiDAR",
        "wgpu", "WGSL",
    ]
    .iter()
    .filter(|&&kw| t.contains(&kw.to_lowercase()))
    .map(|&s| s.to_string())
    .collect()
}

// ── arXiv Atom XML parser (via roxmltree) ─────────────────────────────────────

fn parse_arxiv_atom(xml: &str) -> Vec<ArxivEntry> {
    let doc = match roxmltree::Document::parse(xml) {
        Ok(d) => d,
        Err(e) => { warn!("arXiv XML parse error: {}", e); return vec![]; }
    };

    doc.descendants()
        .filter(|n| n.has_tag_name("entry"))
        .filter_map(|entry| {
            let id = entry.descendants()
                .find(|n| n.has_tag_name("id"))
                .and_then(|n| n.text())
                .map(|s| s.trim().to_string())?;

            let title = entry.descendants()
                .find(|n| n.has_tag_name("title"))
                .and_then(|n| n.text())
                .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))?;

            let abstract_text = entry.descendants()
                .find(|n| n.has_tag_name("summary"))
                .and_then(|n| n.text())
                .map(|s| s.trim().to_string())
                .unwrap_or_default();

            let authors: Vec<String> = entry.descendants()
                .filter(|n| n.has_tag_name("author"))
                .filter_map(|a| a.descendants().find(|n| n.has_tag_name("name")))
                .filter_map(|n| n.text())
                .map(|s| s.trim().to_string())
                .collect();

            let pdf_url = entry.descendants()
                .filter(|n| n.has_tag_name("link"))
                .find(|n| n.attribute("title") == Some("pdf"))
                .and_then(|n| n.attribute("href"))
                .map(|s| s.to_string());

            Some(ArxivEntry { id, title, abstract_text, authors, pdf_url })
        })
        .collect()
}

struct ArxivEntry {
    id: String,
    title: String,
    abstract_text: String,
    authors: Vec<String>,
    pdf_url: Option<String>,
}

async fn fetch_arxiv(client: &Client, query: &str, max: usize) -> Result<Vec<ResearchRecord>> {
    let encoded = urlencoding::encode(query);
    let url = format!(
        "https://export.arxiv.org/api/query?search_query={}&max_results={}&sortBy=relevance",
        encoded, max
    );
    info!("arXiv: {}", query);

    let xml = client.get(&url)
        .timeout(Duration::from_secs(30))
        .send().await.context("arXiv fetch")?
        .text().await?;

    let mut records = Vec::new();
    for entry in parse_arxiv_atom(&xml) {
        let combined = format!("{} {}", entry.title, entry.abstract_text);
        let relevance = score_relevance(&combined);
        if relevance < 0.05 { continue; }

        let arxiv_id = entry.id
            .trim_start_matches("http://arxiv.org/abs/")
            .trim_start_matches("https://arxiv.org/abs/")
            .to_string();

        records.push(ResearchRecord {
            id: format!("arxiv:{}", arxiv_id),
            title: entry.title,
            authors: entry.authors,
            abstract_text: entry.abstract_text,
            pdf_url: entry.pdf_url,
            pdf_text: None,
            source: ResearchSource::ArXiv,
            ingested_at: Utc::now(),
            keywords: extract_keywords(&combined),
            relevance_score: relevance,
            synthesis_notes: None,
            status: ResearchStatus::Fetched,
        });
    }
    Ok(records)
}

// ── Semantic Scholar API ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct S2Response {
    data: Option<Vec<S2Paper>>,
}

#[derive(Debug, Deserialize)]
struct S2Paper {
    #[serde(rename = "paperId")]
    paper_id: String,
    title: Option<String>,
    #[serde(rename = "abstract")]
    abstract_text: Option<String>,
    authors: Option<Vec<S2Author>>,
    #[serde(rename = "openAccessPdf")]
    open_access_pdf: Option<S2PdfLink>,
}

#[derive(Debug, Deserialize)]
struct S2Author {
    name: String,
}

#[derive(Debug, Deserialize)]
struct S2PdfLink {
    url: String,
}

async fn fetch_semantic_scholar(client: &Client, query: &str, max: usize) -> Result<Vec<ResearchRecord>> {
    let encoded = urlencoding::encode(query);
    let url = format!(
        "https://api.semanticscholar.org/graph/v1/paper/search\
         ?query={}&limit={}&fields=title,abstract,authors,openAccessPdf",
        encoded, max.min(100)
    );
    info!("Semantic Scholar: {}", query);

    let resp: S2Response = client.get(&url)
        .timeout(Duration::from_secs(30))
        .header("User-Agent", "CESAROPS-Research-Agent/1.0")
        .send().await.context("Semantic Scholar fetch")?
        .json().await.context("Semantic Scholar parse")?;

    let mut records = Vec::new();
    for paper in resp.data.unwrap_or_default() {
        let title = paper.title.unwrap_or_default();
        let abstract_text = paper.abstract_text.unwrap_or_default();
        let combined = format!("{} {}", title, abstract_text);
        let relevance = score_relevance(&combined);
        if relevance < 0.05 { continue; }

        let authors: Vec<String> = paper.authors.unwrap_or_default()
            .into_iter().map(|a| a.name).collect();

        records.push(ResearchRecord {
            id: format!("s2:{}", paper.paper_id),
            title,
            authors,
            abstract_text,
            pdf_url: paper.open_access_pdf.map(|p| p.url),
            pdf_text: None,
            source: ResearchSource::SemanticScholar,
            ingested_at: Utc::now(),
            keywords: extract_keywords(&combined),
            relevance_score: relevance,
            synthesis_notes: None,
            status: ResearchStatus::Fetched,
        });
    }
    Ok(records)
}

// ── Generic webpage scraper ───────────────────────────────────────────────────
//
// Walks all <a href> links on the page looking for:
//   - PDF links    → download + extract text
//   - arXiv links  → route through fetch_arxiv id lookup
//   - HTML pages   → extract visible text from <p> / <article> / <section>
//
// Each discovered item becomes a ResearchRecord scored by the relevance heuristic.
// Only items scoring > 0.05 are stored.

async fn fetch_webpage(client: &Client, base_url: &str) -> Result<Vec<ResearchRecord>> {
    info!("Webpage scrape: {}", base_url);

    let html_text = client.get(base_url)
        .timeout(Duration::from_secs(30))
        .header("User-Agent", "CESAROPS-Research-Agent/1.0")
        .send().await.context("webpage fetch")?
        .text().await?;

    let doc = Html::parse_document(&html_text);

    // Collect all visible text to score the page itself
    let body_text = {
        let sel = Selector::parse("p, article, section, h1, h2, h3, li").unwrap();
        doc.select(&sel)
            .map(|el| el.text().collect::<Vec<_>>().join(" "))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let mut records = Vec::new();
    let link_sel = Selector::parse("a[href]").unwrap();

    // Deduplicate hrefs
    let mut seen_hrefs = std::collections::HashSet::new();

    for link in doc.select(&link_sel) {
        let href = match link.value().attr("href") {
            Some(h) if !h.is_empty() => h,
            _ => continue,
        };

        // Resolve relative URLs
        let full_url = if href.starts_with("http://") || href.starts_with("https://") {
            href.to_string()
        } else if href.starts_with('/') {
            // Absolute path on same host
            let base = base_url.trim_end_matches('/');
            // Strip to just scheme+host
            let host = base.splitn(4, '/').take(3).collect::<Vec<_>>().join("/");
            format!("{}{}", host, href)
        } else {
            continue; // skip anchors, javascript:, mailto:, etc.
        };

        if !seen_hrefs.insert(full_url.clone()) { continue; }

        let link_text = link.text().collect::<Vec<_>>().join(" ");
        let anchor_score = score_relevance(&link_text);

        if full_url.ends_with(".pdf") || full_url.contains("/pdf/") {
            // Direct PDF link
            if anchor_score < 0.03 && score_relevance(&body_text) < 0.05 { continue; }

            let pdf_text = try_extract_pdf(client, &full_url).await;
            let content = pdf_text.clone().unwrap_or_else(|| link_text.clone());
            let relevance = score_relevance(&content).max(anchor_score);
            if relevance < 0.05 { continue; }

            let title = link_text.trim().to_string();
            let title = if title.is_empty() {
                full_url.split('/').last().unwrap_or("untitled").to_string()
            } else { title };

            let id = format!("web:{}:{}", urlencoding::encode(base_url), urlencoding::encode(&full_url));
            records.push(ResearchRecord {
                id,
                title,
                authors: vec![],
                abstract_text: content.chars().take(2_000).collect(),
                pdf_url: Some(full_url.clone()),
                pdf_text,
                source: ResearchSource::Webpage(base_url.to_string()),
                ingested_at: Utc::now(),
                keywords: extract_keywords(&content),
                relevance_score: relevance,
                synthesis_notes: None,
                status: ResearchStatus::PdfExtracted,
            });

        } else if full_url.contains("arxiv.org/abs/") || full_url.contains("arxiv.org/pdf/") {
            // ArXiv link found on a webpage — extract the ID and let the arXiv fetcher handle it
            let arxiv_id = full_url
                .trim_end_matches('/')
                .split('/')
                .last()
                .unwrap_or("")
                .to_string();
            if arxiv_id.is_empty() { continue; }

            let api_url = format!(
                "https://export.arxiv.org/api/query?id_list={}&max_results=1",
                arxiv_id
            );
            match client.get(&api_url).timeout(Duration::from_secs(15)).send().await {
                Ok(r) => {
                    if let Ok(xml) = r.text().await {
                        let mut fetched = parse_arxiv_atom(&xml)
                            .into_iter()
                            .map(|e| {
                                let combined = format!("{} {}", e.title, e.abstract_text);
                                let relevance = score_relevance(&combined);
                                ResearchRecord {
                                    id: format!("arxiv:{}", arxiv_id),
                                    title: e.title,
                                    authors: e.authors,
                                    abstract_text: e.abstract_text,
                                    pdf_url: e.pdf_url,
                                    pdf_text: None,
                                    source: ResearchSource::ArXiv,
                                    ingested_at: Utc::now(),
                                    keywords: extract_keywords(&combined),
                                    relevance_score: relevance,
                                    synthesis_notes: None,
                                    status: ResearchStatus::Fetched,
                                }
                            })
                            .filter(|r| r.relevance_score > 0.05)
                            .collect::<Vec<_>>();
                        records.append(&mut fetched);
                    }
                }
                Err(e) => warn!("arXiv lookup failed for {}: {}", arxiv_id, e),
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        // Other HTML pages: skip to avoid unbounded crawling
    }

    // Also score the page body as a single record if it's clearly relevant
    let page_score = score_relevance(&body_text);
    if page_score >= 0.10 && records.is_empty() {
        let title = {
            let title_sel = Selector::parse("title, h1").unwrap();
            doc.select(&title_sel)
                .next()
                .map(|el| el.text().collect::<Vec<_>>().join(" "))
                .unwrap_or_else(|| base_url.to_string())
        };
        let id = format!("web:{}", urlencoding::encode(base_url));
        records.push(ResearchRecord {
            id,
            title,
            authors: vec![],
            abstract_text: body_text.chars().take(4_000).collect(),
            pdf_url: None,
            pdf_text: None,
            source: ResearchSource::Webpage(base_url.to_string()),
            ingested_at: Utc::now(),
            keywords: extract_keywords(&body_text),
            relevance_score: page_score,
            synthesis_notes: None,
            status: ResearchStatus::Fetched,
        });
    }

    info!("Webpage scrape of {} yielded {} candidates", base_url, records.len());
    Ok(records)
}

// ── PDF extraction ─────────────────────────────────────────────────────────────

async fn try_extract_pdf(client: &Client, url: &str) -> Option<String> {
    let bytes = client.get(url)
        .timeout(Duration::from_secs(60))
        .header("User-Agent", "CESAROPS-Research-Agent/1.0")
        .send().await.ok()?
        .bytes().await.ok()?;

    pdf_extract::extract_text_from_mem(&bytes).ok()
        .map(|t| t.chars().take(8_000).collect())
}

// ── LLM synthesis ─────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMsg>,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Serialize, Deserialize)]
struct ChatMsg {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMsg,
}

async fn synthesize_paper(client: &Client, llm_url: &str, rec: &ResearchRecord) -> Result<String> {
    // Prefer full PDF text; fall back to abstract
    let text: String = rec.pdf_text.as_deref()
        .unwrap_or(&rec.abstract_text)
        .chars().take(4_000).collect();

    let system = "You are a remote sensing research analyst for CESAROPS — a shipwreck \
        detection and maritime search-and-rescue system running on a distributed GPU cluster. \
        Analyze papers for:\n\
        1. New or improved techniques for SAR slick / wreck detection\n\
        2. Spectral or bathymetry pipeline improvements\n\
        3. Fine-tuning dataset ideas for Qwen or Llama GGUF models\n\
        4. Rust/WGSL compute shader algorithms worth implementing\n\
        5. Any novel feature that could feed into the AnomalyQueue\n\
        Be concise, actionable, code-oriented.";

    let user = format!(
        "Paper: {}\nAuthors: {}\nKeywords: {}\n\nContent excerpt:\n{}\n\n\
        Synthesize: what techniques or ideas are useful for CESAROPS? \
        Include specific implementation notes where possible.",
        rec.title,
        rec.authors.join(", "),
        rec.keywords.join(", "),
        text,
    );

    let url = format!("{}/chat/completions", llm_url.trim_end_matches('/'));
    let payload = ChatRequest {
        model: "qwen2.5-coder",
        messages: vec![
            ChatMsg { role: "system".into(), content: system.into() },
            ChatMsg { role: "user".into(), content: user },
        ],
        max_tokens: 512,
        temperature: 0.3,
    };

    let resp: ChatResponse = client.post(&url)
        .timeout(Duration::from_secs(120))
        .json(&payload)
        .send().await.context("LLM synthesis request")?
        .json().await.context("LLM synthesis parse")?;

    Ok(resp.choices.into_iter()
        .next()
        .map(|c| c.message.content)
        .unwrap_or_default())
}

// ── Fetch loop ────────────────────────────────────────────────────────────────

const ARXIV_QUERIES: &[&str] = &[
    "all:SAR AND all:shipwreck AND all:detection",
    "all:synthetic aperture radar AND all:maritime AND all:anomaly",
    "all:Sentinel-1 AND all:oil slick",
    "all:bathymetry AND all:shipwreck",
    "all:remote sensing AND all:underwater AND all:object detection",
    "all:search and rescue AND all:satellite AND all:detection",
    "all:spectral anomaly AND all:water",
    "all:hyperspectral AND all:marine AND all:anomaly",
    "all:ICESat-2 AND all:seafloor",
    "all:curvelet AND all:SAR",
];

const S2_QUERIES: &[&str] = &[
    "SAR shipwreck detection remote sensing",
    "synthetic aperture radar maritime surveillance anomaly",
    "multispectral bathymetry wreck detection",
    "search rescue satellite imagery deep learning",
    "hyperspectral ocean anomaly detection",
    "SAR oil slick corner reflector",
];

async fn run_fetch(client: &Client, db: &ResearchDb, max: usize, extra_urls: &[String]) {
    let mut new_count = 0usize;
    let mut skip_count = 0usize;

    for query in ARXIV_QUERIES {
        match fetch_arxiv(client, query, max).await {
            Err(e) => warn!("arXiv '{}' error: {}", query, e),
            Ok(records) => {
                for mut rec in records {
                    match db.contains(&rec.id) {
                        Ok(true)  => { skip_count += 1; continue; }
                        Err(e)    => { warn!("DB error: {}", e); continue; }
                        Ok(false) => {}
                    }
                    // Try PDF for open-access arXiv papers
                    if let Some(ref url) = rec.pdf_url.clone() {
                        if let Some(text) = try_extract_pdf(client, url).await {
                            info!("PDF extracted {} chars for {}", text.len(), rec.id);
                            rec.pdf_text = Some(text);
                            rec.status = ResearchStatus::PdfExtracted;
                        }
                    }
                    info!("[{:.2}] {}", rec.relevance_score, rec.title);
                    if db.insert(&rec).is_ok() { new_count += 1; }
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(600)).await;
    }

    for query in S2_QUERIES {
        match fetch_semantic_scholar(client, query, max).await {
            Err(e) => warn!("S2 '{}' error: {}", query, e),
            Ok(records) => {
                for mut rec in records {
                    match db.contains(&rec.id) {
                        Ok(true)  => { skip_count += 1; continue; }
                        Err(e)    => { warn!("DB error: {}", e); continue; }
                        Ok(false) => {}
                    }
                    if let Some(ref url) = rec.pdf_url.clone() {
                        if let Some(text) = try_extract_pdf(client, url).await {
                            rec.pdf_text = Some(text);
                            rec.status = ResearchStatus::PdfExtracted;
                        }
                    }
                    info!("[{:.2}] {}", rec.relevance_score, rec.title);
                    if db.insert(&rec).is_ok() { new_count += 1; }
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(600)).await;
    }

    // ── Extra webpage sources ─────────────────────────────────────────────────
    for url in extra_urls {
        match fetch_webpage(client, url).await {
            Err(e) => warn!("Webpage '{}' error: {}", url, e),
            Ok(records) => {
                for mut rec in records {
                    match db.contains(&rec.id) {
                        Ok(true)  => { skip_count += 1; continue; }
                        Err(e)    => { warn!("DB error: {}", e); continue; }
                        Ok(false) => {}
                    }
                    // If it's a PDF link without text yet, try to extract
                    if rec.pdf_text.is_none() {
                        if let Some(ref pdf_url) = rec.pdf_url.clone() {
                            if let Some(text) = try_extract_pdf(client, pdf_url).await {
                                rec.pdf_text = Some(text);
                                rec.status = ResearchStatus::PdfExtracted;
                            }
                        }
                    }
                    info!("[{:.2}] {}", rec.relevance_score, rec.title);
                    if db.insert(&rec).is_ok() { new_count += 1; }
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    println!(
        "[research] Fetch done: {} new, {} already stored. Total in DB: {}",
        new_count, skip_count, db.len()
    );
}

// ── Synthesis loop ────────────────────────────────────────────────────────────

async fn run_synthesis(client: &Client, db: &ResearchDb, llm_url: &str) {
    let mut pending = match db.all_pending_synthesis() {
        Ok(p) => p,
        Err(e) => { warn!("DB read error: {}", e); return; }
    };

    if pending.is_empty() {
        println!("[synthesis] No papers pending — all synthesized.");
        return;
    }

    // Highest relevance first
    pending.sort_by(|a, b| b.relevance_score.partial_cmp(&a.relevance_score).unwrap_or(std::cmp::Ordering::Equal));

    println!("[synthesis] {} papers pending. Running top 10 by relevance...", pending.len());

    for mut rec in pending.into_iter().take(10) {
        info!("Synthesizing: {} (score={:.2})", rec.title, rec.relevance_score);
        match synthesize_paper(client, llm_url, &rec).await {
            Ok(notes) => {
                println!("\n┌─ {} ─────────────────────", rec.id);
                println!("│ Score: {:.2}  Keywords: {}", rec.relevance_score, rec.keywords.join(", "));
                println!("│ {}", rec.title);
                println!("├─────────────────────────────────────────────────");
                for line in notes.lines() {
                    println!("│ {}", line);
                }
                println!("└─────────────────────────────────────────────────\n");

                rec.synthesis_notes = Some(notes);
                rec.status = ResearchStatus::SynthesisComplete;
                if let Err(e) = db.insert(&rec) {
                    warn!("DB update error: {}", e);
                }
            }
            Err(e) => warn!("Synthesis failed for {}: {}", rec.id, e),
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    println!("[synthesis] Pass complete.");
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    let db = ResearchDb::open(&args.db_path)?;
    let client = Client::builder()
        .user_agent("CESAROPS-Research-Agent/1.0")
        .build()?;

    // Prefer env vars over CLI arg
    let llm_url = std::env::var("REASONING_BASE_URL")
        .or_else(|_| std::env::var("LLM_BASE_URL"))
        .unwrap_or_else(|_| args.llm_url.clone());

    println!("=== CESAROPS Research Ingestion Specialist ===");
    println!("  DB:      {}", args.db_path);
    println!("  LLM:     {}", llm_url);
    println!("  Stored:  {} papers", db.len());

    if args.daemon {
        println!("  Mode:    daemon (fetch every {} min)", args.interval_minutes);
        loop {
            run_fetch(&client, &db, args.max_results, &args.extra_urls).await;
            run_synthesis(&client, &db, &llm_url).await;
            println!("[daemon] Sleeping {} minutes...", args.interval_minutes);
            tokio::time::sleep(Duration::from_secs(args.interval_minutes * 60)).await;
        }
    } else if args.synthesize {
        println!("  Mode:    synthesis only");
        run_synthesis(&client, &db, &llm_url).await;
    } else {
        println!("  Mode:    fetch");
        run_fetch(&client, &db, args.max_results, &args.extra_urls).await;
    }

    Ok(())
}
