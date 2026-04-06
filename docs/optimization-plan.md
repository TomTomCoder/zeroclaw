# ZeroClaw Optimization Plan

> Date: 2026-04-06
> Status: Planning

This document outlines identified performance optimization opportunities across the ZeroClaw codebase. Each task is listed as a standalone improvement that can be implemented independently.

## Executive Summary

The codebase already has several optimization patterns in place:
- LazyLock for regex compilation (`src/agent/loop_.rs`)
- Release profile with LTO and size optimization (`Cargo.toml`)
- Async tool execution with parallel support

However, several hot paths show additional optimization potential.

---

## Tasks

### Task 1: Config Caching for Repeated Loads

**Area**: `src/config/schema.rs`, `src/main.rs`

**Current State**: `Config::load_or_init()` reads and parses config files on every call, including in hot paths like `peripherals/mod.rs:79`.

**Proposed Change**: Add a cached config loader with TTL-based invalidation.

```rust
// Proposed API in src/config/schema.rs
impl Config {
    pub async fn load_cached() -> Result<Arc<Config>> {
        // With 5-second TTL, cache config in static
        // Invalidate on file modifications
    }
}
```

**Risk**: Low - config rarely changes at runtime

**Validation**: Run benchmarks before/after with `cargo bench`

---

### Task 2: Tool Registry Lazy Initialization

**Area**: `src/tools/mod.rs`

**Current State**: `default_tools_with_runtime()` creates all tool Arcs eagerly on every call to `build_tools()`.

**Proposed Change**: Lazy-initialize individual tools on first use rather than all at once.

```rust
// In ToolRegistry
pub fn get_or_init_tool(&self, name: &str) -> Option<Box<dyn Tool>> {
    // Use OnceCell for lazy tool creation
}
```

**Risk**: Medium - requires careful lifetime management

---

### Task 3: JSON Parsing Buffer Reuse in Agent Loop

**Area**: `src/agent/loop_.rs`

**Current State**: `extract_json_values()` allocates new `Vec` on each call. Called repeatedly during tool call parsing.

**Proposed Change**: Use a thread-local buffer that gets cleared and reused.

```rust
thread_local! {
    static JSON_EXTRACT_BUF: RefCell<Vec<serde_json::Value>> = RefCell::new(Vec::new());
}
```

**Risk**: Low - pure optimization, no behavioral change

---

### Task 4: Memory Recall Result Caching

**Area**: `src/agent/loop_.rs:218`, `src/memory/`

**Current State**: `build_context()` calls `mem.recall()` on every agent turn with no result caching.

**Proposed Change**: Add in-memory LRU cache for recent recall results.

```rust
// In build_context
use std::collections::VecDeque;
struct RecallCache { /* LRU with 10-entry limit */ }
```

**Rationale**: User messages repeat often in interactive sessions

**Risk**: Low - cache invalidation on clear_memory

---

### Task 5: Provider Response Deserialization

**Area**: `src/providers/`

**Current State**: Provider responses may be deserialized multiple times (raw text, then structured, then text again).

**Proposed Change**: Single-pass parsing with cached intermediate results.

```rust
// In Provider trait
fn parse_response_once(response: RawResponse) -> ParsedResponse {
    // Parse once, reuse across method calls
}
```

**Risk**: Medium - affects provider abstraction

---

### Task 6: Regex Compilation in Shell Tool

**Area**: `src/tools/shell.rs`

**Current State**: Security policy regexes are compiled per-SecurityPolicy instance.

**Proposed Change**: Move to module-level LazyLock like agent loop does.

```rust
// In src/tools/shell.rs
static DANGEROUS_PATTERN_RE: LazyLock<RegexSet> = LazyLock::new(|| /* ... */);
```

**Risk**: Low - existing pattern, more aggressive application

---

## Non-Goals (Explicit)

This plan does NOT include:

1. **Binary size reduction via feature flags**: Already aggressive in Cargo.toml
2. **Parallel LLM calls**: Not safe for single-turn conversations
3. **Connection pooling**: Added complexity, defer to provider-level optimization
4. **Hot reload of config**: Out of scope for this plan

---

## Benchmarking Strategy

Run benchmarks before any task:

```bash
cargo bench --bench agent_benchmarks
```

Expected metrics:
- Agent turn latency (ms)
- Config load latency (ms)
- Tool execution wall-clock time

---

## Dependencies Between Tasks

```
Task 1 (Config caching)
  └─ Task 4 (Memory caching) - builds on config caching pattern

Task 3 (JSON buffer)
  └─ Task 2 (Tool registry) - independent

Task 5 (Provider parsing)
  └─ Task 6 (Regex) - independent
```

---

## Recommendations

1. Start with Task 1 (Config caching) as it affects multiple call sites
2. Task 3 provides the most direct hot-path improvement
3. Validate each task with benchmarks before moving on

---

_This plan is a living document. Add new tasks by creating a new section above with task number, area, current state, and proposed change._