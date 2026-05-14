# SonarSniffer — User Guide

**by NautiDog Sailing**

SonarSniffer converts proprietary sonar log files into open, portable formats you can view, share, and analyse in standard GIS tools.

---

## Installation

### Windows

1. Download `SonarSniffer_x.x.x_x64-setup.exe` from the Releases page.
2. Double-click the installer and follow the prompts.
3. If Windows SmartScreen warns about an unknown publisher, click **More info → Run anyway**. (The app is unsigned for now; a code-signing certificate will be added in a future release.)
4. Launch *SonarSniffer* from the Start menu.

### macOS

1. Download `SonarSniffer_x.x.x_universal.dmg` from the Releases page. The universal build runs natively on both Intel and Apple Silicon Macs.
2. Open the `.dmg` file and drag **SonarSniffer** into your Applications folder.
3. The first time you open it, macOS Gatekeeper will block it because the app is unsigned. To allow it:
   - Open **System Settings → Privacy & Security**.
   - Scroll down to the blocked-app notice and click **Open Anyway**.
   - Alternatively, right-click the app icon in Finder and choose **Open**, then confirm.
4. SonarSniffer is now ready to use.

---

## Quick Start

1. **Open SonarSniffer.**
2. In the *Parser Pipeline* panel, click **Select Sonar File**.
3. Navigate to your sonar recording and select it. Supported file types:

   | Extension | Source |
   |-----------|--------|
   | `.RSD` | Garmin Striker / ECHOMAP |
   | `.sl2`, `.sl3` | Lowrance (HDS, HOOK, Elite) |
   | `.dat` | Humminbird (Helix, Solix, Apex) |
   | `.jsf` | EdgeTech (JSL Side-Scan) |
   | `.xtf` | Klein, Tritech, and other XTF-format echosounders |

4. Choose which outputs you want (all are enabled by default):

   | Option | What it creates |
   |--------|----------------|
   | **Video Export** | Animated GIF waterfall flythrough |
   | **KML Export** | Track + ping points for Google Earth |
   | **KMZ Export** | Self-contained zipped KML archive |
   | **MBTiles** | Tiled sonar mosaic for QGIS, ArcGIS, or the built-in viewer |
   | **Mosaic PNG** | Full-resolution stitched sonar image |
   | **Waterfall PNG** | Classic depth-scroll strip chart |
   | **ArcGIS Sidecar** | `arcgis_layer.json` for direct import into ArcGIS Pro / Online |
   | **Viewer Bundle** | Self-contained offline web map (MapLibre-based, no internet needed) |

5. Pick a **Colour palette** from the drop-down. *Amber* is the classic sonar look; *Ocean* and *Inferno* are popular for presentations.

6. Tick **Remove water column** if you want the surface blank-band stripped out for cleaner image exports.

7. Optionally click **Browse…** to choose where outputs land. If left blank, SonarSniffer creates a folder named after your file in the same directory.

8. Click **Run Parser**.

---

## Reading the Results Panel

After the run completes, the *Summary* panel shows:

- **Status** — green *Pipeline complete* = success; red = error with message.
- **Parse stats** — record count, sonar channels found, CRC mismatches, sync gaps, depth range, and water temperature (if recorded).
- **Output Files** — each generated file with an **Open** button that reveals it in Finder/Explorer.
- **Video Export** — progress is shown live during rendering; the final GIF path appears here.

---

## Offline Web Viewer

Every run with *Viewer Bundle* enabled produces a `viewer/` folder inside your output directory. To use it:

1. Open `viewer/index.html` in any modern browser (Chrome, Firefox, Edge, Safari).
2. An interactive map loads with your sonar mosaic overlaid on a base map tile layer and your GPS track drawn on top.
3. No internet is required — all map data and tiles are embedded in the bundle.

You can zip the `viewer/` folder and hand it to anyone; they just unzip and open `index.html`.

---

## Advanced: Firmware Analysis

Expand the **Advanced (Firmware Analysis)** panel to analyse a raw firmware binary and extract field maps, version strings, or ping-format signatures. This is a diagnostic tool aimed at developers adding new parser support; most users can ignore it.

---

## Advanced: Corpus Discovery

The **Corpus Discovery** panel scans a folder tree for sonar files across all supported formats and reports what it finds. Useful for inventorying a hard drive full of old recordings before batch-processing.

---

## Output Files Reference

| File | Description |
|------|-------------|
| `waterfall.png` | Vertical depth scroll, time on Y-axis |
| `mosaic.png` | Georeferenced sonar image stitched along the GPS track |
| `sonar.mbtiles` | Tiled raster for QGIS (`Add Raster Layer → MBTiles`), ArcGIS, or MapLibre |
| `track.kml` | GPS track + ping positions for Google Earth |
| `track.kmz` | Same as KML, zipped — better for sharing |
| `arcgis_layer.json` | ArcGIS feature layer sidecar (point features with depth attributes) |
| `viewer/index.html` | Self-contained offline map viewer |
| `sonar_waterfall.gif` | Animated flythrough of the waterfall |

---

## Troubleshooting

**"Please select an .RSD file" even after picking a file**
- Make sure the file picker dialog fully closed before clicking Run. Sometimes the dialog returns focus to the browser before the path is committed — click the file name in the picker and press **Open** explicitly.

**Parse completed but output files are missing**
- Check the *Sync gaps* count; a very high number means the file was heavily corrupted. Try a different sonar file to verify the tool works, then re-export the recording from your chartplotter.

**Depth range shows "—"**
- The file may not contain GPS-referenced depth pings (some older firmware versions store depth without position). The mosaic and KML files will still be created but won't have location data.

**Video export shows "GStreamer not available"**
- The standard build exports an animated GIF instead of MP4. Full GStreamer video support is a build-time feature that requires a GStreamer runtime and is not included in the pre-built installers.

**macOS: "SonarSniffer is damaged and can't be opened"**
- Run this once in Terminal, then relaunch: `xattr -cr /Applications/SonarSniffer.app`

---

## Uninstalling

- **Windows**: Settings → Apps → SonarSniffer → Uninstall.
- **macOS**: Drag SonarSniffer from Applications to the Trash.

No data is written outside your chosen output folder; uninstalling removes the app cleanly.

---

## Version History

| Version | Notes |
|---------|-------|
| 0.1.0 | Initial public test release |

---

*Questions or bug reports? Open an issue on the project repository.*
