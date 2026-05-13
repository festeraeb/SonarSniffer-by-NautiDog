use tracing::info;

#[derive(Debug, Clone)]
pub struct Task {
    pub index: usize,
    pub text: String,
    pub done: bool,
}

pub fn parse_tasks(content: &str) -> Vec<Task> {
    let re = regex::Regex::new(r"- \[([ xX])\] (.+)").unwrap();
    let mut tasks = Vec::new();

    for cap in re.captures_iter(content) {
        let status = cap.get(1).unwrap().as_str();
        let text = cap.get(2).unwrap().as_str().to_string();
        let done = status == "x" || status == "X";
        tasks.push(Task {
            index: tasks.len(),
            text,
            done,
        });
    }

    tasks
}

pub fn build_coder_prompt(task: &str, context_snippets: &[String], corrections: &[String]) -> String {
    let mut prompt = String::with_capacity(4096);
    prompt.push_str("You are an expert Rust developer implementing code for the CesarOps cluster.\n\n");
    prompt.push_str(&format!("TASK: {}\n\n", task));

    if !context_snippets.is_empty() {
        prompt.push_str("RELEVANT CODE CONTEXT (from nautivecs):\n");
        for (i, ctx) in context_snippets.iter().enumerate() {
            let snippet = truncate_to_chars(ctx, 2000);
            prompt.push_str(&format!("--- Snippet {} ---\n{}\n\n", i + 1, snippet));
        }
    }

    if !corrections.is_empty() {
        prompt.push_str("PREVIOUS ERRORS TO FIX:\n");
        for corr in corrections {
            prompt.push_str(&format!("- {}\n", corr));
        }
        prompt.push('\n');
    }

    prompt.push_str("OUTPUT: Complete Rust source files in markdown code blocks. Must pass cargo check.\n");
    prompt.push_str("STYLE: enum dispatch (no trait objects), tracing for logs, anyhow for errors, borrow-before-move.\n");

    truncate_to_chars(&prompt, 100_000)
}

pub fn build_reviewer_prompt(code_output: &str) -> String {
    let mut prompt = String::with_capacity(4096);
    prompt.push_str("You are a senior Rust code reviewer. Review this code for:\n");
    prompt.push_str("1. Compilation errors\n2. API mismatches\n3. Logic bugs\n4. Missing imports\n\n");
    prompt.push_str("Code:\n```rust\n");
    prompt.push_str(&truncate_to_chars(code_output, 12_000));
    prompt.push_str("\n```\n\n");
    prompt.push_str("If code is correct, respond with APPROVED. Otherwise list bugs with exact fixes.\n");
    prompt
}

pub fn build_correction_prompt(original_code: &str, errors: &[String]) -> String {
    let mut prompt = String::with_capacity(4096);
    prompt.push_str("Fix these errors in the code. Output ONLY the corrected files.\n\n");
    prompt.push_str("ERRORS:\n");
    for err in errors {
        prompt.push_str(&format!("- {}\n", err));
    }
    prompt.push_str("\nORIGINAL CODE:\n```rust\n");
    prompt.push_str(&truncate_to_chars(original_code, 8_000));
    prompt.push_str("\n```\n\nOutput corrected code:\n");
    prompt
}

fn truncate_to_chars(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text.to_string();
    }
    info!("Truncating context from {} to {} chars", text.len(), max_chars);
    text[..max_chars].to_string()
}
