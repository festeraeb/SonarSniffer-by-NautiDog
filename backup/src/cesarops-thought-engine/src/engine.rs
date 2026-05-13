use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use uuid::Uuid;

use crate::clients::{KoboldClient, NautivecsClient, CesaropsClient, Message};
use crate::models::{TaskRequest, TaskResponse, PlanSpec, SubTask, Step};

pub struct ThoughtEngine {
    pub kobold: KoboldClient,
    pub nautivecs: NautivecsClient,
    pub cesarops: CesaropsClient,
    pub tasks: Arc<tokio::sync::RwLock<std::collections::HashMap<String, TaskResponse>>>,
}

impl ThoughtEngine {
    pub fn new(
        kobold: KoboldClient,
        nautivecs: NautivecsClient,
        cesarops: CesaropsClient,
    ) -> Self {
        Self {
            kobold,
            nautivecs,
            cesarops,
            tasks: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }

    pub async fn process_task(&self, request: TaskRequest) -> Result<TaskResponse> {
        let task_id = request.task_id.unwrap_or_else(|| {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            format!("task_{}", timestamp)
        });

        let mut response = TaskResponse {
            task_id: task_id.clone(),
            status: "processing".to_string(),
            result: None,
            error: None,
            steps: Vec::new(),
        };

        // Store initial task state
        {
            let mut tasks = self.tasks.write().await;
            tasks.insert(task_id.clone(), response.clone());
        }

        // Step 1: THINK - Decompose Query
        tracing::info!("[{}] Step 1: Thinking/Decomposing...", task_id);
        let thinking_result = self.think(&request.query).await;
        
        match thinking_result {
            Ok(plan_json) => {
                response.steps.push(Step {
                    step: "think".to_string(),
                    output: Some(plan_json.chars().take(200).collect::<String>() + "..."),
                });

                // Parse JSON from thinking result
                let sub_tasks = self.parse_thinking_result(&plan_json, &request.query);
                
                // Step 2: SEARCH - Gather Context
                tracing::info!("[{}] Step 2: Searching...", task_id);
                let context_list = self.search(&sub_tasks).await;
                response.steps.push(Step {
                    step: "search".to_string(),
                    output: Some(format!("Found {} context items", context_list.len())),
                });

                // Step 3: PLAN - Create Spec for 35B
                tracing::info!("[{}] Step 3: Planning Spec...", task_id);
                let spec = self.plan(&request.query, &sub_tasks, &context_list).await;
                response.steps.push(Step {
                    step: "plan".to_string(),
                    output: Some(format!("Spec created with {} sub-tasks", spec.sub_tasks.len())),
                });

                // Step 4: DISPATCH - Send to 35B
                tracing::info!("[{}] Step 4: Dispatching to 35B...", task_id);
                let execution_result = self.dispatch(&spec, &context_list).await?;
                response.steps.push(Step {
                    step: "dispatch".to_string(),
                    output: Some("Execution completed".to_string()),
                });

                // Step 5: VERIFY - Check Output
                tracing::info!("[{}] Step 5: Verifying...", task_id);
                let verification_result = self.verify(&request.query, &execution_result).await?;
                
                if self.is_verified(&verification_result) {
                    response.status = "completed".to_string();
                    response.result = Some(execution_result);
                } else {
                    response.status = "completed_with_warnings".to_string();
                    response.result = Some(execution_result);
                    response.error = Some(format!("Verification warning: {}", verification_result.chars().take(100).collect::<String>()));
                }
            }
            Err(e) => {
                tracing::error!("[{}] Thinking failed: {}", task_id, e);
                response.status = "failed".to_string();
                response.error = Some(format!("Thinking failed: {}", e));
            }
        }

        // Update final state
        {
            let mut tasks = self.tasks.write().await;
            tasks.insert(task_id.clone(), response.clone());
        }

        Ok(response)
    }

    async fn think(&self, query: &str) -> Result<String> {
        let thinking_prompt = vec![
            crate::clients::Message {
                role: "system".to_string(),
                content: "You are a Research Lead. Break down this complex query into specific sub-tasks. Return JSON.".to_string(),
            },
            crate::clients::Message {
                role: "user".to_string(),
                content: format!(
                    "Query: {}\n\nBreak this into 3-5 sub-tasks. Specify for each: task description, source (code/web), and rationale. Return as JSON array.",
                    query
                ),
            },
        ];

        self.kobold
            .chat_completion(thinking_prompt, 0.7, 4096)
            .await
            .context("Failed to get thinking result from KoboldCPP")
    }

    fn parse_thinking_result(&self, thinking_result: &str, original_query: &str) -> Vec<SubTask> {
        // Try to parse as JSON array
        if let Ok(parsed) = serde_json::from_str::<Vec<serde_json::Value>>(thinking_result) {
            return parsed
                .iter()
                .map(|item| SubTask {
                    description: item["task"].as_str().unwrap_or(
                        item["query"].as_str().unwrap_or(original_query)
                    ).to_string(),
                })
                .filter(|st| !st.description.is_empty())
                .collect();
        }

        // Fallback if LLM didn't return pure JSON
        vec![SubTask {
            description: original_query.to_string(),
        }]
    }

    async fn search(&self, sub_tasks: &[SubTask]) -> Vec<String> {
        let mut context_list = Vec::new();

        for sub_task in sub_tasks {
            // Search Nautivecs
            match self.nautivecs.search(&sub_task.description, 5).await {
                Ok(results) => {
                    for result in results {
                        context_list.push(format!(
                            "Code Reference: {} -> {}",
                            result.file_path, result.text
                        ));
                    }
                }
                Err(e) => {
                    tracing::warn!("Nautivecs search failed for '{}': {}", sub_task.description, e);
                }
            }

            // Web search would go here (placeholder)
            // For now, we skip web search as it's not implemented
        }

        context_list
    }

    async fn plan(&self, query: &str, sub_tasks: &[SubTask], context: &[String]) -> PlanSpec {
        let planning_prompt = vec![
            crate::clients::Message {
                role: "system".to_string(),
                content: "You are a Technical Planner. Convert research findings into a structured execution spec for a code generation engine.".to_string(),
            },
            crate::clients::Message {
                role: "user".to_string(),
                content: format!(
                    "Original Query: {}\n\nSub-tasks from thinking: {}\n\nGathered Context: {}\n\nCreate a structured spec with: sub_tasks (list of dicts with 'description'), required_context (list of strings), and output_format (string). Return JSON.",
                    query,
                    serde_json::to_string_pretty(sub_tasks).unwrap_or_default(),
                    serde_json::to_string_pretty(&context.iter().take(5).cloned().collect::<Vec<_>>()).unwrap_or_default()
                ),
            },
        ];

        match self.kobold.chat_completion(planning_prompt, 0.3, 4096).await {
            Ok(plan_result) => {
                // Try to parse as JSON
                if let Ok(spec_json) = serde_json::from_str::<serde_json::Value>(&plan_result) {
                    let sub_tasks: Vec<SubTask> = spec_json
                        .get("sub_tasks")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|item| {
                                    item.get("description")
                                        .and_then(|d| d.as_str())
                                        .map(|s| SubTask { description: s.to_string() })
                                })
                                .collect()
                        })
                        .unwrap_or_else(|| vec![SubTask { description: query.to_string() }]);

                    let required_context: Vec<String> = spec_json
                        .get("required_context")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|item| item.as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_else(|| context.to_vec());

                    let output_format = spec_json
                        .get("output_format")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Code blocks with explanations")
                        .to_string();

                    PlanSpec {
                        sub_tasks,
                        required_context,
                        output_format,
                    }
                } else {
                    // Fallback spec
                    PlanSpec {
                        sub_tasks: vec![SubTask { description: query.to_string() }],
                        required_context: context.to_vec(),
                        output_format: "Code".to_string(),
                    }
                }
            }
            Err(_) => {
                // Fallback spec on error
                PlanSpec {
                    sub_tasks: vec![SubTask { description: query.to_string() }],
                    required_context: context.to_vec(),
                    output_format: "Code".to_string(),
                }
            }
        }
    }

    async fn dispatch(&self, spec: &PlanSpec, context: &[String]) -> Result<String> {
        self.cesarops
            .execute_task(spec, context)
            .await
            .context("Failed to execute task with Cesarops")
    }

    async fn verify(&self, query: &str, execution_result: &str) -> Result<String> {
        let verification_prompt = vec![
            crate::clients::Message {
                role: "system".to_string(),
                content: "You are a Code Reviewer. Check the following code for obvious errors, syntax issues, or logical flaws. If it looks good, say 'VERIFIED'. If not, explain why.".to_string(),
            },
            crate::clients::Message {
                role: "user".to_string(),
                content: format!(
                    "Original Task: {}\n\nGenerated Code:\n{}",
                    query, execution_result
                ),
            },
        ];

        self.kobold
            .chat_completion(verification_prompt, 0.1, 4096)
            .await
            .context("Failed to get verification result from KoboldCPP")
    }

    fn is_verified(&self, verification_result: &str) -> bool {
        let lower = verification_result.to_lowercase();
        lower.contains("verified") || lower.contains("correct")
    }
}
