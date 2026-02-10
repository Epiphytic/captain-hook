# ADR: Dependency Selection for hookwise

**Date:** 2026-02-08
**Status:** Accepted
**Context:** Choosing core dependencies for vector similarity, embedding generation, and glob matching in the hookwise permission gating system.

## Decision

| Component | Selected | Replaces |
|-----------|----------|----------|
| HNSW vector similarity | `instant-distance` | `ruvector` |
| Local embedding generation | `fastembed` | `ruvllm` |
| Glob pattern matching | `globset` | `glob` |

## fastembed over ruvllm

### Why fastembed

- **Local ONNX inference**: Uses ONNX Runtime to run embedding models locally. No external API calls, no network latency, no API keys required.
- **Cached models**: Downloads models once on first use, caches them locally in `~/.cache/fastembed/`. Subsequent runs are instant startup.
- **No GPU required**: Runs on CPU with acceptable performance for our use case (embedding short command strings, not large documents).
- **Multiple model support**: Supports a range of embedding models. We use `BAAI/bge-small-en-v1.5` (33M params, 384 dimensions) — small enough for fast inference, large enough for meaningful command similarity.
- **Rust-native**: Pure Rust bindings to ONNX Runtime. Integrates cleanly with our async Rust binary.
- **Batch embedding**: Can embed multiple inputs in a single call, useful when rebuilding the index from cached decisions.

### Why not ruvllm

- `ruvllm` requires a running LLM server or external API. This adds operational complexity for what should be a lightweight, self-contained binary.
- Embedding generation via a full LLM is overkill for short command strings. A dedicated embedding model is faster and more appropriate.
- External API dependency means network failures can block permission decisions.

### Alternatives considered

| Alternative | Reason rejected |
|-------------|-----------------|
| OpenAI embeddings API | External API dependency, requires API key, adds latency, costs money per call |
| `candle` (Hugging Face) | More flexible but lower-level; fastembed provides the right abstraction for our needs |
| `rust-bert` | Heavier dependency, PyTorch-based, more complex build process |
| Pre-computed embeddings only | Would require shipping embeddings with decisions, preventing similarity search on novel commands |

### Tradeoffs

- **First-run download**: ~50MB model download on first use. Acceptable for a dev tool; subsequent runs are instant.
- **Binary size**: ONNX Runtime adds ~20MB to the binary. Acceptable for a CLI tool.
- **CPU inference latency**: ~5-15ms per embedding on modern hardware. Well within our <5ms target when amortized (embeddings are cached alongside decisions).

## instant-distance over ruvector

### Why instant-distance

- **Pure Rust HNSW**: No C/C++ dependencies, no FFI, no build complexity. Compiles cleanly on all platforms.
- **Serde support**: Index structures implement `Serialize`/`Deserialize`. We can save/load indexes directly to/from disk without custom serialization code.
- **Well-maintained**: Active maintenance, clear API, good documentation.
- **Correct HNSW implementation**: Implements the standard Hierarchical Navigable Small World algorithm with configurable construction and search parameters.
- **Small dependency footprint**: Minimal transitive dependencies.

### Why not ruvector

- `ruvector` bundles its own LLM inference, which overlaps with our separate embedding choice (fastembed). We want to decouple embedding generation from index storage.
- Less mature ecosystem compared to instant-distance.
- Custom serialization format rather than standard serde.

### Alternatives considered

| Alternative | Reason rejected |
|-------------|-----------------|
| `hnsw` crate | Less actively maintained, API less ergonomic |
| `annoy-rs` | Annoy (Spotify) is read-only after build; HNSW supports incremental inserts which we need for live updates |
| `faiss` (via FFI) | C++ dependency, complex build, overkill for our index sizes (hundreds to low thousands of entries) |
| Linear scan | O(n) scan is fine for <100 entries but doesn't scale. HNSW gives us O(log n) with negligible overhead at small n. |

### Tradeoffs

- **No GPU acceleration**: Pure Rust means CPU only. Fine for our index sizes (<10K entries typically).
- **Memory**: HNSW indexes are fully in-memory. For our expected sizes (hundreds of decision embeddings), this is negligible (<1MB).
- **Incremental updates**: instant-distance supports building a new index but not incremental insertion into an existing one. We rebuild the index lazily, which is fast for our sizes (<100ms for 1000 entries).

## globset over glob

### Why globset

- **Compiled glob sets**: Compiles multiple glob patterns into a single automaton. Matching a path against N patterns is nearly as fast as matching against 1 pattern.
- **Batch matching**: `GlobSet::matches()` returns all matching pattern indices in a single pass. Critical for our path policy evaluation where we check deny_write, allow_write, and sensitive_paths simultaneously.
- **Much faster for many patterns**: A role definition can have 10-20 glob patterns. With `glob`, each pattern is matched independently (10-20 filesystem traversals). With `globset`, all patterns are compiled once and matched in a single pass (~1us).
- **Same glob syntax**: Compatible with the glob patterns already defined in `roles.yml`. No migration needed.
- **Part of the ripgrep ecosystem**: Well-tested, battle-hardened code from the ripgrep/BurntSushi ecosystem.

### Why not glob

- The `glob` crate matches one pattern at a time. For N patterns, you pay N * O(path_length). With 12 roles and ~15 patterns each, this adds up.
- No compiled representation — each `glob::Pattern::matches()` call re-interprets the pattern.
- No batch matching API.

### Alternatives considered

| Alternative | Reason rejected |
|-------------|-----------------|
| `glob` | Per-pattern matching, no batch API, slower for many patterns |
| `ignore` | Higher-level gitignore-style matching; more than we need, less control |
| Manual regex conversion | Error-prone, would need to handle glob edge cases ourselves |
| `wax` | Newer, less battle-tested than globset |

### Tradeoffs

- **Startup cost**: Compiling a `GlobSet` takes ~10-50us depending on pattern count. This is a one-time cost at role initialization.
- **Slightly larger API surface**: `globset` has more types (`Glob`, `GlobSet`, `GlobSetBuilder`, `GlobMatcher`). The additional complexity is justified by the performance gains.
