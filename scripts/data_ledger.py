#!/usr/bin/env python3
"""CesarOps data ledger — track what we've ACQUIRED and PROCESSED.

A crash-resilient, append-only record of every scene/granule on disk and what
detection stages have been run against it. Answers: "do we already have this?",
"what's our coverage (sensor x season x year)?", "what still needs processing?"
— so we never re-pull or re-process, and a drive crash can't take the
bookkeeping (it's regenerable by re-scanning disk + the JSONL is plaintext).

Ledger lives at: data/ledger/data_ledger.jsonl  (one JSON object per line)
  {kind:"scene", sensor, scene_id, date, bands:[...], path, bytes, source,
   seen_utc}
  {kind:"processed", scene_id|stack_id, stage, status, ts, notes}

Commands:
  scan   <root...>          # discover scenes on disk, append new ones
  mark   --scene ID --stage poc|temporal|bathy|sar --status ok|fail [--notes ..]
  status                    # coverage summary (sensor x year x season)
  pending --stage poc       # scenes acquired but not yet processed for a stage
"""
import argparse, json, os, re, sys, time, glob
from collections import defaultdict, Counter

LEDGER_DIR = "data/ledger"
LEDGER = os.path.join(LEDGER_DIR, "data_ledger.jsonl")

# Sentinel-2 band-tile naming: S2A_16TFR_20240928_0_L2A.blue.tif
S2_RE = re.compile(r"(S2[AB]_\w+?_(\d{8})_\d+_L2A)\.(\w+)\.tif$")

def season_of(month):
    return {12:"winter",1:"winter",2:"winter",3:"spring",4:"spring",5:"spring",
            6:"summer",7:"summer",8:"summer",9:"fall",10:"fall",11:"fall"}.get(month,"?")

def load_ledger():
    rows = []
    if os.path.exists(LEDGER):
        for line in open(LEDGER):
            line = line.strip()
            if line:
                try: rows.append(json.loads(line))
                except json.JSONDecodeError: pass
    return rows

def append(obj):
    os.makedirs(LEDGER_DIR, exist_ok=True)
    obj.setdefault("seen_utc", time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()))
    with open(LEDGER, "a") as f:
        f.write(json.dumps(obj) + "\n")

def known_scene_ids(rows):
    return {r["scene_id"] for r in rows if r.get("kind") == "scene"}

def cmd_scan(args):
    rows = load_ledger()
    known = known_scene_ids(rows)
    # group S2 band files by scene
    scenes = defaultdict(lambda: {"bands": [], "bytes": 0, "date": None, "path": None})
    for root in args.roots:
        for tif in glob.glob(os.path.join(root, "**", "*.tif"), recursive=True):
            m = S2_RE.search(os.path.basename(tif))
            if not m:
                continue
            sid, date, band = m.group(1), m.group(2), m.group(3)
            s = scenes[sid]
            s["bands"].append(band)
            s["bytes"] += os.path.getsize(tif)
            s["date"] = date
            s["path"] = os.path.dirname(tif)
    new = 0
    for sid, s in sorted(scenes.items()):
        if sid in known:
            continue
        append({"kind": "scene", "sensor": "sentinel2", "scene_id": sid,
                "date": s["date"], "season": season_of(int(s["date"][4:6])),
                "year": int(s["date"][:4]), "bands": sorted(set(s["bands"])),
                "path": s["path"], "bytes": s["bytes"], "source": "element84_aws"})
        new += 1
    print(f"scan: {len(scenes)} scenes on disk, {new} new added to ledger "
          f"({len(known)+new} total tracked)")

def cmd_mark(args):
    append({"kind": "processed", "scene_id": args.scene, "stage": args.stage,
            "status": args.status, "ts": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "notes": args.notes or ""})
    print(f"marked {args.scene} {args.stage}={args.status}")

def cmd_status(args):
    rows = load_ledger()
    scenes = [r for r in rows if r.get("kind") == "scene"]
    proc = [r for r in rows if r.get("kind") == "processed"]
    print(f"=== DATA LEDGER STATUS ===")
    print(f"scenes tracked: {len(scenes)}  total bytes: {sum(s.get('bytes',0) for s in scenes)/1e9:.1f} GB")
    cov = Counter((s.get("sensor"), s.get("year"), s.get("season")) for s in scenes)
    print("\ncoverage (sensor / year / season):")
    for (sensor, year, season), n in sorted(cov.items(), key=lambda x: (x[0][0], -(x[0][1] or 0))):
        print(f"  {sensor:10s} {year} {season:6s}  {n} scene(s)")
    # processed stage tallies
    by_stage = Counter((p.get("stage"), p.get("status")) for p in proc)
    if by_stage:
        print("\nprocessed:")
        for (stage, status), n in sorted(by_stage.items()):
            print(f"  {stage:10s} {status:5s}  {n}")

def cmd_pending(args):
    rows = load_ledger()
    scenes = {r["scene_id"] for r in rows if r.get("kind") == "scene"}
    done = {r["scene_id"] for r in rows
            if r.get("kind") == "processed" and r.get("stage") == args.stage
            and r.get("status") == "ok"}
    pending = sorted(scenes - done)
    print(f"pending for stage '{args.stage}': {len(pending)} of {len(scenes)} scenes")
    for sid in pending:
        print("  ", sid)

def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("scan"); p.add_argument("roots", nargs="+"); p.set_defaults(fn=cmd_scan)
    p = sub.add_parser("mark")
    p.add_argument("--scene", required=True); p.add_argument("--stage", required=True)
    p.add_argument("--status", required=True); p.add_argument("--notes")
    p.set_defaults(fn=cmd_mark)
    p = sub.add_parser("status"); p.set_defaults(fn=cmd_status)
    p = sub.add_parser("pending"); p.add_argument("--stage", required=True); p.set_defaults(fn=cmd_pending)
    args = ap.parse_args()
    args.fn(args)

if __name__ == "__main__":
    main()
