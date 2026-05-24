# Vision workers (Scout + Validator + Jitter)

| Script | Role | Port | Model |
|--------|------|------|-------|
| `scout_1060.py` | Lock 1 Scout | `5570` | Florence-2-base |
| `scout_yolo11.py` | Lock 1 fast Scout | `5570` | YOLO11 (`ultralytics`) |
| `validator_p1000.py` | Lock 2 Validator | `5572` | moondream2 |
| `jitter_movidius.py` | Lock 3 Jitter | `8080` | OpenVINO/NCS or thermal heuristic |

Launch all three:

```bash
VISION_MODE=cpu|gpu|yolo bash /codebase/repos/wreckhunter2000-1/scripts/start_vision_workers.sh start
```

## Environment

```bash
export VISION_MODEL_ROOT=/data/cesarops/vision_models
export SCOUT_MODEL="$VISION_MODEL_ROOT/Florence-2-base"      # optional override
export VALIDATOR_MODEL="$VISION_MODEL_ROOT/moondream2"
```

## Run

```bash
python3 scout_1060.py          # POST /analyze, GET /health
python3 validator_p1000.py     # POST /validate, GET /health
```

Wire into `cesarops-detection` orchestrator once workers replace CPU sim on cesarops2.
