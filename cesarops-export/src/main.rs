mod db;
mod kml;
mod model;
mod thumb;

use std::path::PathBuf;

use clap::Parser;

#[derive(Parser)]
#[command(name = "cesarops-export")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Ingest JSON reports, upsert SQLite, write KMZ.
    Ingest {
        /// BAG MissionReport JSON and/or known_wrecks_straits.json paths.
        #[arg(required = true)]
        inputs: Vec<PathBuf>,
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        kmz: PathBuf,
        /// Directory with *_hillshade.tif / *_recon.tif for thumbnails.
        #[arg(long)]
        thumbs_dir: Option<PathBuf>,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("cesarops-export: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Ingest {
            inputs,
            db,
            kmz,
            thumbs_dir,
        } => ingest(&inputs, &db, &kmz, thumbs_dir.as_deref()),
    }
}

fn ingest(
    inputs: &[PathBuf],
    db_path: &PathBuf,
    kmz_path: &PathBuf,
    thumbs_dir: Option<&std::path::Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut all = Vec::new();
    for path in inputs {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.contains("known_wrecks") {
            all.extend(model::parse_ground_truth(path)?);
        } else {
            all.extend(model::parse_bag_report(path)?);
        }
    }

    if let Some(dir) = thumbs_dir {
        thumb::attach_thumbnails(&mut all, dir)?;
    }

    let conn = db::open(db_path)?;
    for c in &all {
        db::upsert(&conn, c)?;
    }
    eprintln!("Upserted {} candidates -> {}", all.len(), db_path.display());

    let loaded = db::load_all(&conn)?;
    kml::write_kmz(kmz_path, &loaded, thumbs_dir)?;
    eprintln!("Wrote KMZ {} ({} placemarks)", kmz_path.display(), loaded.len());
    Ok(())
}
