#!/usr/bin/env python3
"""Heuristic rubric scores for role-bench rounds (feeds the judge opinion step)."""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import Any


@dataclass
class RubricScore:
    total: float
    breakdown: dict[str, float] = field(default_factory=dict)
    notes: list[str] = field(default_factory=list)


def _has_sections(text: str, headings: list[str]) -> float:
    if not text:
        return 0.0
    low = text.lower()
    hits = sum(1 for h in headings if h.lower() in low)
    return hits / max(len(headings), 1)


def _word_count(text: str) -> int:
    return len(re.findall(r"\S+", text or ""))


def score_thinker(text: str) -> RubricScore:
    headings = [
        "## goal",
        "## constraints",
        "## architecture",
        "## handoff",
        "## risks",
    ]
    structure = _has_sections(text, headings)
    wc = _word_count(text)
    length = 1.0 if 200 <= wc <= 900 else (0.6 if wc >= 120 else 0.3)
    actionable = 1.0 if re.search(r"(acceptance|criteria|numbered|task\s*\d|\d+\.)", text, re.I) else 0.4
    no_forge_ok = 1.0 if re.search(r"forge|9100", text, re.I) is None or re.search(
        r"not.*forge|without forge|direct.*llama", text, re.I
    ) else 0.5
    breakdown = {
        "structure": round(structure * 100, 1),
        "length": round(length * 100, 1),
        "actionable_handoff": round(actionable * 100, 1),
        "constraint_awareness": round(no_forge_ok * 100, 1),
    }
    total = sum(breakdown.values()) / len(breakdown)
    notes = []
    if structure < 0.6:
        notes.append("Missing one or more required thinker sections.")
    if wc < 150:
        notes.append("Outline is very short for a design handoff.")
    if actionable < 0.7:
        notes.append("Weak acceptance criteria / numbered tasks for coders.")
    return RubricScore(total=round(total, 1), breakdown=breakdown, notes=notes)


def score_coder(text: str) -> RubricScore:
    has_paths = 1.0 if re.search(r"[`/][\w./-]+\.(py|rs|sh|md|json)", text) else 0.3
    has_code = 1.0 if "```" in text or re.search(r"def |class |fn ", text) else 0.35
    has_run = 1.0 if re.search(r"(how to run|python3? |cargo run|bash )", text, re.I) else 0.4
    scope = 1.0 if re.search(r"did not|deliberately|out of scope|NOT do", text, re.I) else 0.5
    wc = _word_count(text)
    length = 1.0 if wc >= 180 else 0.45
    breakdown = {
        "paths": round(has_paths * 100, 1),
        "code": round(has_code * 100, 1),
        "run_instructions": round(has_run * 100, 1),
        "scope_discipline": round(scope * 100, 1),
        "depth": round(length * 100, 1),
    }
    total = sum(breakdown.values()) / len(breakdown)
    notes = []
    if has_code < 0.7:
        notes.append("Little or no implementation detail.")
    if has_paths < 0.7:
        notes.append("File paths not clearly specified.")
    return RubricScore(total=round(total, 1), breakdown=breakdown, notes=notes)


def score_reviewer(text: str) -> RubricScore:
    sections = ["## summary", "## critical", "## correction", "## recommendation"]
    structure = _has_sections(text, sections)
    compares = 1.0 if re.search(r"weak|strong|best|worst|both", text, re.I) else 0.4
    actionable = 1.0 if re.search(r"should|must|fix|change|replace", text, re.I) else 0.45
    wc = _word_count(text)
    length = 1.0 if wc >= 150 else 0.5
    breakdown = {
        "structure": round(structure * 100, 1),
        "comparative": round(compares * 100, 1),
        "actionable": round(actionable * 100, 1),
        "depth": round(length * 100, 1),
    }
    total = sum(breakdown.values()) / len(breakdown)
    notes = []
    if structure < 0.5:
        notes.append("Reviewer sections incomplete.")
    return RubricScore(total=round(total, 1), breakdown=breakdown, notes=notes)


SCORERS = {
    "thinker": score_thinker,
    "coder": score_coder,
    "reviewer": score_reviewer,
}


def score_round(round_name: str, text: str) -> RubricScore:
    fn = SCORERS.get(round_name)
    if not fn:
        return RubricScore(total=0.0, notes=[f"Unknown round: {round_name}"])
    return fn(text)


def format_candidate_block(candidates: list[dict[str, Any]]) -> str:
    parts = []
    for c in candidates:
        rub = c.get("rubric") or {}
        parts.append(
            f"### candidate_id: {c['id']}\n"
            f"endpoint: {c['endpoint']}\n"
            f"model: {c.get('model', '?')}\n"
            f"node/gpu: {c.get('node', '?')} / {c.get('gpu_name', '?')}\n"
            f"rubric_total: {rub.get('total', '?')}\n"
            f"rubric_breakdown: {rub.get('breakdown', {})}\n"
            f"rubric_notes: {rub.get('notes', [])}\n"
            f"latency_s: {c.get('latency_s', '?')}\n"
            f"--- output ---\n{c.get('text', '')[:12000]}\n"
        )
    return "\n".join(parts)
