# ZeroClaw Optimization Plan

> Date: 2026-04-06
> Status: Implemented

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

**Status**: ✅ Implemented

**Implementation**: `Config::load_cached()` at `src/config/schema.rs:3265` provides TTL-based (5s) config caching with file modification detection.

**Risk**: Low - config rarely changes at runtime

---

### Task 2: Tool Registry Lazy Initialization

**Area**: `src/tools/mod.rs`

**Status**: ✅ Implemented

**Implementation**: `ToolRegistry` struct (lines 126-166) uses `OnceCell` for lazy tool initialization. Call `get_or_init_tool()` to get or create tools on demand.

**Risk**: Medium - requires careful lifetime management

---

### Task 3: JSON Parsing Buffer Reuse in Agent Loop

**Area**: `src/agent/loop_.rs`

**Status**: ✅ Implemented

**Implementation**: Thread-local `JSON_EXTRACT_BUF` at line 22-25 clears and reuses a `Vec<serde_json::Value>` buffer across calls.

```rust
thread_local! {
    static JSON_EXTRACT_BUF: RefCell<Vec<serde_json::Value>> = RefCell::new(Vec::new());
}
```

**Risk**: Low - pure optimization, no behavioral change

---

### Task 4: Memory Recall Result Caching

**Area**: `src/agent/loop_.rs:218`, `src/memory/`

**Status**: ✅ Implemented

**Implementation**: Thread-local `RECALL_CACHE` (lines 22-70) provides LRU caching with 10-entry limit. Cleared on `clear_memory` operations.

**Rationale**: User messages repeat often in interactive sessions

**Risk**: Low - cache invalidation on clear_memory

---

### Task 5: Provider Response Deserialization

**Area**: `src/providers/`

**Status**: ✅ Implemented

**Implementation**: `parse_response_once()` method in Provider trait (`src/providers/traits.rs:640-644`) enables single-pass parsing with cached intermediate results.

**Risk**: Medium - affects provider abstraction

---

### Task 6: Regex Compilation in Shell Tool

**Area**: `src/tools/shell.rs`

**Status**: ✅ Implemented

**Implementation**: Module-level `static DANGEROUS_PATTERN_RE: LazyLock<RegexSet>` (lines 11-30) compiles regex patterns once at startup.

```rust
static DANGEROUS_PATTERN_RE: LazyLock<RegexSet> = LazyLock::new(|| /* ... */);
```

**Risk**: Low - existing pattern, more aggressive application

---

## Implementation Status

All 6 tasks have been verified as already implemented in the codebase:

| Task | Status | Evidence |
|------|--------|-----------|
| Task 1: Config Caching | ✅ Implemented | `src/config/schema.rs:3265` — `load_cached()` method with TTL and file modification detection |
| Task 2: Tool Registry Lazy Init | ✅ Implemented | `src/tools/mod.rs:126-166` — `ToolRegistry` struct with `OnceCell` and `get_or_init_tool()` |
| Task 3: JSON Buffer Reuse | ✅ Implemented | `src/agent/loop_.rs:22-25` — `JSON_EXTRACT_BUF` thread-local with `clear()` and `std::mem::take()` |
| Task 4: Memory Recall Caching | ✅ Implemented | `src/agent/loop_.rs:22-70` — `RECALL_CACHE` thread-local with LRU (10-entry limit) and `clear()` on memory clear |
| Task 5: Provider Response Parsing | ✅ Implemented | `src/providers/traits.rs:640-644` — `parse_response_once()` method in Provider trait |
| Task 6: Regex in Shell Tool | ✅ Implemented | `src/tools/shell.rs:11-30` — Module-level `LazyLock<RegexSet>` for dangerous patterns |

### Key Observations

1. **LazyLock usage**: The codebase already uses `LazyLock` for regex compilation in multiple locations (agent loop, shell tool)
2. **Thread-local buffers**: `JSON_EXTRACT_BUF` and `RECALL_CACHE` demonstrate hot-path optimization patterns
3. **Config caching**: `load_cached()` provides TTL-based caching with file modification detection
4. **Tool registry**: `ToolRegistry` with `OnceCell` enables lazy tool initialization on first use

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