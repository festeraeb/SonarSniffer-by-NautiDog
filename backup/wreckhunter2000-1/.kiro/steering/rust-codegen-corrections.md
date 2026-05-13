# Rust Code Generation Corrections — Learned from LLM Output

These are recurring compiler errors that LLMs produce when writing Rust code.
Inject these as corrections when the system generates Rust.

## Trait Object Safety (dyn dispatch)

WRONG: Using `Box<dyn MyTrait>` when the trait has async methods.
```rust
// This fails: async fn makes traits not object-safe
#[async_trait]
trait SearchEngine {
    async fn search(&self, query: &str) -> Result<Vec<Finding>>;
}
let engine: Box<dyn SearchEngine> = Box::new(SearXNG::new());
```

RIGHT: Use enum dispatch instead of trait objects for async traits.
```rust
enum SearchBackend {
    SearXNG(SearXNGSearch),
    GoogleCSE(GoogleCSESearch),
}
impl SearchBackend {
    async fn search(&self, query: &str) -> Result<Vec<Finding>> {
        match self {
            Self::SearXNG(s) => s.search(query).await,
            Self::GoogleCSE(g) => g.search(query).await,
        }
    }
}
```

## Borrow After Move

WRONG: Using a value after it's been moved into a struct or loop.
```rust
let results = compute_results();
Ok(Response {
    findings: results,          // moves results
    total: results.len(),       // ERROR: borrowed after move
})
```

RIGHT: Compute derived values before the move.
```rust
let results = compute_results();
let total = results.len();      // compute BEFORE move
Ok(Response {
    findings: results,
    total,
})
```

WRONG: Iterating over a Vec then using it again.
```rust
for url in to_remove { cache.remove(&url); }
if !to_remove.is_empty() { ... }  // ERROR: moved
```

RIGHT: Iterate over a reference.
```rust
for url in &to_remove { cache.remove(url); }
if !to_remove.is_empty() { ... }  // OK: only borrowed
```

## String Type Mismatches (&String vs &str)

WRONG: Mixing &String and &str in tuple arrays.
```rust
let params = [
    ("key", &self.api_key),   // &String
    ("q", query),             // &str
];
```

RIGHT: Use Vec with explicit type or .as_str().
```rust
let params: Vec<(&str, &str)> = vec![
    ("key", self.api_key.as_str()),
    ("q", query),
];
```

## Scraper/Tendril Text Collection

WRONG: Trying to .join() on scraper's text() iterator directly.
```rust
let text = element.text().collect::<Vec<_>>().join(" ");  // Tendril, not &str
```

RIGHT: Map to String first.
```rust
let text = element.text().map(|t| t.to_string()).collect::<Vec<_>>().join(" ");
```

## Ambiguous Numeric Types

WRONG: Calling methods on untyped float literals.
```rust
let score = (1.0 / (k + rank)).min(1.0);  // ambiguous: f32 or f64?
```

RIGHT: Annotate the type.
```rust
let score: f32 = (1.0f32 / (k as f32 + rank as f32)).min(1.0);
```

## Match Arm Type Consistency

WRONG: Returning &str from some arms and String from others.
```rust
match entity.as_str() {
    "amp" => "&",           // &str
    _ => format!("&{};", entity),  // String
}
```

RIGHT: Make all arms return the same type.
```rust
match entity.as_str() {
    "amp" => "&".to_string(),
    "lt" => "<".to_string(),
    _ => format!("&{};", entity),
}
```

## Iterator .collect() Type Inference

WRONG: Collecting into a Vec without type annotation when the compiler can't infer.
```rust
let results = response.items.map(|w| w.results.into_iter().map(|r| convert(r)).collect()).unwrap_or_default();
```

RIGHT: Annotate the binding.
```rust
let results: Vec<MyResult> = response.items.map(|w| { ... }).unwrap_or_default();
```
