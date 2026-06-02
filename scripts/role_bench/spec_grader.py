#!/usr/bin/env python3
"""Rubric for OperatorSpec JSON from local spec-draft models."""

from __future__ import annotations

import json
import re
from dataclasses import dataclass, field
from typing import Any


@dataclass
class SpecScore:
    total: float
    breakdown: dict[str, float] = field(default_factory=dict)
    notes: list[str] = field(default_factory=list)
    parsed: dict[str, Any] | None = None


def extract_json_object(text: str) -> dict[str, Any] | None:
    if not text:
        return None
    text = text.strip()
    if text.startswith("```"):
        text = re.sub(r"^```(?:json)?\s*", "", text)
        text = re.sub(r"\s*```\s*$", "", text)
    try:
        obj = json.loads(text)
        return obj if isinstance(obj, dict) else None
    except json.JSONDecodeError:
        pass
    m = re.search(r"\{[\s\S]*\}", text)
    if not m:
        return None
    try:
        obj = json.loads(m.group(0))
        return obj if isinstance(obj, dict) else None
    except json.JSONDecodeError:
        return None


def _has_list(val: Any, min_len: int = 1) -> bool:
    return isinstance(val, list) and len(val) >= min_len


def score_operator_spec(text: str) -> SpecScore:
    notes: list[str] = []
    parsed = extract_json_object(text)
    if not parsed:
        return SpecScore(
            total=0.0,
            breakdown={"json_valid": 0.0},
            notes=["Could not parse JSON object from model output."],
        )

    required_top = ["spec_version", "title", "given", "do", "deliver", "needs_user_approval"]
    top_hits = sum(1 for k in required_top if k in parsed)
    structure = top_hits / len(required_top)

    given = parsed.get("given") if isinstance(parsed.get("given"), dict) else {}
    g_ctx = 1.0 if isinstance(given.get("context"), str) and len(given["context"]) >= 20 else 0.0
    g_cons = 1.0 if _has_list(given.get("constraints"), 1) else 0.3
    g_paths = 1.0 if isinstance(given.get("paths"), dict) and given["paths"] else 0.4

    do_list = parsed.get("do") if isinstance(parsed.get("do"), list) else []
    do_ok = 0.0
    if do_list:
        actionable = sum(
            1
            for item in do_list
            if isinstance(item, dict)
            and item.get("action")
            and (item.get("success") or item.get("command"))
        )
        do_ok = actionable / max(len(do_list), 1)
    else:
        notes.append("Missing or empty do[] steps.")

    deliver = parsed.get("deliver") if isinstance(parsed.get("deliver"), dict) else {}
    del_ok = 1.0 if _has_list(deliver.get("acceptance"), 1) else 0.35

    approval = 1.0 if parsed.get("needs_user_approval") is True else 0.0
    if approval < 1.0:
        notes.append("needs_user_approval must be true.")

    blob = json.dumps(parsed)
    no_placeholder = 0.0 if re.search(r"/path/to/", blob, re.I) else 1.0
    if no_placeholder < 1.0:
        notes.append("Contains placeholder paths.")

    p100_aware = 1.0 if re.search(r"p100|5001|5002|10\.0\.0\.61", blob, re.I) else 0.5
    version = 1.0 if parsed.get("spec_version") == 1 else 0.6

    breakdown = {
        "json_valid": 100.0,
        "structure": round(structure * 100, 1),
        "given_context": round(g_ctx * 100, 1),
        "given_constraints": round(g_cons * 100, 1),
        "do_actionable": round(do_ok * 100, 1),
        "deliver_acceptance": round(del_ok * 100, 1),
        "approval_gate": round(approval * 100, 1),
        "no_placeholders": round(no_placeholder * 100, 1),
        "p100_awareness": round(p100_aware * 100, 1),
        "spec_version": round(version * 100, 1),
    }
    weights = {
        "structure": 1.2,
        "given_context": 0.8,
        "given_constraints": 0.7,
        "do_actionable": 1.3,
        "deliver_acceptance": 1.0,
        "approval_gate": 1.0,
        "no_placeholders": 1.1,
        "p100_awareness": 0.6,
        "spec_version": 0.3,
    }
    num = sum(breakdown[k] * weights[k] for k in weights)
    den = sum(weights.values()) * 100
    total = round(num / den, 1)

    if structure < 0.85:
        notes.append("Missing required top-level OperatorSpec fields.")
    if do_ok < 0.6:
        notes.append("do[] steps lack success criteria or commands.")

    return SpecScore(total=total, breakdown=breakdown, notes=notes, parsed=parsed)
