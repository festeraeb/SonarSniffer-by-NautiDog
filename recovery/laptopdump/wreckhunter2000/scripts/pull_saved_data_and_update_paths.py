import shutil
import os
from pathlib import Path

# Configure your source/destination roots
BAGRECOVERY_ROOT = Path(r"c:\Users\thomf\programming\Bagrecovery")
TARGET_ROOT = Path(r"c:\Users\thomf\programming\wreckhunter2000")

FOLDERS_TO_COPY = [
    "magnetic_data",
    "bagfilework",
    "db",
    "wreck_hunting_ml/output",
    "bagfiles",
    # optionally include per-target sentinel bands if Bagrecovery has it
    "wreck_hunting_ml/sentinel_bands",
    "sample_data",
    "data/sentinel",
]

FILE_PATTERNS_TO_UPDATE = [
    "**/*.py",
    "**/*.md",
    "**/*.sh",
    "**/*.ps1",
    "**/*.rs",
]

OLD_PATH_FRAGMENT = str(BAGRECOVERY_ROOT).replace("\\", "\\\\")
NEW_PATH_FRAGMENT = str(TARGET_ROOT).replace("\\", "\\\\")


def copy_folders():
    for rel in FOLDERS_TO_COPY:
        src = BAGRECOVERY_ROOT / rel
        dst = TARGET_ROOT / rel
        if not src.exists():
            print(f"[WARN] Source folder not found: {src}")
            continue

        if dst.exists():
            print(f"[INFO] Destination already exists, skipping copy for {dst}")
            continue

        print(f"Copying {src} -> {dst}")
        shutil.copytree(src, dst)


def copy_additional_sentinel_folders():
    candidate_pairs = [
        ("wreck_hunting_ml/sentinel_bands", "data/sentinel"),
        ("sample_data", "data/sentinel"),
    ]
    for src_rel, dst_rel in candidate_pairs:
        src = BAGRECOVERY_ROOT / src_rel
        dst = TARGET_ROOT / dst_rel
        if not src.exists():
            print(f"[INFO] Additional source folder not found (skip): {src}")
            continue

        if dst.exists():
            print(f"[INFO] Additional destination exists (skip): {dst}")
            continue

        print(f"[COPY] Additional folder {src} -> {dst}")
        shutil.copytree(src, dst)


def update_path_strings():
    for pattern in FILE_PATTERNS_TO_UPDATE:
        for path in TARGET_ROOT.glob(pattern):
            if path.is_file():
                try:
                    text = path.read_text(encoding="utf-8")
                except UnicodeDecodeError:
                    continue

                if str(BAGRECOVERY_ROOT) in text:
                    new_text = text.replace(str(BAGRECOVERY_ROOT), str(TARGET_ROOT))
                    if new_text != text:
                        path.write_text(new_text, encoding="utf-8")
                        print(f"[UPDATED] {path}")


def main():
    print("=== Pulling saved data from Bagrecovery to wreckhunter2000 ===")
    copy_folders()
    copy_additional_sentinel_folders()

    print("=== Updating embedded Bagrecovery paths in scripts/files ===")
    update_path_strings()

    print("=== Done. Please verify Git diff and pipeline paths. ===")


if __name__ == "__main__":
    main()
