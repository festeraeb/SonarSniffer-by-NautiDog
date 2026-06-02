#!/usr/bin/env python3
"""
Production PDF Redaction Breaker
================================
Consolidates all redaction-breaking techniques into a single CLI tool with
CSV + JSON output.  Designed for the NOAA SHPO Feature Report PDFs.

Usage:
    python pdf_redaction_breaker.py                       # scan current dir
    python pdf_redaction_breaker.py -d /path/to/pdfs      # scan a folder
    python pdf_redaction_breaker.py -f report.pdf          # single file
    python pdf_redaction_breaker.py -d . --csv out.csv     # write CSV
"""

import argparse
import csv
import json
import os
import re
import sys
import io
import base64
from datetime import datetime
from pathlib import Path

import numpy as np

# ── optional heavy imports (degrade gracefully) ─────────────────────────────

try:
    import PyPDF2
    HAS_PYPDF2 = True
except ImportError:
    HAS_PYPDF2 = False

try:
    import fitz  # PyMuPDF
    HAS_FITZ = True
except ImportError:
    HAS_FITZ = False

try:
    from PIL import Image
    HAS_PIL = True
except ImportError:
    HAS_PIL = False

try:
    import cv2
    HAS_CV2 = True
except ImportError:
    HAS_CV2 = False

try:
    import pytesseract
    HAS_TESSERACT = True
except ImportError:
    HAS_TESSERACT = False

try:
    import easyocr
    HAS_EASYOCR = True
except ImportError:
    HAS_EASYOCR = False

try:
    import torch
    HAS_TORCH = True
except ImportError:
    HAS_TORCH = False

# ── target keywords ─────────────────────────────────────────────────────────

DEFAULT_TARGETS = [
    "Elva", "Griffon", "Cedarville", "Nordmeer",
    "ship", "vessel", "wreck", "maritime", "obstruction",
    "coordinates", "location", "position",
    "depth", "sonar", "bathymetric",
    "H13253", "H13254", "H13255", "H13364", "H13366",
    "H13367", "H13368", "H13369",
    "W00470", "W00555",
]


# ── core breaker class ──────────────────────────────────────────────────────

class PDFRedactionBreaker:
    """All-technique PDF redaction breaker with structured output."""

    def __init__(self, targets=None, save_images=True, output_dir="redaction_output", skip_ocr=False):
        self.targets = [t.lower() for t in (targets or DEFAULT_TARGETS)]
        self.save_images = save_images
        self.output_dir = output_dir
        self.skip_ocr = skip_ocr
        self._easyocr_reader = None   # lazy

    # ── public API ──────────────────────────────────────────────────────────

    def analyze(self, pdf_path: str) -> dict:
        """Run every available technique on a single PDF and return results dict."""
        pdf_path = str(pdf_path)
        basename = os.path.basename(pdf_path)
        print(f"\n{'='*70}")
        print(f"  {basename}")
        print(f"{'='*70}")

        result = {
            "file": basename,
            "path": pdf_path,
            "timestamp": datetime.now().isoformat(),
            "techniques": [],
            "text_blocks": [],
            "metadata": {},
            "images": [],
            "redaction_zones": [],
            "findings": [],
        }

        # 1  PyPDF2 text extraction (bypasses visual blocks)
        self._run(result, self._pypdf2_text,       pdf_path, "pypdf2_text")
        # 2  PyMuPDF advanced (fonts, images, annotations)
        self._run(result, self._pymupdf_extract,    pdf_path, "pymupdf_advanced")
        # 3  Metadata mining
        self._run(result, self._metadata_mining,    pdf_path, "metadata_mining")
        # 4  Content-stream analysis
        self._run(result, self._content_streams,    pdf_path, "content_streams")
        # 5  Enhanced content-stream mining
        self._run(result, self._enhanced_streams,   pdf_path, "enhanced_streams")
        # 6  Redaction-pattern detection
        self._run(result, self._redaction_patterns, pdf_path, "redaction_patterns")
        # 7  Image analysis + redaction recovery
        self._run(result, self._image_analysis,     pdf_path, "image_analysis")
        # 8  OCR on extracted images
        if not self.skip_ocr:
            self._run(result, self._ocr_images,         pdf_path, "ocr")

        # Target matching across all collected text
        result["findings"] = self._match_targets(result)

        ntex = len(result["text_blocks"])
        nimg = len(result["images"])
        nfnd = len(result["findings"])
        ntech = len(result["techniques"])
        print(f"  => {ntech} techniques | {ntex} text blocks | {nimg} images | {nfnd} target hits")
        return result

    def analyze_directory(self, directory: str) -> list:
        """Analyze every PDF under *directory* (recursive)."""
        pdfs = sorted(Path(directory).rglob("*.pdf"))
        if not pdfs:
            print(f"No PDFs found under {directory}")
            return []
        print(f"Found {len(pdfs)} PDFs under {directory}")
        return [self.analyze(str(p)) for p in pdfs]

    # ── writers ─────────────────────────────────────────────────────────────

    def write_json(self, results: list, path: str):
        with open(path, "w") as f:
            json.dump(results, f, indent=2, default=str)
        print(f"JSON  -> {path}")

    def write_csv(self, results: list, path: str):
        """Flatten findings into a CSV with columns:
        file, page, technique, target, context, confidence"""
        rows = []
        for res in results:
            for finding in res.get("findings", []):
                rows.append({
                    "file": res["file"],
                    "page": finding.get("page", ""),
                    "technique": finding.get("method", ""),
                    "target": finding.get("target", ""),
                    "context": finding.get("context", "")[:500],
                    "confidence": finding.get("confidence", ""),
                })
            # Also emit redaction zones
            for rz in res.get("redaction_zones", []):
                rows.append({
                    "file": res["file"],
                    "page": rz.get("page", ""),
                    "technique": rz.get("method", "redaction_zone"),
                    "target": "REDACTION_ZONE",
                    "context": json.dumps(rz.get("rect", [])),
                    "confidence": rz.get("confidence", ""),
                })

        with open(path, "w", newline="", encoding="utf-8") as f:
            writer = csv.DictWriter(f, fieldnames=["file", "page", "technique",
                                                     "target", "context", "confidence"])
            writer.writeheader()
            writer.writerows(rows)
        print(f"CSV   -> {path}  ({len(rows)} rows)")

    # ── technique runners ───────────────────────────────────────────────────

    def _run(self, result, fn, pdf_path, label):
        try:
            fn(result, pdf_path)
            result["techniques"].append(label)
        except Exception as e:
            print(f"  [{label}] failed: {e}")

    # -- 1. PyPDF2 text -------------------------------------------------------

    def _pypdf2_text(self, result, pdf_path):
        if not HAS_PYPDF2:
            return
        with open(pdf_path, "rb") as fh:
            reader = PyPDF2.PdfReader(fh)
            for page_num, page in enumerate(reader.pages, 1):
                text = page.extract_text() or ""
                for block in text.split("\n"):
                    block = block.strip()
                    if len(block) > 3:
                        result["text_blocks"].append({
                            "page": page_num, "method": "pypdf2", "content": block,
                            "confidence": "high",
                        })

    # -- 2. PyMuPDF advanced ---------------------------------------------------

    def _pymupdf_extract(self, result, pdf_path):
        if not HAS_FITZ:
            return
        doc = fitz.open(pdf_path)
        for page_num in range(len(doc)):
            page = doc[page_num]
            # text blocks
            for block in page.get_text("blocks"):
                text = block[4].strip() if len(block) > 4 else ""
                if len(text) > 3:
                    result["text_blocks"].append({
                        "page": page_num + 1, "method": "pymupdf_blocks",
                        "content": text, "confidence": "high",
                    })
            # images
            for img in page.get_images(full=True):
                xref = img[0]
                info = doc.extract_image(xref)
                if info:
                    result["images"].append({
                        "page": page_num + 1, "xref": xref,
                        "ext": info["ext"], "size": len(info["image"]),
                        "data": info["image"],   # bytes, kept for later OCR
                    })
            # annotations (can hide text)
            for annot in (page.annots() or []):
                atype = annot.type[1] if hasattr(annot, "type") else "unknown"
                content = annot.info.get("content", "") if hasattr(annot, "info") else ""
                if content.strip():
                    result["text_blocks"].append({
                        "page": page_num + 1, "method": "annotation",
                        "content": content.strip(), "confidence": "medium",
                    })
        doc.close()

    # -- 3. Metadata mining ----------------------------------------------------

    def _metadata_mining(self, result, pdf_path):
        meta = {}
        if HAS_PYPDF2:
            with open(pdf_path, "rb") as fh:
                reader = PyPDF2.PdfReader(fh)
                if reader.metadata:
                    meta["pypdf2"] = {str(k): str(v) for k, v in reader.metadata.items()}
        if HAS_FITZ:
            doc = fitz.open(pdf_path)
            meta["pymupdf"] = doc.metadata
            try:
                xmp = doc.get_xml_metadata()
                if xmp:
                    meta["xmp"] = xmp
            except Exception:
                pass
            doc.close()
        result["metadata"] = meta

        # Promote metadata values into text_blocks for target matching
        for source, md in meta.items():
            if isinstance(md, dict):
                for k, v in md.items():
                    vs = str(v).strip()
                    if len(vs) > 3:
                        result["text_blocks"].append({
                            "page": "meta", "method": f"metadata_{source}",
                            "content": f"{k}: {vs}", "confidence": "medium",
                        })

    # -- 4. Content-stream analysis --------------------------------------------

    def _content_streams(self, result, pdf_path):
        if not HAS_PYPDF2:
            return
        text_pats = [
            (r"\(([^)]+)\)\s*Tj",    "Tj_paren"),
            (r"BT\s*(.+?)\s*ET",     "BT_ET"),
        ]
        with open(pdf_path, "rb") as fh:
            reader = PyPDF2.PdfReader(fh)
            for page_num, page in enumerate(reader.pages, 1):
                raw = self._get_page_stream(page)
                if not raw:
                    continue
                for pat, label in text_pats:
                    for m in re.finditer(pat, raw, re.DOTALL):
                        txt = m.group(1).strip()
                        if len(txt) > 3:
                            result["text_blocks"].append({
                                "page": page_num, "method": f"stream_{label}",
                                "content": txt, "confidence": "medium",
                            })

    # -- 5. Enhanced streams (coordinate patterns) -----------------------------

    def _enhanced_streams(self, result, pdf_path):
        if not HAS_PYPDF2:
            return
        coord_pat = re.compile(
            r"(\d+)[°]\s*(\d+)[\']\s*([\d.]+)[\"']?\s*([NS])\s*"
            r"(\d+)[°]\s*(\d+)[\']\s*([\d.]+)[\"']?\s*([EW])",
            re.IGNORECASE,
        )
        with open(pdf_path, "rb") as fh:
            reader = PyPDF2.PdfReader(fh)
            for page_num, page in enumerate(reader.pages, 1):
                raw = self._get_page_stream(page)
                if not raw:
                    continue
                for m in coord_pat.finditer(raw):
                    result["text_blocks"].append({
                        "page": page_num, "method": "stream_dms_coord",
                        "content": m.group(0), "confidence": "high",
                    })

    # -- 6. Redaction-pattern detection ----------------------------------------

    def _redaction_patterns(self, result, pdf_path):
        if not HAS_FITZ:
            return
        doc = fitz.open(pdf_path)
        for page_num in range(len(doc)):
            page = doc[page_num]
            for d in page.get_drawings():
                fill = d.get("fill")
                if fill and len(fill) >= 3 and all(c < 0.1 for c in fill[:3]):
                    rect = d.get("rect")
                    if rect:
                        result["redaction_zones"].append({
                            "page": page_num + 1, "rect": list(rect),
                            "method": "black_rect_drawing",
                            "confidence": "high",
                        })
        doc.close()

    # -- 7. Image analysis + inpainting recovery --------------------------------

    def _image_analysis(self, result, pdf_path):
        if not (HAS_PIL and HAS_CV2):
            return
        for img_rec in result.get("images", []):
            data = img_rec.pop("data", None)
            if data is None:
                continue
            try:
                pil = Image.open(io.BytesIO(data))
                gray = np.array(pil.convert("L"))
                h, w = gray.shape

                # detect solid-black rectangles (most common NOAA redaction)
                _, thresh = cv2.threshold(gray, 10, 255, cv2.THRESH_BINARY_INV)
                contours, _ = cv2.findContours(thresh, cv2.RETR_EXTERNAL, cv2.CHAIN_APPROX_SIMPLE)
                rects = []
                for cnt in contours:
                    x, y, cw, ch = cv2.boundingRect(cnt)
                    roi = gray[y:y+ch, x:x+cw]
                    if roi.size > 0 and (np.sum(roi < 50) / roi.size) > 0.8:
                        rects.append((x, y, cw, ch))

                if rects and self.save_images:
                    # inpaint and save
                    mask = np.zeros_like(gray)
                    for x, y, cw, ch in rects:
                        cv2.rectangle(mask, (x, y), (x+cw, y+ch), 255, -1)
                    src = np.array(pil) if pil.mode != "L" else np.array(pil.convert("RGB"))
                    inpainted = cv2.inpaint(src, mask, 3, cv2.INPAINT_TELEA)
                    out_dir = os.path.join(self.output_dir, "recovered_images")
                    os.makedirs(out_dir, exist_ok=True)
                    fname = f"{Path(pdf_path).stem}_p{img_rec['page']}_x{img_rec['xref']}.png"
                    cv2.imwrite(os.path.join(out_dir, fname), inpainted)

                img_rec["redaction_rects"] = len(rects)
            except Exception:
                pass

    # -- 8. OCR on images ------------------------------------------------------

    def _ocr_images(self, result, pdf_path):
        if not HAS_FITZ:
            return
        doc = fitz.open(pdf_path)
        for page_num in range(len(doc)):
            page = doc[page_num]
            for img in page.get_images(full=True):
                xref = img[0]
                info = doc.extract_image(xref)
                if not info:
                    continue
                try:
                    pil = Image.open(io.BytesIO(info["image"]))
                    gray = pil.convert("L")
                except Exception:
                    continue

                # Tesseract first (faster)
                if HAS_TESSERACT:
                    try:
                        txt = pytesseract.image_to_string(gray).strip()
                        if len(txt) > 3:
                            result["text_blocks"].append({
                                "page": page_num + 1, "method": "tesseract_ocr",
                                "content": txt, "confidence": "medium",
                            })
                            continue  # got text, skip easyocr
                    except Exception:
                        pass

                # Fallback: easyocr
                if HAS_EASYOCR:
                    try:
                        reader = self._get_easyocr()
                        ocr_results = reader.readtext(np.array(gray))
                        for _, txt, conf in ocr_results:
                            if len(txt.strip()) > 3:
                                result["text_blocks"].append({
                                    "page": page_num + 1, "method": "easyocr",
                                    "content": txt.strip(),
                                    "confidence": f"{conf:.2f}",
                                })
                    except Exception:
                        pass
        doc.close()

    # ── helpers ─────────────────────────────────────────────────────────────

    @staticmethod
    def _get_page_stream(page) -> str:
        if "/Contents" not in page:
            return ""
        contents = page["/Contents"]
        if hasattr(contents, "get_object"):
            obj = contents.get_object()
            if hasattr(obj, "get_data"):
                return obj.get_data().decode("latin-1", errors="ignore")
        return ""

    def _match_targets(self, result) -> list:
        findings = []
        seen = set()
        for tb in result.get("text_blocks", []):
            content_lower = tb.get("content", "").lower()
            for target in self.targets:
                if target in content_lower:
                    key = (tb["page"], target, tb["content"][:80])
                    if key not in seen:
                        seen.add(key)
                        findings.append({
                            "page": tb["page"],
                            "target": target,
                            "method": tb["method"],
                            "context": tb["content"][:500],
                            "confidence": tb.get("confidence", ""),
                        })
        return findings

    def _get_easyocr(self):
        if self._easyocr_reader is None:
            use_gpu = bool(HAS_TORCH and torch.cuda.is_available())
            self._easyocr_reader = easyocr.Reader(["en"], gpu=use_gpu, verbose=False)
        return self._easyocr_reader


# ── CLI ─────────────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(
        description="Break PDF redactions and extract hidden content",
    )
    parser.add_argument("-d", "--directory", default=None,
                        help="Directory containing PDFs (recursive)")
    parser.add_argument("-f", "--file", default=None,
                        help="Single PDF file to analyze")
    parser.add_argument("--csv", default=None,
                        help="Write findings to CSV file")
    parser.add_argument("--json", default=None,
                        help="Write full results to JSON file")
    parser.add_argument("--targets", nargs="*", default=None,
                        help="Custom target keywords to search for")
    parser.add_argument("--no-images", action="store_true",
                        help="Skip saving recovered images")
    parser.add_argument("--no-ocr", action="store_true",
                        help="Skip OCR (Tesseract + EasyOCR) — faster")
    parser.add_argument("-o", "--output-dir", default="redaction_output",
                        help="Output directory (default: redaction_output)")
    args = parser.parse_args()

    breaker = PDFRedactionBreaker(
        targets=args.targets,
        save_images=not args.no_images,
        output_dir=args.output_dir,
        skip_ocr=args.no_ocr,
    )

    os.makedirs(args.output_dir, exist_ok=True)

    print("=" * 70)
    print("  PDF REDACTION BREAKER  —  Production Build")
    print("=" * 70)
    caps = []
    if HAS_PYPDF2:   caps.append("PyPDF2")
    if HAS_FITZ:     caps.append("PyMuPDF")
    if HAS_CV2:      caps.append("OpenCV")
    if HAS_TESSERACT:caps.append("Tesseract")
    if HAS_EASYOCR:  caps.append("EasyOCR")
    print(f"  Engines: {', '.join(caps) or 'NONE — install deps!'}")
    print(f"  Targets: {len(breaker.targets)} keywords")
    print()

    results = []
    if args.file:
        results = [breaker.analyze(args.file)]
    elif args.directory:
        results = breaker.analyze_directory(args.directory)
    else:
        # Default: look for NOAA PDFs in current dir or project root
        candidates = [
            ".",
            os.path.dirname(os.path.abspath(__file__)),
            os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."),
        ]
        for cand in candidates:
            pdfs = list(Path(cand).glob("DOC-NOAA*.pdf"))
            if pdfs:
                results = [breaker.analyze(str(p)) for p in sorted(pdfs)]
                break
        if not results:
            results = breaker.analyze_directory(".")

    if not results:
        print("No PDFs analyzed.")
        return

    # Summary
    total_findings = sum(len(r.get("findings", [])) for r in results)
    total_redactions = sum(len(r.get("redaction_zones", [])) for r in results)
    total_texts = sum(len(r.get("text_blocks", [])) for r in results)

    print(f"\n{'='*70}")
    print(f"  SUMMARY")
    print(f"{'='*70}")
    print(f"  PDFs analyzed:     {len(results)}")
    print(f"  Text blocks found: {total_texts}")
    print(f"  Redaction zones:   {total_redactions}")
    print(f"  Target hits:       {total_findings}")

    if total_findings:
        # Aggregate target hits
        target_counts = {}
        for r in results:
            for f in r.get("findings", []):
                t = f["target"]
                target_counts[t] = target_counts.get(t, 0) + 1
        print(f"\n  Target breakdown:")
        for t, c in sorted(target_counts.items(), key=lambda x: -x[1]):
            print(f"    {t:30s}  {c}")

    # Write outputs
    ts = datetime.now().strftime("%Y%m%d_%H%M%S")

    # Strip non-serialisable bytes from images before JSON dump
    clean = _strip_bytes(results)

    json_path = args.json or os.path.join(args.output_dir, f"results_{ts}.json")
    breaker.write_json(clean, json_path)

    csv_path = args.csv or os.path.join(args.output_dir, f"findings_{ts}.csv")
    breaker.write_csv(results, csv_path)

    print(f"\n  Done. Outputs in {args.output_dir}/")


def _strip_bytes(obj):
    """Recursively remove bytes values so json.dump doesn't choke."""
    if isinstance(obj, dict):
        return {k: _strip_bytes(v) for k, v in obj.items() if not isinstance(v, (bytes, bytearray))}
    if isinstance(obj, list):
        return [_strip_bytes(i) for i in obj]
    if isinstance(obj, (bytes, bytearray)):
        return f"<{len(obj)} bytes>"
    return obj


if __name__ == "__main__":
    main()
