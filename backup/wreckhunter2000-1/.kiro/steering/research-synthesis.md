---
inclusion: fileMatch
fileMatchPattern: "**/research_engine*"
---

# Sovereign Research & Synthesis (SRS) Loop

## Pattern: The Librarian

The LLM acts as a Research Lead, not an expert. It:
1. **Plans** — decomposes the query into specific sub-queries
2. **Fetches** — fires sub-queries to nautivecs + external oracles in parallel
3. **Synthesizes** — assembles findings section by section
4. **Stores** — successful findings get added to the vector store for next time

## Research Phase Structure

```rust
ResearchPhase {
    sub_queries: Vec<String>,    // 3-5 specific lookup tasks
    findings: Vec<Finding>,       // Results from each source
    conflicts: Vec<Conflict>,     // Disagreements between sources
    synthesis: String,            // Final assembled answer
}
```

## Finding Sources (Priority Order)

1. **Human Corrections** — highest priority, from n8n feedback store
2. **Local Code** (nautivecs) — actual codebase, grounded truth
3. **Web Research** — external APIs (Gemini Flash, Groq, Google CSE)
4. **LLM Training Data** — lowest priority, only for general knowledge

## Conflict Resolution

When sources disagree:
- Local code + Human correction → trust the correction
- Local code + Web research → flag `[CONFLICT]`, ask human
- Web research + LLM training → trust web research (more current)
- Never trust LLM training data over any other source

## Vetting Filter for Web Results

- Summarize web pages to 3 bullets related to the specific task
- Only accept results from trusted domains (arxiv, docs.rs, github, official docs)
- Cross-reference against nautivecs — if web says X but code does Y, flag it

## Performance Contract

- Planning phase: < 1 second (1.5B model, fast)
- Parallel fetch: < 5 seconds (network-bound)
- Synthesis per section: < 3 seconds (1.5B model)
- Total for complex query: < 15 seconds
- Simple grounded query (nautivecs only): < 5 seconds
