# Code Quality Review -- captain-hook v0.1

**Date:** 2026-02-08
**Reviewer:** code-reviewer (automated)
**Scope:** All 43 files in `src/`
**Build status:** Passes

---

## Summary

The codebase implements a 6-tier permission cascade for Claude Code tool calls. The architecture is well-structured with clear module boundaries. The main concerns are: TOCTOU race conditions in file-based session management, several `unwrap()` calls on locks that could panic under contention, a non-functional queue CLI, and a full index rebuild on every insert in the embedding tier. No critical correctness bugs were found in the cascade logic itself.

---

## Findings

### CRITICAL

#### C1. Queue CLI creates a new empty queue every invocation
**File:** `src/cli/queue.rs:10-12`
**Description:** `load_queue()` constructs a brand-new `DecisionQueue::new()` on every call. The `run_queue`, `run_approve`, and `run_deny` functions will never see pending decisions from the running `check` process because the queue is entirely in-memory (not shared via IPC or file).
**Impact:** The `queue`, `approve`, and `deny` subcommands are non-functional. A user running `captain-hook approve <id>` will silently succeed but have no effect on the blocking `check` process.
**Suggested fix:** The queue must be shared via the IPC socket. `approve`/`deny` CLI commands should send a message to the supervisor socket, which owns the actual `DecisionQueue`. Alternatively, use a file-backed queue under `/tmp/captain-hook-<team>-pending.json`.

#### C2. Embedding tier full HNSW rebuild on every insert
**File:** `src/cascade/embed_sim.rs:113-136`
**Description:** `EmbeddingSimilarity::insert()` rebuilds the entire HNSW index from scratch on every new decision. `instant_distance::Builder::default().build(points, values)` is O(n log n). This is called from `CascadeRunner::persist_decision()` on every non-cache-hit tool call.
**Impact:** As the decision corpus grows, every tool call that produces a new decision becomes progressively slower. At 1000+ decisions, this could add hundreds of milliseconds to the hot path.
**Suggested fix:** Batch inserts and rebuild periodically (e.g., every N inserts or on `captain-hook build`), or use an incremental ANN index. The insert path should skip the rebuild and mark the index as stale.

---

### HIGH

#### H1. Lock poisoning panics throughout the codebase
**Files:**
- `src/cascade/cache.rs:29,37,55,89` (`.unwrap()` on `RwLock`)
- `src/cascade/token_sim.rs:36,50,104,109,132` (`.unwrap()` on `RwLock`)
- `src/cascade/embed_sim.rs:68,70,78,100,104,117,125,140,157,172,199,211,213` (`.unwrap()` on `RwLock`/`Mutex`)
- `src/cascade/human.rs:62,68,74,79,84,105,118` (`.unwrap()` on `RwLock`)

**Description:** All `RwLock::read().unwrap()` and `RwLock::write().unwrap()` calls will panic if any thread that held the lock previously panicked (lock poisoning). In a hook binary that runs per tool call this is unlikely, but in long-running supervisor mode with `tokio::spawn`, a panic in one connection handler could poison shared locks and crash all subsequent requests.
**Suggested fix:** Use `lock.read().unwrap_or_else(|e| e.into_inner())` to recover from poisoned locks, or replace `std::sync::RwLock` with `parking_lot::RwLock` (which does not poison).

#### H2. TOCTOU in session registration file operations
**File:** `src/session/registration.rs:23-44`
**Description:** `write_registration_entry` reads the file, modifies the HashMap, writes to a temp file, and renames. If two processes call `register` concurrently for different sessions, one write will be lost (last writer wins, overwriting the other's entry).
**Impact:** In multi-agent scenarios (the primary use case), concurrent session registrations could silently drop entries.
**Suggested fix:** Use file locking (`flock`) around the read-modify-write cycle, or use a per-session file scheme instead of a shared JSON file.

#### H3. TOCTOU in exclusion file operations
**File:** `src/session/mod.rs:306-318`
**Description:** `add_exclusion` and `remove_exclusion` have the same read-modify-write race as H2. Two concurrent `disable` calls could lose one entry.
**Suggested fix:** Same as H2 -- add file locking.

#### H4. Path policy does not check sensitive paths for read operations
**File:** `src/cascade/path_policy.rs:127-133`
**Description:** For read-only tools (`Read`, `Glob`, `Grep`), the path policy only checks `allow_read`. Sensitive paths like `.env*` are only checked via `sensitive_ask_write`. A `Read` tool call targeting `.env` will not trigger an `ask` -- it passes through unchecked.
**Impact:** Agents in any role can freely read sensitive files (secrets, env vars, config) without any gate.
**Suggested fix:** Add a `sensitive_ask_read` pattern set (or reuse `sensitive_ask_write`) and check it for read operations.

#### H5. Monitor command reads file content by byte offset, not line-safe
**File:** `src/cli/monitor.rs:38`
**Description:** `&contents[last_size as usize..]` slices a `String` at a byte offset derived from `fs::metadata().len()`. If a JSONL line was partially written when the metadata was read, or if the file contains multi-byte UTF-8, this could slice mid-character and panic with `byte index is not a char boundary`.
**Suggested fix:** Use a line-oriented approach: track the number of lines read rather than byte offsets, or read with `BufReader` starting from the last-known byte position and skip incomplete lines.

#### H6. Scope resolver loads ALL decisions for every scope on every call
**File:** `src/scope/mod.rs:64-66`
**Description:** `ScopeResolver::resolve()` iterates through 4 scope levels and calls `storage.load_decisions(scope)` for each. `load_decisions` reads and parses every JSONL file for that scope. For a single cache lookup, this reads up to 12 files from disk.
**Impact:** The scope resolver is called on the hot path. Parsing every JSONL file on every tool call makes the exact-cache tier's O(1) lookup pointless -- the I/O cost dominates.
**Suggested fix:** The scope resolver should work against in-memory caches (the `ExactCache` or a dedicated scope-aware cache), not disk storage. Alternatively, it should be called only as a fallback when the in-memory tiers miss.

---

### MEDIUM

#### M1. `ScopeLevel::Project` and `ScopeLevel::Role` map to the same directory
**File:** `src/storage/jsonl.rs:31,37`
**Description:** Both `ScopeLevel::Project` and `ScopeLevel::Role` resolve to `self.project_root.join("rules")`. Decisions at these two scope levels are stored in the same files, making it impossible to distinguish them during `load_decisions`.
**Suggested fix:** Give `Role` scope its own subdirectory (e.g., `rules/roles/<role_name>/`) or encode scope in the JSONL record and filter on load.

#### M2. `parse_git_remote_url` produces incorrect results for HTTPS URLs
**File:** `src/session/mod.rs:366-379`
**Description:** The HTTPS parser uses `.into()` on a `String` to convert to `Option<String>`, which always succeeds (non-empty string is `Some`). However, the logic `s.split('/').skip(1).collect::<Vec<_>>().join("/")` joins all path segments including the hostname remainder. For `https://github.com/org/repo.git`, it produces `org/repo` correctly, but for `https://gitlab.internal.com/group/subgroup/repo.git`, it would produce `group/subgroup/repo` and the `splitn(2, '/')` would yield `("group", "subgroup/repo")` instead of the subgroup+repo as project.
**Impact:** Incorrectly parsed org/project names propagate to session context, affecting cache keys and audit trails. This primarily affects self-hosted git with nested groups.
**Suggested fix:** Use the `url` crate for robust URL parsing, or at minimum handle the last two path segments rather than the first two.

#### M3. Default fallback to `/tmp` for HOME directory
**Files:**
- `src/config/policy.rs:182`
- `src/cli/mod.rs:134`
- `src/cli/check.rs:165`
- `src/cli/monitor.rs:122`
- `src/cli/build.rs:87`
- `src/cli/override_cmd.rs:87`

**Description:** `HOME` env var fallback is `/tmp`. If `HOME` is unset (unusual but possible in containers), global config is read from `/tmp/.config/captain-hook/config.yml`. This is world-readable/writable on most systems.
**Impact:** An attacker with access to `/tmp` could plant a malicious `config.yml` with a rogue supervisor socket path.
**Suggested fix:** If HOME is unset, error out or use a secure fallback like the user's passwd entry (`dirs` crate).

#### M4. `dirs_global()` duplicated in 5 files
**Files:**
- `src/cli/mod.rs:133-136`
- `src/cli/check.rs:164-167`
- `src/cli/monitor.rs:121-124`
- `src/cli/build.rs:86-89`
- `src/cli/override_cmd.rs:86-89`
- `src/config/policy.rs:181-184`

**Description:** The same 4-line function is copy-pasted 6 times across the codebase.
**Suggested fix:** Extract to a single location (e.g., `config::dirs_global()`) and import everywhere.

#### M5. Embedding model fallback creates a second `EmbeddingSimilarity` that panics
**File:** `src/cli/check.rs:93-99`
**Description:** If the first `EmbeddingSimilarity::new("default", ...)` fails (model not available), the fallback tries to create another one with threshold 999.0. If the underlying fastembed model init fails both times, the `unwrap_or_else(|_| panic!(...))` will crash the hook process.
**Impact:** On systems where fastembed model download fails (offline, no disk space), every tool call will crash.
**Suggested fix:** Create a no-op `EmbeddingSimilarity` variant or make the embedding tier fully optional (skip it in the cascade if unavailable).

#### M6. `truncate()` in queue.rs can panic on multi-byte UTF-8
**File:** `src/cli/queue.rs:116-122`
**Description:** `&s[..max]` slices by byte offset. If `max` falls inside a multi-byte character, this panics.
**Suggested fix:** Use `s.chars().take(max).collect::<String>()` or `s.char_indices()`.

#### M7. `sk-` prefix is too broad for aho-corasick sanitization
**File:** `src/sanitize/aho.rs:26`
**Description:** The prefix `"sk-"` matches any token starting with `sk-`. This will redact non-secret content like shell variable names (`$sk-something`), configuration keys, or natural-language text containing `sk-` as a substring.
**Impact:** False-positive redaction corrupts cache keys and may cause Jaccard/embedding similarity mismatches.
**Suggested fix:** Either remove the generic `sk-` prefix (keep the more specific `sk-ant-`) or add a minimum-length requirement specifically for `sk-` matches (e.g., require at least 20 characters after the prefix).

#### M8. Cascade default-deny record incorrectly reports PathPolicy tier
**File:** `src/cascade/mod.rs:131-136`
**Description:** When no cascade tier resolves, the fallback `DecisionRecord` sets `tier: DecisionTier::PathPolicy` with reason "no cascade tier resolved; default deny". This is misleading -- the decision came from the cascade runner itself, not from path policy evaluation.
**Suggested fix:** Add a `DecisionTier::Default` variant or use a distinct reason/tier to avoid confusion in audit logs and the monitor view.

#### M9. `aho-corasick` `AhoCorasick::new` uses `expect` that panics
**File:** `src/sanitize/aho.rs:14`
**Description:** `AhoCorasick::new(&prefixes).expect("valid aho-corasick patterns")` panics if patterns are invalid. Since `new()` accepts user-provided prefixes (via custom pipeline), invalid input would crash the process.
**Suggested fix:** Return a `Result` from `AhoCorasickSanitizer::new()` instead of panicking.

---

### LOW

#### L1. Unused import in `cascade/human.rs`
**File:** `src/cascade/human.rs:12`
**Description:** `ScopeLevel` is imported as `ScopeLevelType` but `ScopeLevel` is also imported via `crate::decision`. The alias `ScopeLevelType` is used in `HumanResponse` but is identical to `ScopeLevel`.
**Suggested fix:** Remove the aliased import; use `ScopeLevel` consistently.

#### L2. `let _ = i;` dead code in embed_sim.rs
**File:** `src/cascade/embed_sim.rs:91`
**Description:** `let _ = i;` is a no-op statement that appears to be leftover from development.
**Suggested fix:** Remove the line.

#### L3. `save_index` and `load_index` are no-ops
**File:** `src/cascade/embed_sim.rs:183-194`
**Description:** Both methods return `Ok(())` without doing anything. The comment explains the limitation, but these methods are public API that callers might rely on.
**Suggested fix:** Either implement them (serialize entries as JSONL and rebuild on load) or remove them and document that index persistence is not yet supported.

#### L4. `rebuild_index` on `StorageBackend` is a no-op
**File:** `src/storage/jsonl.rs:164-169`
**Description:** The `rebuild_index` method on `JsonlStorage` does nothing. It's required by the `StorageBackend` trait but never meaningfully implemented.
**Suggested fix:** Either remove from the trait (index rebuilding is not a storage concern) or implement it by calling into the appropriate cascade tier.

#### L5. `_session` parameter unused in `ScopeResolver::resolve`
**File:** `src/scope/mod.rs:53`
**Description:** The `SessionContext` parameter is accepted but never used (prefixed with `_`).
**Suggested fix:** Remove if not needed, or use it to filter by session's role for more targeted scope resolution.

#### L6. Decision priority duplicated between `decision.rs` and `scope/merge.rs`
**Files:**
- `src/decision.rs:82-88` (`Decision::precedence`)
- `src/scope/merge.rs:27-33` (`decision_priority`)

**Description:** The same precedence logic (Deny=3, Ask=2, Allow=1) is implemented twice.
**Suggested fix:** Use `Decision::precedence()` in `merge.rs` instead of the local `decision_priority` function.

#### L7. `serde_yaml` is used but not in Cargo.toml dependencies
**Files:** `src/config/policy.rs:61`, `src/config/roles.rs:96`
**Description:** The code calls `serde_yaml::from_str` but `serde_yaml` was not listed in the CLAUDE.md dependencies table. If it is in Cargo.toml, this is fine. If not, the build would fail.
**Suggested fix:** Verify `serde_yaml` is in Cargo.toml. If so, add it to the CLAUDE.md dependency table.

#### L8. `check.rs` exits with code 1 after writing hook output
**File:** `src/cli/check.rs:156-158`
**Description:** After writing a `Deny` hook output to stdout, the process calls `std::process::exit(1)`. This skips Rust destructors and may leave buffered I/O unflushed (though `write_hook_output` explicitly locks and writes). The `main()` function returns `anyhow::Result<()>`, so a non-zero exit could be achieved by returning `Err`.
**Suggested fix:** Return an error from `run()` and let `main()` handle the exit code, or ensure stdout is explicitly flushed before `exit(1)`.

---

## Architecture Observations

1. **Good separation of concerns**: The cascade tier trait with `evaluate()` returning `Option<DecisionRecord>` is clean. Tiers compose well and the fall-through semantics are easy to reason about.

2. **Sanitization pipeline is solid**: The three-layer approach (literal prefix, regex, entropy) covers a good range of secret formats. The pipeline runs before any storage or cache operation, which is correct.

3. **Session management is the weakest module**: It mixes in-memory (`DashMap` with `LazyLock`) and file-based state without proper synchronization between them. The global static `SESSIONS` is only useful within a single process invocation, which is fine for the hook binary but misleading in the codebase.

4. **The scope resolver is architecturally misplaced**: It reads from disk on every call but sits in the hot path. It should either be removed (letting the cascade tiers handle scope-aware lookup) or cached.

5. **The IPC layer is clean**: Socket server/client with proper timeout handling and JSON-line protocol. Good use of `tokio::select!` for shutdown.

---

## Recommendations (Priority Order)

1. **Fix the queue CLI** (C1) -- this is the most user-visible bug
2. **Add file locking to registration** (H2, H3) -- required for multi-agent safety
3. **Make embedding insert incremental or batched** (C2) -- performance cliff
4. **Add sensitive path check for reads** (H4) -- security gap
5. **Deduplicate `dirs_global()`** (M4) -- low-effort cleanup
6. **Replace `unwrap()` on locks** (H1) -- robustness in long-running mode
