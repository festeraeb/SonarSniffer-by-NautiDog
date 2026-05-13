//! Graph-Relational Indexing Extension
//!
//! Extends nautivecs from flat "chunks" to a symbol graph.
//! When a query matches a function, we also pull in the definitions
//! of types/structs it uses — preventing the LLM from inventing
//! adjacent symbols (like `floaat()`) because it has the real definitions.
//!
//! This module is a DESIGN SKETCH — the types are defined but the
//! tree-sitter extraction logic is TODO.
//!
//! How it works:
//! 1. During `index_directory`, tree-sitter visits each function/struct
//! 2. For functions: extract `type_identifier` and `call_expression` nodes
//!    → these become the `dependencies` list
//! 3. For structs: extract field types → these become dependencies too
//! 4. At query time: when a function matches, BFS its dependencies
//!    and inject those definitions into the context alongside it
//!
//! Result: The LLM gets the function PLUS the "blueprint" of every
//! custom type used inside it. No more inventing `cegpu-utils`.

use serde::{Deserialize, Serialize};

/// The type of symbol indexed in the graph
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SymbolType {
    Function,
    AsyncFunction,
    Struct,
    Enum,
    Trait,
    Impl,
    Constant,
    TypeAlias,
    Module,
}

/// Extended metadata for a code chunk — includes dependency graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolMetadata {
    /// The symbol's own name (e.g., "deploy_specialist_node")
    pub symbol_name: String,

    /// What kind of symbol this is
    pub symbol_type: SymbolType,

    /// File path where this symbol lives
    pub file_path: String,

    /// Line range in the source file
    pub line_range: (usize, usize),

    /// Is this symbol public (pub)?
    pub is_public: bool,

    /// Types/structs this symbol USES (extracted from AST)
    /// e.g., for `fn foo(node: P100Node) -> wgpu::Buffer`
    /// dependencies = ["P100Node", "wgpu::Buffer"]
    pub dependencies: Vec<String>,

    /// Functions this symbol CALLS (extracted from call_expression nodes)
    /// e.g., ["device.create_command_encoder", "encoder.begin_compute_pass"]
    pub calls: Vec<String>,

    /// Symbols that reference THIS symbol (reverse lookup, populated post-index)
    pub referenced_by: Vec<String>,

    /// The module path (e.g., "cesarops_hybrid_engine::cluster")
    pub module_path: Option<String>,
}

/// Query expansion result — the matched symbol plus its dependency tree
#[derive(Debug, Clone)]
pub struct ExpandedQueryResult {
    /// The primary match (the function/struct that matched the query)
    pub primary: SymbolMetadata,

    /// Definitions of types this symbol depends on (BFS expansion)
    /// These get injected alongside the primary match to prevent hallucination
    pub dependency_definitions: Vec<SymbolMetadata>,

    /// How deep the BFS went (1 = direct deps only, 2 = deps of deps)
    pub expansion_depth: usize,
}

/// Expand a matched symbol by pulling in its dependency definitions.
///
/// Algorithm:
/// 1. Start with the matched symbol's `dependencies` list
/// 2. For each dependency name, search the index for its definition
/// 3. If found, add to results (don't recurse deeper than `max_depth`)
/// 4. Return the primary match + all resolved dependencies
///
/// This is the key anti-hallucination mechanism:
/// - LLM asks about `deploy_specialist_node`
/// - We return the function AND the definition of `P100Node`
/// - LLM can't invent `floaat()` because it sees the real `Arc<wgpu::Device>` field
pub fn expand_symbol_context(
    _primary: &SymbolMetadata,
    _all_symbols: &[SymbolMetadata],
    _max_depth: usize,
) -> ExpandedQueryResult {
    // TODO: Implement BFS expansion
    // For now, return just the primary with no expansion
    ExpandedQueryResult {
        primary: _primary.clone(),
        dependency_definitions: Vec::new(),
        expansion_depth: 0,
    }
}

/// Extract dependencies from a tree-sitter AST node.
///
/// Visits:
/// - `type_identifier` nodes → struct/enum names used as types
/// - `call_expression` nodes → function calls made within the body
/// - `use_declaration` nodes → imported symbols
///
/// TODO: Implement with tree-sitter-rust
pub fn extract_dependencies_from_ast(_source: &str, _node: &str) -> Vec<String> {
    // Placeholder — will use tree-sitter to walk the AST
    Vec::new()
}
