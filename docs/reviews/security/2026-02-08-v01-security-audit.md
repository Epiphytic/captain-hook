# Security Audit: captain-hook v0.1

**Date:** 2026-02-08
**Reviewer:** security-reviewer (automated)
**Scope:** All 43 source files in `src/`
**Build status:** Passing

---

## Executive Summary

captain-hook is a security-critical permission gating system for Claude Code. This audit reviewed all Rust source files across the sanitization pipeline, permission model, cascade engine, IPC layer, session management, storage, and CLI. The codebase is generally well-structured with clear separation of concerns. However, several security-relevant issues were identified, primarily around the sanitization pipeline's bypass vectors, file-based state management in `/tmp`, and the path policy engine's bash command parsing limitations.

**Finding summary:**
- CRITICAL: 2
- HIGH: 5
- MEDIUM: 8
- LOW: 6

---

## 1. Secret Sanitization Completeness

### CRITICAL-01: Secrets bypass via encoding (base64, URL-encoding, Unicode)

**Severity:** CRITICAL
**Affected files:** `src/sanitize/aho.rs`, `src/sanitize/regex_san.rs`, `src/sanitize/entropy.rs`
**Description:** The sanitization pipeline operates only on the raw text representation. Secrets that are base64-encoded, URL-encoded, hex-encoded, or use Unicode homoglyphs will pass through all three layers undetected.

**Attack vectors:**
- `echo "c2stYW50LTEyMzQ1Njc4OQ==" | base64 -d > token.txt` -- base64-encoded `sk-ant-*` token passes aho-corasick and regex layers entirely.
- `export API_KEY=sk%2Dant%2D123456789` -- URL-encoded prefix `sk-ant-` bypasses literal prefix matching.
- `export TOKEN=\u0073\u006b-ant-secret123` -- Unicode escape sequences bypass prefix matching.
- String concatenation: `export KEY="sk-" + "ant-" + "secret123"` -- split across expressions.

**Remediation:**
- Add a base64 detection layer: check for strings matching `^[A-Za-z0-9+/=]{40,}$` after delimiters, attempt decode, run sanitization on decoded output.
- Add URL-decode normalization before running the aho-corasick layer.
- Consider hex-decode normalization for `\x` and `\u` escape sequences.
- Document that runtime concatenation is out of scope (not statically detectable).

---

### CRITICAL-02: Entropy sanitizer only triggers after `=` or `:` delimiters

**Severity:** CRITICAL
**Affected file:** `src/sanitize/entropy.rs:49-91`
**Description:** The `EntropySanitizer` only scans tokens following `=` or `:` delimiters. High-entropy secrets passed as bare arguments (e.g., positional CLI args, inline in URLs without `=`, piped from other commands) are never checked by the entropy layer.

**Attack vectors:**
- `curl https://api.example.com/v1/data -H "sk-ant-longSecretToken123456789"` -- if prefix not matched, entropy won't catch it because there's no `=`/`:` preceding it.
- `echo longHighEntropySecretString | some-command` -- no delimiter, no entropy check.
- `./deploy longHighEntropyApiKey123456789012` -- positional arg, no delimiter.

**Remediation:**
- Add a standalone token scanner that splits on whitespace and checks entropy for all tokens above the length threshold, not just those following delimiters.
- Consider a secondary entropy pass on all whitespace-delimited tokens that are at least `min_length` characters.

---

### HIGH-01: Aho-corasick token boundary detection is fragile

**Severity:** HIGH
**Affected file:** `src/sanitize/aho.rs:59-61`
**Description:** Token boundaries are defined as whitespace, `"`, `'`, `,`, or `;`. Secrets embedded in other contexts (JSON values with `}`, `]`, or `)` as delimiters, or within backticks, or in heredocs) will cause over-redaction or under-redaction.

**Example:** In `{"key":"ghp_abc123def"}`, the token boundary scan extends past `}` and `"`, so this particular case works because `"` is a delimiter. But `(ghp_abc123def)` would include the `)` in the redacted span since `)` is not a listed delimiter.

**Remediation:**
- Extend token boundary characters to include `}`, `]`, `)`, `` ` ``, `\n`, `\r`.

---

### HIGH-02: Missing well-known secret prefixes

**Severity:** HIGH
**Affected file:** `src/sanitize/aho.rs:22-41`
**Description:** The default prefix list covers major providers but is missing several common secret formats:

Missing prefixes:
- `whsec_` (Stripe webhook secrets)
- `sk_live_` / `sk_test_` (Stripe API keys)
- `rk_live_` / `rk_test_` (Stripe restricted keys)
- `SG.` (SendGrid API keys)
- `xoxs-` (Slack user tokens)
- `xoxa-` (Slack app tokens)
- `dop_v1_` (DigitalOcean)
- `nrk-` / `NRAK-` (New Relic)
- `hf_` (Hugging Face)
- `vlt_` / `hvs.` (HashiCorp Vault)
- `op_` (1Password)
- `AIzaSy` (Google API keys)
- `ya29.` (Google OAuth tokens)
- `PRIVATE KEY` (in addition to `-----BEGIN`)

**Remediation:**
- Expand the default prefix list, ideally sourcing from the gitleaks regex database (which research task #8 already curated).

---

### MEDIUM-01: Regex sanitizer patterns have false negative gaps

**Severity:** MEDIUM
**Affected file:** `src/sanitize/regex_san.rs:34-46`
**Description:** The regex patterns have several gaps:
1. Bearer token pattern requires 20+ chars but some OAuth tokens are shorter.
2. API key assignment pattern `\S{8,}` can miss values containing spaces in quotes: `api_key="my secret key"` matches but the value captured is only `"my`.
3. Connection string pattern only covers `postgres|mysql|mongodb|redis|amqp` -- missing `mssql`, `sqlserver`, `oracle`, `cockroachdb`, etc.
4. CLI flag pattern uses `\S{8,}` which won't match values with `=` separator (e.g., `--password=mysecret`).

**Remediation:**
- Add `=` variants for CLI flag patterns: `--password=\S{8,}`.
- Extend connection string pattern to cover additional databases.
- Consider quoted value handling in API key patterns.

---

## 2. Permission Model Integrity

### HIGH-03: `ask` can be silently downgraded to `allow` via similarity tiers

**Severity:** HIGH
**Affected files:** `src/cascade/token_sim.rs:161-163`, `src/cascade/embed_sim.rs:251-253`
**Description:** Both similarity tiers (Jaccard and embedding) propagate `ask` decisions from cached records. However, the caching logic in `CascadeRunner::persist_decision()` at `src/cascade/mod.rs:103-107` skips persisting decisions from `TokenJaccard` and `EmbeddingSimilarity` tiers. This means if a similarity tier returns `ask`, that `ask` decision is **not** cached back into the exact cache or JSONL. On the **next** invocation of the same (now exact) input, the exact cache will not have an entry, and the similarity tier might match a **different** cached entry that is `allow` instead of `ask`.

**Attack vector:**
1. User marks `rm -rf /important` as `ask` at the human tier.
2. An attacker issues `rm -rf /important --verbose` (similar but not exact).
3. Similarity tier matches the `ask` entry, returns `ask`. Decision is NOT persisted.
4. Attacker issues `rm -rf /important --verbose` again. Exact cache misses. Similarity tier might now match a different `allow` entry with slightly higher score.

**Remediation:**
- Persist all decisions from similarity tiers to the exact cache, including `ask`. This ensures that once a decision is made for a specific sanitized input, it is exact-matched on subsequent calls.
- Alternatively, similarity-matched `ask` decisions should always be escalated regardless of caching behavior (which they are at the tier level, but the non-persistence means repeated calls can drift).

---

### HIGH-04: Scope merge ignores scope precedence (Org > Project > User > Role)

**Severity:** HIGH
**Affected files:** `src/scope/merge.rs:6-25`, `src/scope/hierarchy.rs:1-13`
**Description:** The `merge_decisions()` function selects the decision with the highest `decision_priority` (DENY > ASK > ALLOW) but completely ignores scope precedence. The `ScopeLevel::precedence()` method exists in `hierarchy.rs` but is never called during merge.

The documented precedence is "Org > Project > User > Role", meaning an Org-level `allow` should override a Role-level `deny`. But the current implementation treats all scopes equally and just picks the most restrictive decision regardless of source scope.

**Impact:** While "most restrictive wins" is a safe default (it can never cause a deny to become an allow), it deviates from the documented behavior. A user who sets an org-level `allow` for a specific tool would expect it to override a role-level `deny`, but it won't.

**Remediation:**
- Implement tie-breaking: when two decisions have the same priority, prefer the one from the higher scope.
- Or, document that the actual behavior is "most restrictive wins regardless of scope" and remove the misleading scope precedence method.

---

### MEDIUM-02: Exact cache returns `ask` decisions that auto-resolve

**Severity:** MEDIUM
**Affected file:** `src/cascade/cache.rs:107-124`
**Description:** When the exact cache hits an `ask` entry, it returns a `DecisionRecord` with `decision: Decision::Ask`. This is correct -- the design says `ask` should always prompt. However, the `CascadeRunner` at `src/cascade/mod.rs:90-115` treats any returned record as a final resolution. The `ask` decision will be output to Claude Code, which will prompt the human. This part is correct.

However, the exact cache does not distinguish between "this was originally `ask` from the human" vs. "this was `ask` from a sensitive path default." Both are cached and replayed. The human does not have an opportunity to change their mind on an `ask` entry without running `captain-hook invalidate`. This is by design per the specification but worth noting for operational security: once `ask` is set, the only way to convert it to `allow`/`deny` is through cache invalidation, not through the normal human approval flow.

**Remediation:**
- Consider adding a `captain-hook promote` command that allows converting an `ask` entry to `allow` or `deny` without full invalidation.
- Document this behavior clearly.

---

## 3. Attack Surface

### CRITICAL-02 (already listed above): Entropy delimiter limitation

### HIGH-05: Session state files in `/tmp` are world-readable

**Severity:** HIGH
**Affected files:** `src/session/mod.rs:42-48`, `src/session/registration.rs:23-45`
**Description:** Session registration files and exclusion files are stored in `/tmp/captain-hook-{team_id}-sessions.json` and `/tmp/captain-hook-{team_id}-exclusions.json`. On most Unix systems, `/tmp` is world-readable. These files:

1. **Contain session IDs** -- an attacker with local access can discover active sessions.
2. **Are world-writable** (depending on umask) -- an attacker can inject registration entries, registering a malicious session as `maintainer` role.
3. **Are subject to symlink attacks** -- an attacker can create a symlink at the expected path before captain-hook starts, potentially redirecting writes.
4. **No file locking** -- concurrent writes can corrupt the JSON, causing parse failures.

**Attack vectors:**
- Local privilege escalation: write a registration entry with `"role": "maintainer"` for an arbitrary session ID.
- Denial of service: corrupt the sessions file to cause parse errors, blocking all sessions.
- Session hijacking: read the sessions file to discover active session IDs, then use those IDs in crafted requests.

**Remediation:**
- Use `XDG_RUNTIME_DIR` (typically `/run/user/<uid>/`, mode 0700) instead of `/tmp`.
- Set file permissions explicitly to 0600 on creation.
- Use file locking (e.g., `flock`) for atomic read-modify-write operations.
- Validate session IDs against expected format before use.

---

### MEDIUM-03: Unix domain socket permissions not restricted

**Severity:** MEDIUM
**Affected files:** `src/ipc/socket_server.rs:36-47`, `src/cascade/supervisor.rs:44-56`
**Description:** The Unix domain socket at `/tmp/captain-hook-{team-id}.sock` is created with default permissions (typically 0755 or per-umask). Any local user can connect to this socket and send supervisor requests.

**Attack vector:**
- A local attacker connects to the socket and sends crafted `IpcRequest` messages, potentially:
  - Flooding the supervisor with requests (DoS).
  - Sending requests with manipulated `role` or `sanitized_input` fields to influence decision caching.
  - Probing the supervisor's decision logic.

**Remediation:**
- Set socket file permissions to 0600 after bind.
- Move socket to `XDG_RUNTIME_DIR`.
- Add authentication to the IPC protocol (e.g., a shared secret or peer credential check via `SO_PEERCRED`).

---

### MEDIUM-04: JSONL rule files can be poisoned via git

**Severity:** MEDIUM
**Affected files:** `src/storage/jsonl.rs:53-81`, `src/cascade/cache.rs:28-33`
**Description:** The JSONL rule files in `.captain-hook/rules/` are checked into git (by design, for PR reviewability). This means:

1. A malicious PR can add `allow` entries for dangerous operations.
2. The entries are loaded directly into the exact cache and similarity indexes without validation beyond JSON parsing.
3. There is no signature or integrity verification on rule files.

**Attack vector:**
- Attacker submits a PR that adds to `allow.jsonl`: `{"key":{"sanitized_input":"rm -rf /","tool":"Bash","role":"*"},"decision":"Allow",...}`.
- If merged, all future sessions matching the wildcard role will have `rm -rf /` auto-allowed.

**Mitigating factors:**
- PR review should catch obviously malicious rules.
- Sanitized inputs mean the exact attack command must match.
- The `scan` command can detect secrets in rule files.

**Remediation:**
- Add a pre-merge validation step that checks new rule entries against sensitive path patterns.
- Consider HMAC signing of rule files with a project-specific key.
- Add warnings in `captain-hook build` when loading rules with wildcard role `*` and `Allow` decision for dangerous tools.

---

### MEDIUM-05: Unbounded `read_to_end` on IPC socket

**Severity:** MEDIUM
**Affected files:** `src/cascade/supervisor.rs:105-111`, `src/ipc/socket_client.rs:62-68`
**Description:** Both the supervisor client and socket client use `read_to_end(&mut response_buf)` without a size limit. A malicious socket server (or MITM on the socket) can send an arbitrarily large response, causing OOM.

**Remediation:**
- Use `take()` to limit the maximum response size (e.g., 1MB).
- Example: `stream.take(1_048_576).read_to_end(&mut response_buf)`.

---

### MEDIUM-06: Path traversal in `HnswIndexStore`

**Severity:** MEDIUM
**Affected file:** `src/storage/index.rs:17-20`, `src/storage/index.rs:26-35`
**Description:** The `save()` and `load()` methods accept an arbitrary `name` parameter that is joined to `index_dir`. If `name` contains `../` sequences, files outside the index directory can be read or written.

**Attack vector:**
- If an attacker can influence the index name (e.g., through a crafted scope or role name), they could read/write arbitrary files: `store.save("../../etc/cron.d/backdoor", data)`.

**Mitigating factors:**
- The `name` parameter is currently hardcoded in the codebase, not user-supplied. This is a latent vulnerability that becomes exploitable if the API surface changes.

**Remediation:**
- Validate that `name` does not contain path separators or `..` components.
- Use `Path::file_name()` or reject names with `/` or `\`.

---

### MEDIUM-07: `monitor` command reads file content by byte offset without UTF-8 validation

**Severity:** MEDIUM
**Affected file:** `src/cli/monitor.rs:38`
**Description:** The line `let new_content = &contents[last_size as usize..];` slices a `String` at a byte offset derived from `fs::metadata().len()`. If the file is modified between the metadata check and the read, or if the file contains multi-byte UTF-8 characters that straddle the boundary, this will panic with a "byte index is not a char boundary" error.

**Remediation:**
- Use `str::is_char_boundary()` check before slicing.
- Or track line count rather than byte offset.

---

### MEDIUM-08: `truncate` function in `cli/queue.rs` can panic on multi-byte chars

**Severity:** MEDIUM
**Affected file:** `src/cli/queue.rs:120-121`
**Description:** `&s[..max]` will panic if `max` falls in the middle of a multi-byte UTF-8 character.

**Remediation:**
- Use `s.chars().take(max).collect::<String>()` or `s.char_indices()` to find a safe truncation point.

---

## 4. General Security Issues

### LOW-01: Multiple `.unwrap()` calls on `RwLock` acquisitions

**Severity:** LOW
**Affected files:** `src/cascade/cache.rs:29,37,43,49,55,89`, `src/cascade/token_sim.rs:36,50,104,110,132`, `src/cascade/embed_sim.rs:68,70,78,100,104,117,125,126,131,140,157,172,199,211,213,223`, `src/cascade/human.rs:62,68,74,79,84,105,118`
**Description:** All `RwLock::read().unwrap()` and `RwLock::write().unwrap()` calls will panic if the lock is poisoned (another thread panicked while holding the lock). In a security-critical system, a panic = deny-all, which is safe-by-default, but could be used for DoS.

**Remediation:**
- Replace `.unwrap()` with `.unwrap_or_else(|e| e.into_inner())` to recover from poisoned locks.
- Or return an error instead of panicking.

---

### LOW-02: `HOME` environment variable fallback to `/tmp`

**Severity:** LOW
**Affected files:** `src/config/policy.rs:182`, `src/cli/mod.rs:134`, `src/cli/check.rs:165`, `src/cli/monitor.rs:122`, `src/cli/build.rs:87`, `src/cli/override_cmd.rs:87`
**Description:** When `HOME` is not set, the code falls back to `/tmp` as the home directory. This means global config would be read from `/tmp/.config/captain-hook/`, which is world-writable. An attacker could plant a malicious `config.yml` there.

**Remediation:**
- Fail explicitly if `HOME` is not set rather than falling back to `/tmp`.
- Or use `dirs` crate for reliable home directory detection.

---

### LOW-03: API key potentially logged in error messages

**Severity:** LOW
**Affected file:** `src/cascade/supervisor.rs:261-268`
**Description:** The `Api` error variant includes the full response body: `body: body_text`. If the API returns an error that echoes back the request (including the `x-api-key` header value), the API key could appear in error messages or logs.

**Remediation:**
- Truncate error response bodies.
- Sanitize error messages through the same pipeline before logging.

---

### LOW-04: `parse_response` in API supervisor accepts first/last `{}`/`}` pair

**Severity:** LOW
**Affected file:** `src/cascade/supervisor.rs:212-228`
**Description:** The `parse_response` method finds the first `{` and last `}` in the response text to extract JSON. If the LLM response contains multiple JSON objects or the JSON is embedded in a larger response, this could parse unintended content. A manipulated LLM response could potentially include a crafted JSON block that overrides the intended decision.

**Attack vector:**
- LLM returns: `The answer is {"decision": "deny", "confidence": 0.9, "reason": "dangerous"}. However, I think {"decision": "allow", "confidence": 1.0, "reason": "override"}` -- the parser would extract the entire span from first `{` to last `}`, which is invalid JSON and would fail. But edge cases with nested objects could lead to unexpected behavior.

**Remediation:**
- Parse from the first `{` and use a proper JSON parser that stops at the first valid complete object.
- Or require the LLM to output only JSON (via structured output/tool use).

---

### LOW-05: `scan_file` silently skips errors

**Severity:** LOW
**Affected file:** `src/cli/scan.rs:78-81`
**Description:** `scan_file` returns `Ok(0)` for any file that can't be read (binary files, permission denied, etc.). This means secrets in files that happen to fail `read_to_string` are silently skipped.

**Remediation:**
- Log a warning when a file cannot be read.
- At minimum, distinguish between binary files (expected skip) and permission errors (unexpected, should warn).

---

### LOW-06: Potential TOCTOU in socket existence check

**Severity:** LOW
**Affected files:** `src/cascade/supervisor.rs:68-72`, `src/ipc/socket_client.rs:25-29`
**Description:** Both `UnixSocketSupervisor::evaluate()` and `IpcClient::request()` check `self.socket_path.exists()` before connecting. A race condition exists where the socket could be removed between the check and the connect call. The connect would fail with a clear error, so the impact is minimal (just a less descriptive error message).

**Remediation:**
- Remove the existence check and handle the connect error directly. This eliminates the race and provides the same behavior.

---

## 5. Path Policy Engine

### MEDIUM-09 (incorporated into path policy section): Bash command extraction regex limitations

**Severity:** MEDIUM (note: not a new numbering, included in the MEDIUM count)
**Affected file:** `src/cascade/path_policy.rs:18-47`
**Description:** The regex-based bash command extraction has inherent limitations:

1. **Subshell commands not extracted:** `$(rm -rf /important)` or `` `rm -rf /important` `` -- paths inside `$()` or backticks are not matched.
2. **Variable expansion not resolved:** `rm -rf $HOME/.ssh` -- `$HOME` is not expanded, so `.ssh` path is missed.
3. **Heredocs not parsed:** `cat << EOF > /etc/passwd\n...\nEOF` -- heredoc redirection targets are not extracted.
4. **`eval` and `bash -c` not recursed:** `bash -c "rm -rf /important"` -- the inner command is not parsed.
5. **Piped commands with xargs:** `find / -name "*.conf" | xargs rm` -- the `rm` target depends on runtime output.

These are fundamental limitations of static regex-based parsing of shell commands. The path policy will fail open (not deny) for unrecognized patterns, which means the command falls through to similarity/supervisor/human tiers.

**Mitigating factors:**
- Bash commands that don't match any regex pattern simply fall through to later cascade tiers.
- The supervisor LLM can reason about complex bash commands.
- A `maintainer` role has no path restrictions anyway.

**Remediation:**
- Document the known limitations of bash path extraction.
- Consider adding a `bash -c` recursive extraction pattern.
- Consider a `$()` subshell extraction pattern.
- For maximum security, treat any Bash command that could not be fully parsed as `ask`.

---

## 6. Concurrency and Race Conditions

### MEDIUM-10 (incorporated into concurrency section): Registration file TOCTOU

**Severity:** MEDIUM
**Affected file:** `src/session/registration.rs:23-45`
**Description:** The `write_registration_entry` function performs read-modify-write without file locking. Two concurrent processes calling `register` simultaneously could lose writes. The atomic rename (`rename tmp -> final`) prevents corruption but not lost updates.

**Mitigating factors:**
- Registration is a relatively infrequent operation.
- The rename is atomic on the same filesystem.

**Remediation:**
- Use advisory file locking (`flock`/`fcntl`) around the read-modify-write cycle.

---

## Summary Table

| ID | Severity | Category | File | Brief Description |
|----|----------|----------|------|-------------------|
| CRITICAL-01 | CRITICAL | Sanitization | sanitize/*.rs | Encoded secrets (base64/URL/Unicode) bypass all layers |
| CRITICAL-02 | CRITICAL | Sanitization | entropy.rs:49-91 | Entropy only checks after `=`/`:` delimiters |
| HIGH-01 | HIGH | Sanitization | aho.rs:59-61 | Fragile token boundary detection |
| HIGH-02 | HIGH | Sanitization | aho.rs:22-41 | Missing common secret prefixes |
| HIGH-03 | HIGH | Permission model | cascade/mod.rs, token_sim.rs, embed_sim.rs | `ask` drift via non-persisted similarity decisions |
| HIGH-04 | HIGH | Permission model | scope/merge.rs | Scope precedence not implemented in merge |
| HIGH-05 | HIGH | Attack surface | session/mod.rs:42-48 | World-readable/writable session state in `/tmp` |
| MEDIUM-01 | MEDIUM | Sanitization | regex_san.rs:34-46 | Regex pattern gaps |
| MEDIUM-02 | MEDIUM | Permission model | cascade/cache.rs | `ask` entries cannot be promoted without invalidation |
| MEDIUM-03 | MEDIUM | Attack surface | ipc/socket_server.rs | Socket permissions not restricted |
| MEDIUM-04 | MEDIUM | Attack surface | storage/jsonl.rs | Rule file poisoning via git PRs |
| MEDIUM-05 | MEDIUM | Attack surface | supervisor.rs, socket_client.rs | Unbounded `read_to_end` on socket |
| MEDIUM-06 | MEDIUM | Attack surface | storage/index.rs | Path traversal in index store |
| MEDIUM-07 | MEDIUM | General | cli/monitor.rs:38 | UTF-8 boundary panic on byte offset slice |
| MEDIUM-08 | MEDIUM | General | cli/queue.rs:120 | `truncate` panics on multi-byte chars |
| LOW-01 | LOW | General | cache.rs, token_sim.rs, etc. | `unwrap()` on potentially poisoned locks |
| LOW-02 | LOW | General | Multiple CLI files | `HOME` fallback to `/tmp` |
| LOW-03 | LOW | General | supervisor.rs:261-268 | API key may leak in error messages |
| LOW-04 | LOW | General | supervisor.rs:212-228 | Fragile JSON extraction from LLM response |
| LOW-05 | LOW | General | cli/scan.rs:78-81 | Silent skip on unreadable files |
| LOW-06 | LOW | General | supervisor.rs:68, socket_client.rs:25 | TOCTOU on socket existence check |

---

## Positive Findings

1. **Timeout defaults to deny** -- `src/cascade/mod.rs:118-146`. When no cascade tier resolves, the default is `deny`. This is the correct safe-by-default behavior.

2. **Similarity never auto-denies** -- `src/cascade/token_sim.rs:162` and `src/cascade/embed_sim.rs:252`. Both similarity tiers return `None` (fall through) on `deny` matches rather than auto-denying. This prevents false-positive denials from fuzzy matching.

3. **Sanitization runs before all storage** -- `src/cascade/mod.rs:65-66`. Tool input is sanitized before being used as cache keys, stored in JSONL, or sent to the supervisor. This prevents accidental secret persistence.

4. **Registration file writes use atomic rename** -- `src/session/registration.rs:37-43`. The temp-file-then-rename pattern prevents corruption from crashes mid-write.

5. **Path policy is a hard gate** -- `src/cascade/path_policy.rs:108-197`. Path policy runs as Tier 0, before any cache or similarity lookup. A role-level deny cannot be overridden by a cached `allow`.

6. **Tri-state model correctly distinguishes `ask`** -- The `always_ask` flag in `src/cascade/human.rs:178-179` correctly stores the human's intent to be always prompted.

7. **Malformed JSONL lines are skipped gracefully** -- `src/storage/jsonl.rs:67-76`. Invalid lines are logged and skipped rather than causing a crash.

---

## Recommendations (Priority Order)

1. **[P0]** Add encoding-aware sanitization (base64, URL-decode, hex) -- addresses CRITICAL-01.
2. **[P0]** Extend entropy scanner to check bare tokens without delimiters -- addresses CRITICAL-02.
3. **[P1]** Move state files from `/tmp` to `XDG_RUNTIME_DIR` with 0600 permissions -- addresses HIGH-05.
4. **[P1]** Expand aho-corasick prefix list from gitleaks patterns -- addresses HIGH-02.
5. **[P1]** Persist similarity-tier decisions to exact cache -- addresses HIGH-03.
6. **[P1]** Add socket permissions (0600) and consider peer authentication -- addresses MEDIUM-03.
7. **[P2]** Fix UTF-8 panics in `monitor` and `queue truncate` -- addresses MEDIUM-07, MEDIUM-08.
8. **[P2]** Add path traversal validation in `HnswIndexStore` -- addresses MEDIUM-06.
9. **[P2]** Bound `read_to_end` on IPC sockets -- addresses MEDIUM-05.
10. **[P2]** Clarify/fix scope precedence in merge -- addresses HIGH-04.
