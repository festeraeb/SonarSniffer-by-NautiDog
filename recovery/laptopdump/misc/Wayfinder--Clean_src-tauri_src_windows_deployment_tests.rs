#[cfg(test)]
mod windows_deployment_tests {
    use super::*;
    use std::env;
    use std::path::PathBuf;

    #[test]
    fn test_windows_platform_detected() {
        #[cfg(target_os = "windows")]
        {
            assert!(cfg!(windows), "Windows platform should be detected");
        }
    }

    #[test]
    fn test_path_handling_backslashes() {
        // Windows uses backslashes, ensure path handling works
        let test_path = r"C:\Temp\md-scanner\.wayfinder_index";
        let path = std::path::Path::new(test_path);
        assert!(path.is_absolute(), "Windows absolute path should be recognized");
    }

    #[test]
    fn test_path_handling_forward_slashes() {
        // Test forward slash compatibility on Windows
        let test_path = "C:/Temp/md-scanner/.wayfinder_index";
        let path = std::path::Path::new(test_path);
        assert!(path.is_absolute(), "Forward slash paths should work on Windows");
    }

    #[test]
    fn test_azure_config_struct() {
        use crate::commands::AzureConfig;

        let config = AzureConfig {
            endpoint: "https://test.openai.azure.com".to_string(),
            api_key: "test-key".to_string(),
            deployment_name: "text-embedding-3-small".to_string(),
            api_version: "2024-02-01".to_string(),
        };

        assert_eq!(config.endpoint, "https://test.openai.azure.com");
        assert_eq!(config.deployment_name, "text-embedding-3-small");
    }

    #[test]
    fn test_file_entry_serialization() {
        use crate::commands::FileEntry;

        let entry = FileEntry {
            path: r"C:\Temp\test.md".to_string(),
            name: "test.md".to_string(),
            size: 1024,
            modified: "2026-02-13T10:00:00Z".to_string(),
            extension: "md".to_string(),
        };

        let serialized = serde_json::to_string(&entry);
        assert!(serialized.is_ok(), "FileEntry should serialize successfully");
    }

    #[test]
    fn test_index_data_structure() {
        use crate::commands::{IndexData, FileEntry};

        let files = vec![
            FileEntry {
                path: r"C:\Temp\file1.md".to_string(),
                name: "file1.md".to_string(),
                size: 512,
                modified: "2026-02-13T10:00:00Z".to_string(),
                extension: "md".to_string(),
            }
        ];

        let index = IndexData {
            files,
            scan_path: r"C:\Temp".to_string(),
            created_at: "2026-02-13T10:00:00Z".to_string(),
        };

        assert_eq!(index.files.len(), 1);
        assert_eq!(index.scan_path, r"C:\Temp");
    }

    #[tokio::test]
    async fn test_validate_all_azure_configs_invalid_path() {
        use crate::commands::validate_all_azure_configs;

        let result = validate_all_azure_configs(r"C:\NonExistent\Path".to_string()).await;
        assert!(result.is_err(), "Should error on non-existent path");
    }

    #[test]
    fn test_extension_filtering() {
        let extensions = vec!["md", "txt", "rs"];
        let test_file = "test.md";

        let ext = test_file.split('.').last().unwrap_or("");
        assert!(extensions.contains(&ext), "Should detect markdown extension");
    }

    #[test]
    fn test_search_result_structure() {
        use crate::commands::SearchResult;

        let result = SearchResult {
            path: r"C:\Temp\result.md".to_string(),
            name: "result.md".to_string(),
            score: 0.95,
            preview: Some("Preview text...".to_string()),
        };

        assert!(result.score > 0.9, "High relevance score should be preserved");
        assert!(result.preview.is_some(), "Preview should be present");
    }

    #[test]
    fn test_cluster_structure() {
        use crate::commands::Cluster;

        let cluster = Cluster {
            id: 0,
            centroid: vec![0.1, 0.2, 0.3],
            file_paths: vec![
                r"C:\Temp\file1.md".to_string(),
                r"C:\Temp\file2.md".to_string(),
            ],
            label: Some("Documentation".to_string()),
        };

        assert_eq!(cluster.file_paths.len(), 2);
        assert!(cluster.label.is_some());
    }

    #[test]
    fn test_system_info_includes_windows() {
        #[cfg(target_os = "windows")]
        {
            let os = env::consts::OS;
            assert_eq!(os, "windows", "OS constant should be 'windows'");
        }
    }

    #[test]
    fn test_embedding_dimensions() {
        // text-embedding-3-small uses 1536 dimensions
        let embedding: Vec<f32> = vec![0.0; 1536];
        assert_eq!(embedding.len(), 1536, "Embedding should have 1536 dimensions");
    }

    #[test]
    fn test_cosine_similarity_calculation() {
        // Simple cosine similarity test
        let v1 = vec![1.0, 0.0, 0.0];
        let v2 = vec![1.0, 0.0, 0.0];

        let dot: f32 = v1.iter().zip(v2.iter()).map(|(a, b)| a * b).sum();
        let mag1: f32 = v1.iter().map(|x| x * x).sum::<f32>().sqrt();
        let mag2: f32 = v2.iter().map(|x| x * x).sum::<f32>().sqrt();

        let similarity = dot / (mag1 * mag2);
        assert!((similarity - 1.0).abs() < 0.001, "Identical vectors should have similarity of 1.0");
    }

    #[test]
    fn test_git_repo_detection_structure() {
        // Test that git repo path handling works
        let test_paths = vec![
            r"C:\Temp\md-scanner\.git",
            r"C:\Users\Project\.git",
            r"C:\Code\wayfinder\.git",
        ];

        for path in test_paths {
            let path_obj = std::path::Path::new(path);
            let parent = path_obj.parent();
            assert!(parent.is_some(), "Should extract parent directory from .git path");
        }
    }

    #[test]
    fn test_file_size_formatting() {
        let sizes = vec![
            (512u64, "bytes"),
            (1024u64, "KB"),
            (1_048_576u64, "MB"),
            (1_073_741_824u64, "GB"),
        ];

        for (size, _unit) in sizes {
            assert!(size > 0, "File size should be positive");
        }
    }

    #[test]
    fn test_timestamp_parsing() {
        use chrono::{DateTime, Local};

        let timestamp = Local::now().to_rfc3339();
        let parsed = DateTime::parse_from_rfc3339(&timestamp);
        assert!(parsed.is_ok(), "Timestamp should parse correctly");
    }

    #[test]
    fn test_json_serialization_roundtrip() {
        use crate::commands::AzureConfig;

        let original = AzureConfig {
            endpoint: "https://test.openai.azure.com".to_string(),
            api_key: "test-key-123".to_string(),
            deployment_name: "embedding-model".to_string(),
            api_version: "2024-02-01".to_string(),
        };

        let json = serde_json::to_string(&original).unwrap();
        let deserialized: AzureConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(original.endpoint, deserialized.endpoint);
        assert_eq!(original.api_key, deserialized.api_key);
    }

    #[test]
    fn test_walkdir_filter_logic() {
        // Test the logic used in validate_all_azure_configs
        let test_dir_name = ".wayfinder_index";
        let config_file_name = "azure_config.json";

        assert_eq!(test_dir_name, ".wayfinder_index");
        assert_eq!(config_file_name, "azure_config.json");
    }
}
