#!/usr/bin/env python3
"""Apply the Deep View side-scan ground-truth verdict to the Straits known-wrecks
file: the BAG-unmask 'masked target / Robert Burns' candidate ~0.4 nm SE of the
Elva is GEOLOGY (parallel ridges), not wreckage.

Flips the two bag_unmask_candidate entries to ruled-out negatives and records a
structured verdict for ML labeling. Backs up the original first.
"""
import json, shutil, datetime, sys

PATHS = [
    "/data/repos/wreckhunter2000-1/scripts/known_wrecks_straits.json",
    "/data/repos/wreckhunter2000-1/data/known_wrecks_straits.json",
]

VERDICT = {
    "gt_status": "ruled_out",
    "gt_method": "side_scan_sonar (Deep View mosaic)",
    "gt_date": "2026-06-03",
    "gt_finding": "geology: parallel ridges, no wreckage in mosaic",
    "gt_note": "Covered ~0.4 nm SE of the Elva. BAG-unmask cap was glacial channel "
               "geology aligned with the thalweg, not a hull. Retained as a HARD "
               "NEGATIVE for ML (interior relief + channel-aligned ridges fooled "
               "shape tests; azimuth-alignment-with-channel was the true tell).",
}

for p in PATHS:
    try:
        d = json.load(open(p))
    except FileNotFoundError:
        print("skip (missing):", p); continue
    changed = []
    for k, v in d.items():
        if v.get("confidence") == "bag_unmask_candidate":
            v["confidence"] = "side_scan_ruled_out"
            v["type"] = "geology_ridges"
            v["ml_label"] = 0  # hard negative
            v.update(VERDICT)
            old = v.get("notes", "")
            v["notes"] = "[RULED OUT by Deep View side-scan 2026-06-03 — geology, not wreckage] " + old
            changed.append(k)
    if changed:
        bak = p + ".bak_" + datetime.datetime.now().strftime("%Y%m%d_%H%M%S")
        shutil.copy2(p, bak)
        json.dump(d, open(p, "w"), indent=2)
        print("updated %s  (%d entries: %s)  backup=%s" % (p, len(changed), ",".join(changed), bak))
    else:
        print("no bag_unmask_candidate entries in", p)
