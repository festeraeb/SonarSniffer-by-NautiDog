//! Rust replacement for `pipelines/satellite/probe_sources.py`.

use clap::Parser;
use reqwest::Client;

#[derive(Parser, Debug)]
#[command(name = "source-probe", about = "Probe satellite source endpoints")]
struct Args {
    /// Timeout per request in seconds
    #[arg(long, default_value_t = 10)]
    timeout_s: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(args.timeout_s))
        .build()?;

    let groups: Vec<(&str, Vec<(&str, &str)>)> = vec![
        (
            "NRCan GDR",
            vec![("GRD index", "https://gdr.agg.nrcan.gc.ca/pub/gdr/GRD/")],
        ),
        (
            "NOAA NCEI",
            vec![
                (
                    "WMM anomaly grid",
                    "https://www.ncei.noaa.gov/products/earth-magnetic-model-anomaly-grid",
                ),
                (
                    "EMAG2 dataset",
                    "https://data.noaa.gov/dataset/dataset/emag2-earth-magnetic-anomaly-grid-2-arc-minute-resolution-version-3",
                ),
                ("Grid extract", "https://www.ncei.noaa.gov/maps/grid-extract/"),
            ],
        ),
        (
            "USGS Magnetic",
            vec![
                ("NAmag_origmrg.zip", "https://mrdata.usgs.gov/magnetic/NAmag_origmrg.zip"),
                ("USmag_origmrg.zip", "https://mrdata.usgs.gov/magnetic/USmag_origmrg.zip"),
                ("NAmag_hp500.zip", "https://mrdata.usgs.gov/magnetic/NAmag_hp500.zip"),
            ],
        ),
        (
            "WDMAM",
            vec![
                ("WDMAM2_v2_XYZ.zip", "https://wdmam.org/WDMAM2_v2_XYZ.zip"),
                ("WDMAM2_xyz.zip", "https://wdmam.org/download/WDMAM2_xyz.zip"),
            ],
        ),
    ];

    for (group, urls) in groups {
        println!("\n=== {group} ===");
        for (name, url) in urls {
            let start = std::time::Instant::now();
            let resp = match client.head(url).send().await {
                Ok(r) => Ok(r),
                Err(_) => client.get(url).send().await,
            };
            match resp {
                Ok(r) => {
                    let code = r.status().as_u16();
                    let cl = r
                        .headers()
                        .get(reqwest::header::CONTENT_LENGTH)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|s| s.parse::<u64>().ok())
                        .map(|n| format!("{:.1}MB", n as f64 / 1024.0 / 1024.0))
                        .unwrap_or_else(|| "?".to_string());
                    println!(
                        "  {code:>3} {:>10} {:>5}ms  {name}  {url}",
                        cl,
                        start.elapsed().as_millis()
                    );
                }
                Err(e) => {
                    println!("  ERR {:>16}  {name}  {url}", e);
                }
            }
        }
    }

    Ok(())
}
