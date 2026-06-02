#!/usr/bin/env python3
"""Compile and validate OperatorSpec before fleet dispatch (stdlib only)."""

from __future__ import annotations

import json
import os
import re
import subprocess
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any


PLACEHOLDER_RE = re.compile(r"/path/to/", re.I)


@dataclass
class CoderTaskSpec:
    target_file_path: str
    injected_context_files: list[str] = field(default_factory=list)
    explicit_steps: list[str] = field(default_factory=list)
    done_condition: str = ""


@dataclass
class OperatorSpec:
    spec_version: int
    title: str
    architecture_overview: str
    constraints: list[str]
    ordered_tasks: list[CoderTaskSpec]
    needs_user_approval: bool = True
    repo_root: str = ""
    risks: list[str] = field(default_factory=list)


def parse_operator_spec(data: dict[str, Any]) -> OperatorSpec:
    tasks = []
    for raw in data.get("ordered_tasks") or []:
        if not isinstance(raw, dict):
            raise ValueError("ordered_tasks entries must be objects")
        tasks.append(
            CoderTaskSpec(
                target_file_path=str(raw.get("target_file_path") or "").strip(),
                injected_context_files=[
                    str(p).strip() for p in (raw.get("injected_context_files") or []) if str(p).strip()
                ],
                explicit_steps=[str(s).strip() for s in (raw.get("explicit_steps") or []) if str(s).strip()],
                done_condition=str(raw.get("done_condition") or "").strip(),
            )
        )
    return OperatorSpec(
        spec_version=int(data.get("spec_version") or 0),
        title=str(data.get("title") or "").strip(),
        architecture_overview=str(data.get("architecture_overview") or "").strip(),
        constraints=[str(c).strip() for c in (data.get("constraints") or []) if str(c).strip()],
        ordered_tasks=tasks,
        needs_user_approval=bool(data.get("needs_user_approval")),
        repo_root=str(data.get("repo_root") or "").strip(),
        risks=[str(r).strip() for r in (data.get("risks") or []) if str(r).strip()],
    )


def resolve_path(repo_root: Path, p: str) -> Path:
    path = Path(p)
    if path.is_absolute():
        return path
    return (repo_root / path).resolve()


def validate_and_compile_spec(
    spec: OperatorSpec,
    repo_root: Path,
    *,
    require_existing_context: bool = True,
    git_branch_check: bool = False,
    allowed_branch_prefix: str = "spec/",
) -> tuple[bool, list[str], list[str]]:
    """Return (ok, errors, warnings). Paths resolved under repo_root unless absolute."""
    errors: list[str] = []
    warnings: list[str] = []

    if spec.spec_version != 1:
        errors.append(f"spec_version must be 1, got {spec.spec_version}")
    if not spec.title:
        errors.append("title is required")
    if len(spec.architecture_overview) < 20:
        errors.append("architecture_overview too short")
    if not spec.constraints:
        errors.append("constraints[] is empty")
    if spec.needs_user_approval is not True:
        errors.append("needs_user_approval must be true")
    if not spec.ordered_tasks:
        errors.append("ordered_tasks[] is empty")

    blob = json.dumps(
        {
            "title": spec.title,
            "constraints": spec.constraints,
            "tasks": [t.__dict__ for t in spec.ordered_tasks],
        }
    )
    if PLACEHOLDER_RE.search(blob):
        errors.append("spec contains /path/to/ placeholders")

    if not re.search(r"p100|5001|5002|10\.0\.0\.61", blob, re.I):
        warnings.append("constraints do not mention keeping T440 P100s free during satellite runs")

    if git_branch_check:
        try:
            branch = (
                subprocess.check_output(
                    ["git", "-C", str(repo_root), "rev-parse", "--abbrev-ref", "HEAD"],
                    text=True,
                    stderr=subprocess.DEVNULL,
                )
                .strip()
            )
            if not branch.startswith(allowed_branch_prefix):
                errors.append(
                    f"git branch '{branch}' does not start with '{allowed_branch_prefix}' "
                    "(use --no-git-check to skip)"
                )
        except (subprocess.CalledProcessError, FileNotFoundError) as e:
            errors.append(f"git branch check failed: {e}")

    for idx, task in enumerate(spec.ordered_tasks, start=1):
        prefix = f"task {idx}"
        if not task.target_file_path:
            errors.append(f"{prefix}: target_file_path missing")
        if not task.explicit_steps:
            errors.append(f"{prefix}: explicit_steps empty")
        if not task.done_condition:
            errors.append(f"{prefix}: done_condition missing")

        target = resolve_path(repo_root, task.target_file_path)
        parent = target.parent
        if not parent.exists() and not target.exists():
            errors.append(f"{prefix}: parent directory does not exist: {parent}")

        if require_existing_context:
            for ctx in task.injected_context_files:
                cp = resolve_path(repo_root, ctx)
                if not cp.exists():
                    errors.append(f"{prefix}: injected context not found: {cp}")

    return len(errors) == 0, errors, warnings


def spec_to_worker_prompt(task: CoderTaskSpec, spec: OperatorSpec) -> str:
    steps = "\n".join(f"- {s}" for s in task.explicit_steps)
    ctx = "\n".join(f"- {p}" for p in task.injected_context_files) or "(none)"
    return (
        f"You are a closed-world coder for cesarops-forge-v2.\n"
        f"Mission: {spec.title}\n"
        f"Overview: {spec.architecture_overview}\n\n"
        f"Target file: {task.target_file_path}\n"
        f"Context files:\n{ctx}\n\n"
        f"Steps:\n{steps}\n\n"
        f"Done condition (must pass before stopping):\n{task.done_condition}\n"
    )


def load_spec_json(text: str) -> OperatorSpec:
    text = text.strip()
    if text.startswith("```"):
        text = re.sub(r"^```(?:json)?\s*", "", text)
        text = re.sub(r"\s*```\s*$", "", text)
    data = json.loads(text)
    if not isinstance(data, dict):
        raise ValueError("OperatorSpec must be a JSON object")
    return parse_operator_spec(data)
