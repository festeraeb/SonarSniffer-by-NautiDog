use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::model::ExportCandidate;

pub fn write_kmz(
    kmz_path: &Path,
    candidates: &[ExportCandidate],
    thumbs_dir: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let kml = build_kml(candidates);
    let file = File::create(kmz_path)?;
    let mut zip = ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("doc.kml", opts)?;
    zip.write_all(kml.as_bytes())?;

    if let Some(dir) = thumbs_dir {
        for c in candidates {
            if let Some(name) = &c.thumb_png {
                let thumb_path = dir.join("_export_thumbs").join(name);
                if thumb_path.is_file() {
                    let zip_name = format!("files/{name}");
                    zip.start_file(&zip_name, opts)?;
                    let mut f = File::open(&thumb_path)?;
                    let mut buf = Vec::new();
                    f.read_to_end(&mut buf)?;
                    zip.write_all(&buf)?;
                }
            }
        }
    }

    zip.finish()?;
    write_network_link(kmz_path)?;
    Ok(())
}

fn write_network_link(kmz_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let link_path = kmz_path.with_extension("network_link.kml");
    let kmz_name = kmz_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("targets.kmz");
    let body = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
  <NetworkLink>
    <name>Straits targets (auto-refresh)</name>
    <Link>
      <href>{kmz_name}</href>
      <refreshMode>onInterval</refreshMode>
      <refreshInterval>60</refreshInterval>
    </Link>
  </NetworkLink>
</kml>
"#
    );
    std::fs::write(link_path, body)?;
    Ok(())
}

fn build_kml(candidates: &[ExportCandidate]) -> String {
    let mut out = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
<Document>
  <name>CESAROPS targets</name>
"#,
    );

    for folder in [
        ("Physical", "bag_physical"),
        ("Masked", "bag_masked"),
        ("Satellite", "satellite"),
        ("GroundTruth", "ground_truth"),
    ] {
        let subset: Vec<_> = candidates
            .iter()
            .filter(|c| c.source == folder.1)
            .collect();
        if subset.is_empty() {
            continue;
        }
        out.push_str(&format!("<Folder><name>{}</name>\n", folder.0));
        for c in subset {
            out.push_str(&placemark(c));
        }
        out.push_str("</Folder>\n");
    }

    out.push_str("</Document></kml>\n");
    out
}

fn placemark(c: &ExportCandidate) -> String {
    let color = source_color(&c.source);
    let desc = html_description(c);
    let thumb = c
        .thumb_png
        .as_ref()
        .map(|n| format!("<br/><img src=\"files/{n}\" width=\"256\"/>"))
        .unwrap_or_default();
    format!(
        r#"<Placemark>
  <name>{name}</name>
  <styleUrl>#style_{src}</styleUrl>
  <Style id="style_{src}">
    <IconStyle><color>{color}</color><scale>1.1</scale></IconStyle>
  </Style>
  <description><![CDATA[{desc}{thumb}]]></description>
  <Point><coordinates>{lon},{lat},0</coordinates></Point>
</Placemark>
"#,
        name = xml_escape(&c.id),
        src = xml_escape(&c.source),
        color = color,
        desc = desc,
        thumb = thumb,
        lon = c.lon,
        lat = c.lat,
    )
}

fn source_color(source: &str) -> &'static str {
    match source {
        "ground_truth" => "ff00ff00",
        "bag_physical" => "ff00ffff",
        "bag_masked" => "ff0000ff",
        "satellite" => "ffff0000",
        _ => "ffaaaaaa",
    }
}

fn html_description(c: &ExportCandidate) -> String {
    let mut rows = vec![
        ("id", c.id.clone()),
        ("source", c.source.clone()),
        ("confidence", format!("{:.3}", c.confidence)),
    ];
    if let Some(v) = c.long_ft {
        rows.push(("long_ft", format!("{:.1}", v)));
    }
    if let Some(v) = c.short_ft {
        rows.push(("short_ft", format!("{:.1}", v)));
    }
    if let Some(v) = c.depth_ft {
        rows.push(("depth_ft", format!("{:.1}", v)));
    }
    if let Some(v) = c.relief_ft {
        rows.push(("relief_ft", format!("{:.1}", v)));
    }
    if let Some(s) = &c.signature {
        rows.push(("signature", s.clone()));
    }
    rows.push(("notes", c.notes.clone()));

    let mut html = String::from("<table border=\"1\" cellpadding=\"4\">");
    for (k, v) in rows {
        html.push_str(&format!(
            "<tr><th>{}</th><td>{}</td></tr>",
            xml_escape(k),
            xml_escape(&v)
        ));
    }
    html.push_str("</table>");
    html
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
