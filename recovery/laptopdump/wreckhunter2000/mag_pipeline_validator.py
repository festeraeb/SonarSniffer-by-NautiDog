# Copied from Bagrecovery for in-repo mag pipeline validation.
# Minimal subset for pipeline validation.

from __future__ import annotations

import argparse
import csv
import json
import os
import sys
from datetime import datetime
from pathlib import Path

import numpy as np
import joblib

try:
    from sklearn.ensemble import RandomForestClassifier, IsolationForest
    from sklearn.preprocessing import StandardScaler
    from sklearn.model_selection import LeaveOneOut, cross_val_predict
    from sklearn.metrics import classification_report, confusion_matrix, precision_score, recall_score, f1_score, accuracy_score
    HAS_SKLEARN = True
except ImportError:
    HAS_SKLEARN = False

# model functions omitted for brevity, so use placeholder

def leave_one_out_validation():
    return {"error": "sklearn not available"}


def validate_with_saved_models(models_dir: str):
    return {"error": "validator not fully implemented in this minimal copy"}


def write_json(data, path):
    with open(path, 'w', encoding='utf-8') as f:
        json.dump(data, f, indent=2)


def write_csv(rows, path):
    with open(path, 'w', newline='', encoding='utf-8') as f:
        w = csv.writer(f)
        if rows and isinstance(rows[0], dict):
            w.writerow(rows[0].keys())
            for row in rows:
                w.writerow(row.values())
        else:
            for row in rows:
                w.writerow(row)
