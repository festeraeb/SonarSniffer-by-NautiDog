/// research_engine.rs — Autonomous research loop for sovereign-cloud idle time.
///
/// ANTI-HALLUCINATION DESIGN:
///   - Every finding MUST have a source URL or DOI. No URL = not stored.
///   - The LLM is only used to extract structured fields from real abstracts.
///   - Hypotheses are proposals to run actual pipeline tasks, not assertions.
///   - All stored findings are tagged with their source and a confidence that
///     reflects citation count / recency, not LLM confidence.
///
/// Research domains (rotated each cycle):
///   1. Wreck detection — satellite, SAR, magnetic, sonar fusion
///   2. Hydrocarbon recognition — SWIR suppression, slick spectral signatures
///   3. Turbidity / bubble compensation — optical depth correction, backscatter
///   4. Search and rescue remote sensing — drift prediction, thermal detection
///   5. SAR processing — speckle, coherence, change detection
///   6. Bathymetry — IHO standards, uncertainty, shallow-water limits

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

// ── Research finding (always source-grounded) ─────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchFinding {
    pub id: String,
    pub domain: ResearchDomain,
    pub title: String,
    pub authors: Vec<String>,
    pub year: u32,
    pub source_url: String,       // mandatory — no URL = not stored
    pub doi: Option<String>,
    pub abstract_snippet: String, // first 500 chars of real abstract
    pub extracted_technique: Option<String>, // LLM-extracted, clearly labelled
    pub hypothesis: Option<ResearchHypothesis>,
    pub citation_count: u32,
    pub ingested_at: i64,
}

// ── Hypothesis — a testable proposal, not an assertion ───────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchHypothesis {
    pub description: String,
    /// Concrete pipeline parameters to test this hypothesis
    pub suggested_band_weights: Option<Vec<f32>>,
    pub suggested_confidence_threshold: Option<f32>,
    pub suggested_scan_region: Option<(f64, f64, f64, f64)>, // lat_min, lat_max, lon_min, lon_max
    pub status: HypothesisStatus,
    pub test_result: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum HypothesisStatus {
    Proposed,
    Queued,
    Tested,
    Rejected,
    Confirmed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ResearchDomain {
    WreckDetection,
    HydrocarbonRecognition,
    TurbidityCompensation,
    SearchAndRescue,
    SarProcessing,
    Bathymetry,
}

impl ResearchDomain {
    fn all() -> Vec<Self> {
        vec![
            Self::WreckDetection,
            Self::HydrocarbonRecognition,
            Self::TurbidityCompensation,
            Self::SearchAndRescue,
            Self::SarProcessing,
            Self::Bathymetry,
        ]
    }

    fn arxiv_query(&self) -> &str {
        match self {
            Self::WreckDetection =>
                "ti:shipwreck OR ti:\"submerged vessel\" OR (ti:SAR AND ti:wreck) OR (ti:magnetometer AND ti:anomaly AND ti:marine)",
            Self::HydrocarbonRecognition =>
                "ti:\"oil spill\" AND (ti:SWIR OR ti:spectral OR ti:SAR) AND ti:detection",
            Self::TurbidityCompensation =>
                "ti:turbidity AND (ti:correction OR ti:compensation) AND (ti:bathymetry OR ti:remote sensing)",
            Self::SearchAndRescue =>
                "ti:\"search and rescue\" AND (ti:satellite OR ti:SAR OR ti:drift) AND ti:maritime",
            Self::SarProcessing =>
                "ti:SAR AND (ti:ship OR ti:vessel OR ti:maritime) AND (ti:detection OR ti:classification)",
            Self::Bathymetry =>
                "ti:bathymetry AND (ti:satellite OR ti:ICESat OR ti:SWOT) AND (ti:shallow OR ti:coastal)",
        }
    }

    fn semantic_scholar_query(&self) -> &str {
        match self {
            Self::WreckDetection => "shipwreck detection SAR magnetometer submerged vessel sonar",
            Self::HydrocarbonRecognition => "oil spill SWIR spectral SAR detection ocean satellite",
            Self::TurbidityCompensation => "turbidity correction bathymetry optical depth remote sensing",
            Self::SearchAndRescue => "maritime search rescue drift prediction SAR thermal satellite",
            Self::SarProcessing => "SAR ship detection vessel classification Sentinel-1 maritime",
            Self::Bathymetry => "satellite derived bathymetry ICESat-2 shallow water coastal",
        }
    }
}

// ── Research state ────────────────────────────────────────────────────────────
pub struct ResearchEngine {
    pub findings: Arc<RwLock<Vec<ResearchFinding>>>,
    pub hypotheses: Arc<RwLock<Vec<ResearchHypothesis>>>,
    domain_idx: Arc<RwLock<usize>>,
    llm_base: String,
    db_path: String,
}

impl ResearchEngine {
    pub fn new(llm_base: String, db_path: String) -> Self {
        Self {
            findings: Arc::new(RwLock::new(Vec::new())),
            hypotheses: Arc::new(RwLock::new(Vec::new())),
            domain_idx: Arc::new(RwLock::new(0)),
            llm_base,
            db_path,
        }
    }

    /// Run one research cycle — queries sources, stores grounded findings,
    /// proposes hypotheses. Called from idle_scout when mode includes Research.
    pub async fn run_cycle(&self) -> Result<usize> {
        let domains = ResearchDomain::all();
        let idx = {
            let mut i = self.domain_idx.write().await;
            let current = *i;
            *i = (current + 1) % domains.len();
            current
        };
        let domain = &domains[idx];
        info!("ResearchEngine: cycle for domain {:?}", domain);

        let mut new_findings = 0usize;

        // Query arXiv
        match self.query_arxiv(domain).await {
            Ok(papers) => {
                info!("ResearchEngine: arXiv returned {} papers", papers.len());
                for paper in papers {
                    if self.store_finding(paper).await {
                        new_findings += 1;
                    }
                }
            }
            Err(e) => warn!("ResearchEngine: arXiv query failed: {}", e),
        }

        // Query Semantic Scholar
        match self.query_semantic_scholar(domain).await {
            Ok(papers) => {
                info!("ResearchEngine: SemanticScholar returned {} papers", papers.len());
                for paper in papers {
                    if self.store_finding(paper).await {
                        new_findings += 1;
                    }
                }
            }
            Err(e) => warn!("ResearchEngine: SemanticScholar query failed: {}", e),
        }

        // Query Google Custom Search if API key is set
        if let (Ok(key), Ok(cx)) = (
            std::env::var("GOOGLE_CSE_KEY"),
            std::env::var("GOOGLE_CSE_CX"),
        ) {
            match self.query_google_cse(domain, &key, &cx).await {
                Ok(results) => {
                    info!("ResearchEngine: Google CSE returned {} results", results.len());
                    for r in results {
                        if self.store_finding(r).await {
                            new_findings += 1;
                        }
                    }
                }
                Err(e) => warn!("ResearchEngine: Google CSE failed: {}", e),
            }
        }

        // Generate hypotheses from new findings using LLM
        if new_findings > 0 {
            self.generate_hypotheses(domain).await;
        }

        // Persist to disk
        self.persist().await;

        info!("ResearchEngine: cycle complete — {} new findings", new_findings);
        Ok(new_findings)
    }

    // ── arXiv query ───────────────────────────────────────────────────────────
    async fn query_arxiv(&self, domain: &ResearchDomain) -> Result<Vec<ResearchFinding>> {
        let query = urlencoding::encode(domain.arxiv_query());
        let url = format!(
            "https://export.arxiv.org/api/query?search_query=all:{}&start=0&max_results=5&sortBy=submittedDate&sortOrder=descending",
            query
        );

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .user_agent("CESARops-ResearchEngine/1.0 (cesarops.com)")
            .build()?;

        let resp = client.get(&url).send().await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("arXiv returned {}", resp.status()));
        }

        let xml = resp.text().await?;
        let findings = parse_arxiv_xml(&xml, domain);
        Ok(findings)
    }

    // ── Semantic Scholar query ────────────────────────────────────────────────
    async fn query_semantic_scholar(&self, domain: &ResearchDomain) -> Result<Vec<ResearchFinding>> {
        let query = urlencoding::encode(domain.semantic_scholar_query());
        let url = format!(
            "https://api.semanticscholar.org/graph/v1/paper/search?query={}&limit=5&fields=title,authors,year,externalIds,abstract,citationCount,openAccessPdf",
            query
        );

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .user_agent("CESARops-ResearchEngine/1.0")
            .build()?;

        // Semantic Scholar has a public API — no key needed for basic search
        // but rate-limited to 100 req/5min. We run at most once per 5 min cycle.
        let resp = client.get(&url).send().await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("SemanticScholar returned {}", resp.status()));
        }

        let body: serde_json::Value = resp.json().await?;
        let findings = parse_semantic_scholar(&body, domain);
        Ok(findings)
    }

    // ── Google Custom Search Engine ───────────────────────────────────────────
    // Set GOOGLE_CSE_KEY and GOOGLE_CSE_CX in .env to enable.
    // Create a CSE at https://programmablesearchengine.google.com/
    // pointing at scholar.google.com, researchgate.net, mdpi.com, etc.
    async fn query_google_cse(
        &self,
        domain: &ResearchDomain,
        api_key: &str,
        cx: &str,
    ) -> Result<Vec<ResearchFinding>> {
        let query = urlencoding::encode(domain.arxiv_query());
        let url = format!(
            "https://www.googleapis.com/customsearch/v1?key={}&cx={}&q={}&num=5",
            api_key, cx, query
        );

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()?;

        let resp = client.get(&url).send().await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("Google CSE returned {}", resp.status()));
        }

        let body: serde_json::Value = resp.json().await?;
        let findings = parse_google_cse(&body, domain);
        Ok(findings)
    }

    // ── Store finding (dedup by URL, require source_url) ─────────────────────
    async fn store_finding(&self, finding: ResearchFinding) -> bool {
        // ANTI-HALLUCINATION: reject anything without a real source URL
        if finding.source_url.is_empty() || finding.source_url == "unknown" {
            warn!("ResearchEngine: rejected finding '{}' — no source URL", finding.title);
            return false;
        }

        let mut findings = self.findings.write().await;

        // Dedup by URL
        if findings.iter().any(|f| f.source_url == finding.source_url) {
            return false;
        }

        info!("ResearchEngine: stored '{}' [{}]", finding.title, finding.source_url);
        findings.push(finding);

        // Cap buffer at 500 findings, drop oldest
        if findings.len() > 500 {
            findings.remove(0);
        }
        true
    }

    // ── Hypothesis generation via LLM ─────────────────────────────────────────
    // The LLM is given ONLY real abstracts and asked to propose testable
    // parameter changes. It cannot assert findings — only suggest experiments.
    async fn generate_hypotheses(&self, domain: &ResearchDomain) {
        let findings = self.findings.read().await;
        let recent: Vec<&ResearchFinding> = findings.iter()
            .filter(|f| f.domain == *domain)
            .rev()
            .take(3)
            .collect();

        if recent.is_empty() { return; }

        // Build a grounded prompt — only real abstracts, no invented content
        let abstracts = recent.iter()
            .map(|f| format!(
                "Title: {}\nSource: {}\nAbstract: {}\n",
                f.title, f.source_url, f.abstract_snippet
            ))
            .collect::<Vec<_>>()
            .join("\n---\n");

        let prompt = format!(
            "You are analyzing real research papers about {:?}. \
            Based ONLY on the abstracts below (do not invent facts), \
            suggest ONE specific testable change to our detection pipeline. \
            Format your response as JSON with fields: \
            description (string), suggested_band_weights (array of 7 floats or null), \
            suggested_confidence_threshold (float 0-1 or null). \
            Do not assert findings — only propose an experiment.\n\n{}",
            domain, abstracts
        );

        let llm_url = format!("{}/chat/completions",
            self.llm_base.trim_end_matches('/'));
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_default();

        let payload = serde_json::json!({
            "model": "default",
            "messages": [{"role": "user", "content": prompt}],
        });

        match client.post(&llm_url).json(&payload).send().await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(body) = resp.json::<serde_json::Value>().await {
                    let content = body["choices"][0]["message"]["content"]
                        .as_str().unwrap_or("").to_string();

                    // Parse the JSON the LLM returned
                    if let Ok(h) = serde_json::from_str::<serde_json::Value>(&content) {
                        let hypothesis = ResearchHypothesis {
                            description: h["description"].as_str()
                                .unwrap_or("(no description)").to_string(),
                            suggested_band_weights: h["suggested_band_weights"]
                                .as_array()
                                .map(|a| a.iter()
                                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                                    .collect()),
                            suggested_confidence_threshold: h["suggested_confidence_threshold"]
                                .as_f64().map(|f| f as f32),
                            suggested_scan_region: None,
                            status: HypothesisStatus::Proposed,
                            test_result: None,
                        };
                        info!("ResearchEngine: hypothesis proposed — {}", hypothesis.description);
                        self.hypotheses.write().await.push(hypothesis);
                    }
                }
            }
            Ok(resp) => warn!("ResearchEngine: LLM returned {}", resp.status()),
            Err(e) => warn!("ResearchEngine: LLM unreachable: {}", e),
        }
    }

    // ── Persist findings to disk ──────────────────────────────────────────────
    async fn persist(&self) {
        let findings = self.findings.read().await;
        let hypotheses = self.hypotheses.read().await;
        let data = serde_json::json!({
            "findings": *findings,
            "hypotheses": *hypotheses,
            "updated_at": chrono::Utc::now().timestamp(),
        });
        if let Ok(json) = serde_json::to_string_pretty(&data) {
            let _ = std::fs::write(&self.db_path, json);
        }
    }

    /// Load previously persisted findings from disk on startup
    pub fn load_from_disk(db_path: &str) -> (Vec<ResearchFinding>, Vec<ResearchHypothesis>) {
        if let Ok(data) = std::fs::read_to_string(db_path) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
                let findings = serde_json::from_value(v["findings"].clone())
                    .unwrap_or_default();
                let hypotheses = serde_json::from_value(v["hypotheses"].clone())
                    .unwrap_or_default();
                return (findings, hypotheses);
            }
        }
        (Vec::new(), Vec::new())
    }

    /// Get the most recent findings for the API response
    pub async fn recent_findings(&self, limit: usize) -> Vec<ResearchFinding> {
        let findings = self.findings.read().await;
        findings.iter().rev().take(limit).cloned().collect()
    }

    /// Get all proposed hypotheses
    pub async fn pending_hypotheses(&self) -> Vec<ResearchHypothesis> {
        let hypotheses = self.hypotheses.read().await;
        hypotheses.iter()
            .filter(|h| h.status == HypothesisStatus::Proposed)
            .cloned()
            .collect()
    }
}

// ── arXiv XML parser ──────────────────────────────────────────────────────────
fn parse_arxiv_xml(xml: &str, domain: &ResearchDomain) -> Vec<ResearchFinding> {
    let mut findings = Vec::new();

    // Simple XML extraction without a full parser — arXiv Atom feed is regular
    for entry in xml.split("<entry>").skip(1) {
        let title = extract_xml_text(entry, "title")
            .unwrap_or_default()
            .replace('\n', " ")
            .trim()
            .to_string();
        let abstract_text = extract_xml_text(entry, "summary")
            .unwrap_or_default()
            .trim()
            .to_string();
        let arxiv_id = extract_xml_attr(entry, "id")
            .unwrap_or_default();
        let source_url = if arxiv_id.contains("arxiv.org") {
            arxiv_id.clone()
        } else {
            format!("https://arxiv.org/abs/{}", arxiv_id.trim())
        };

        // Extract year from published date
        let year = extract_xml_text(entry, "published")
            .and_then(|d| d[..4].parse::<u32>().ok())
            .unwrap_or(2024);

        // Extract authors
        let authors: Vec<String> = entry.split("<name>")
            .skip(1)
            .filter_map(|s| s.split("</name>").next().map(|n| n.trim().to_string()))
            .take(3)
            .collect();

        if title.is_empty() || source_url.is_empty() { continue; }

        findings.push(ResearchFinding {
            id: format!("arxiv-{}", uuid_ts()),
            domain: domain.clone(),
            title,
            authors,
            year,
            source_url,
            doi: None,
            abstract_snippet: abstract_text.chars().take(500).collect(),
            extracted_technique: None,
            hypothesis: None,
            citation_count: 0,
            ingested_at: chrono::Utc::now().timestamp(),
        });
    }
    findings
}

// ── Semantic Scholar JSON parser ──────────────────────────────────────────────
fn parse_semantic_scholar(body: &serde_json::Value, domain: &ResearchDomain) -> Vec<ResearchFinding> {
    let mut findings = Vec::new();
    let papers = match body["data"].as_array() {
        Some(p) => p,
        None => return findings,
    };

    for paper in papers {
        let title = paper["title"].as_str().unwrap_or("").to_string();
        if title.is_empty() { continue; }

        // Build source URL — prefer open access PDF, fall back to S2 page
        let source_url = paper["openAccessPdf"]["url"]
            .as_str()
            .map(|s| s.to_string())
            .or_else(|| {
                paper["externalIds"]["DOI"].as_str()
                    .map(|doi| format!("https://doi.org/{}", doi))
            })
            .or_else(|| {
                paper["paperId"].as_str()
                    .map(|id| format!("https://www.semanticscholar.org/paper/{}", id))
            })
            .unwrap_or_default();

        if source_url.is_empty() { continue; }

        let doi = paper["externalIds"]["DOI"].as_str().map(|s| s.to_string());
        let year = paper["year"].as_u64().unwrap_or(2024) as u32;
        let citation_count = paper["citationCount"].as_u64().unwrap_or(0) as u32;
        let abstract_text = paper["abstract"].as_str().unwrap_or("").to_string();
        let authors: Vec<String> = paper["authors"].as_array()
            .map(|a| a.iter()
                .filter_map(|au| au["name"].as_str().map(|s| s.to_string()))
                .take(3)
                .collect())
            .unwrap_or_default();

        findings.push(ResearchFinding {
            id: format!("s2-{}", uuid_ts()),
            domain: domain.clone(),
            title,
            authors,
            year,
            source_url,
            doi,
            abstract_snippet: abstract_text.chars().take(500).collect(),
            extracted_technique: None,
            hypothesis: None,
            citation_count,
            ingested_at: chrono::Utc::now().timestamp(),
        });
    }
    findings
}

// ── Google CSE JSON parser ────────────────────────────────────────────────────
fn parse_google_cse(body: &serde_json::Value, domain: &ResearchDomain) -> Vec<ResearchFinding> {
    let mut findings = Vec::new();
    let items = match body["items"].as_array() {
        Some(i) => i,
        None => return findings,
    };

    for item in items {
        let title = item["title"].as_str().unwrap_or("").to_string();
        let source_url = item["link"].as_str().unwrap_or("").to_string();
        let snippet = item["snippet"].as_str().unwrap_or("").to_string();

        if title.is_empty() || source_url.is_empty() { continue; }

        // Only accept URLs from known academic/research domains
        let trusted = ["arxiv.org", "doi.org", "semanticscholar.org",
                       "mdpi.com", "researchgate.net", "nature.com",
                       "sciencedirect.com", "ieee.org", "noaa.gov",
                       "usgs.gov", "esa.int", "nasa.gov"];
        if !trusted.iter().any(|d| source_url.contains(d)) {
            continue; // reject non-academic sources
        }

        findings.push(ResearchFinding {
            id: format!("cse-{}", uuid_ts()),
            domain: domain.clone(),
            title,
            authors: Vec::new(),
            year: 2024,
            source_url,
            doi: None,
            abstract_snippet: snippet.chars().take(500).collect(),
            extracted_technique: None,
            hypothesis: None,
            citation_count: 0,
            ingested_at: chrono::Utc::now().timestamp(),
        });
    }
    findings
}

// ── XML helpers ───────────────────────────────────────────────────────────────
fn extract_xml_text(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(xml[start..end].to_string())
}

fn extract_xml_attr(xml: &str, tag: &str) -> Option<String> {
    // For arXiv <id> which contains the URL directly
    extract_xml_text(xml, tag)
}

fn uuid_ts() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    format!("{:x}", SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos())
}
