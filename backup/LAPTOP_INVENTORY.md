# LAPTOP INVENTORY

## Python Files (.py)

| Filename | Purpose | Category |
|----------|---------|----------|
| ai_director.py | Orchestrates AI model selection and inference tasks | AI Director |
| alaska_canopy_scan.py | Handles Alaska-specific canopy penetration radar data | Regional Scanning |
| analyze_crossref.py | Cross-references sonar and magnetometer data | Data Analysis |
| andaste_geometry_test.py | Tests geometric algorithms for Andaste sector | Testing |
| audit_wrecks_db.py | Audits wreck database integrity and records | Database Admin |
| config_loader.py | Loads and validates system configuration files | Configuration |
| database_connector.py | Manages persistent connections to the wreck DB | Database Core |
| db_ingestor.py | Ingests raw scan data into the database schema | Data Ingestion |
| ml/inference/wreck_vs_obstruction_classifier.py | Classifies targets as wrecks vs obstructions | ML Inference |
| ml/training/train_wreck_classifier_gpu.py | Trains classification models using GPU acceleration | ML Training |
| scan_engine.py | Executes the unified 7-pass detection pipeline | Core Engine |
| sonar_sniffer.py (inferred) | Sniffs and parses raw sonar packets | Sonar Processing |
| utils/helpers.py (inferred) | General utility functions shared across modules | Utilities |
| tests/test_scan_pipeline.py (inferred) | Unit tests for the scanning pipeline | Testing |
| __init__.py (inferred) | Package initialization markers | Configuration |

## Rust Files (.rs)

| Filename | Purpose | Category |
|----------|---------|----------|
| src/main.rs | Main application entry point and CLI handling | Application Core |
| crates/mistral-rs-fork/src/lib.rs | Forked Mistral-RS library interface | AI Library |
| crates/mistral-rs-fork/src/vision_models/phi4/audio_embedding.rs | Audio feature extraction for multimodal AI | AI Vision |
| sovereign-cloud/src/api.rs | REST API endpoints for cloud resources | Cloud API |
| sovereign-cloud/src/allocation.rs | Logic for resource allocation and scheduling | Cloud Management |
| sovereign-cloud/src/idle_scout.rs | Detects and utilizes idle compute resources | Cloud Optimization |
| cesarops-adaptive/src/pipeline.rs | Adaptive detection logic processing scan data | Detection Pipeline |
| nauticuvs/src/navigation.rs | Navigation algorithms and path planning | Navigation |
| Cargo.toml | Rust project manifest and dependency definitions | Configuration |
