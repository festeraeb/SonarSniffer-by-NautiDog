# Tool-Calling Pivot — cesarops-forge

## The Fix

Switch from open-ended text generation to structured tool calling.
The LLM emits a tool call, the forge executes it, returns the result.
No more repetition loops. No more hallucinated file lists.

## Endpoint Change

```
OLD: POST /api/v1/generate {"prompt": "...", "max_length": 8192}
NEW: POST /v1/chat/completions {"messages": [...], "tools": [...]}
```

KoboldCPP supports the OpenAI-compatible chat completions endpoint.
Tool calls terminate generation automatically — impossible to loop.

## Tool Schemas

### 1. write_file
```json
{
  "type": "function",
  "function": {
    "name": "write_file",
    "description": "Write content to a file on the RAID. Creates parent directories if needed.",
    "parameters": {
      "type": "object",
      "properties": {
        "path": {
          "type": "string",
          "description": "Relative path from project root (e.g. 'src/scheduler.rs')"
        },
        "content": {
          "type": "string",
          "description": "The complete file content to write"
        }
      },
      "required": ["path", "content"]
    }
  }
}
```

### 2. read_file
```json
{
  "type": "function",
  "function": {
    "name": "read_file",
    "description": "Read the contents of a file from the project.",
    "parameters": {
      "type": "object",
      "properties": {
        "path": {
          "type": "string",
          "description": "Relative path from project root"
        }
      },
      "required": ["path"]
    }
  }
}
```

### 3. cargo_check
```json
{
  "type": "function",
  "function": {
    "name": "cargo_check",
    "description": "Run cargo check on the project. Returns compiler errors or 'OK' if it passes.",
    "parameters": {
      "type": "object",
      "properties": {
        "project_dir": {
          "type": "string",
          "description": "Path to the crate directory containing Cargo.toml"
        }
      },
      "required": ["project_dir"]
    }
  }
}
```

### 4. search_codebase
```json
{
  "type": "function",
  "function": {
    "name": "search_codebase",
    "description": "Search the CESAROPS codebase via nautivecs for relevant code snippets.",
    "parameters": {
      "type": "object",
      "properties": {
        "query": {
          "type": "string",
          "description": "Short keyword query (3-5 words max)"
        },
        "top_k": {
          "type": "integer",
          "description": "Number of results to return (default 5)"
        }
      },
      "required": ["query"]
    }
  }
}
```

### 5. run_command
```json
{
  "type": "function",
  "function": {
    "name": "run_command",
    "description": "Execute a shell command and return stdout/stderr. Use for nvidia-smi, ls, cat, etc.",
    "parameters": {
      "type": "object",
      "properties": {
        "command": {
          "type": "string",
          "description": "The shell command to execute"
        },
        "timeout_secs": {
          "type": "integer",
          "description": "Timeout in seconds (default 30)"
        }
      },
      "required": ["command"]
    }
  }
}
```

### 6. search_web
```json
{
  "type": "function",
  "function": {
    "name": "search_web",
    "description": "Search the internet via cesarops-wso for documentation, GitHub issues, Stack Overflow.",
    "parameters": {
      "type": "object",
      "properties": {
        "query": {
          "type": "string",
          "description": "Search query (keep under 100 chars)"
        }
      },
      "required": ["query"]
    }
  }
}
```

## System Prompt (for chat completions)

```
You are the CesarOps autonomous developer agent running on dual P100 GPUs.
You have tools available. USE THEM instead of generating code in plain text.

WORKFLOW:
1. When asked to implement something, FIRST call search_codebase to find relevant patterns
2. Then call write_file to create/modify files
3. Then call cargo_check to verify your changes compile
4. If cargo_check fails, read the errors and fix them by calling write_file again
5. Report the result to the user

RULES:
- ALWAYS use write_file to create code. Never output raw code in your response.
- ALWAYS call cargo_check after writing code.
- Keep search queries short (3-5 keywords).
- If you don't know something, call search_codebase or search_web.
- Never hallucinate file paths or task IDs.
```

## Implementation in forge-web

The forge-web main.rs needs to:
1. Switch from `/api/v1/generate` to `/v1/chat/completions`
2. Include the tool definitions in every request
3. When the response contains a `tool_calls` array, execute each tool
4. Return tool results back to the model as a follow-up message
5. Loop until the model responds with plain text (no more tool calls)

## Why This Kills Repetition

- Tool calls are structured JSON with a defined schema
- The model generates `{"name": "write_file", "arguments": {...}}` and STOPS
- There is no open-ended continuation where it can loop
- Each action is small (50-200 tokens), verifiable, and terminates cleanly
- The forge validates every action before returning results

## Fallback

If KoboldCPP's /v1/chat/completions doesn't support tool_calls properly:
- Parse the model's output for XML-style tool calls: `<tool>write_file</tool><args>...</args>`
- The model can be prompted to emit structured XML instead of JSON
- The forge parses the XML and executes the tool
- Same effect, different wire format
