# hookwise: Intelligent Permission Gating for Claude Code

## Overview

hookwise is a Rust binary that acts as a Claude Code hook to provide intelligent, learned permission gating across multi-session and multi-agent Claude Code environments. It implements a cascading decision system that starts fast (cached exact matches), falls back to token-level Jaccard similarity, then to embedding-based HNSW similarity (instant-distance + fastembed), then to a pluggable LLM supervisor agent, and finally to a human-in-the-loop — only when genuinely needed.

Decisions are cached and checked into git at the project level, so permission knowledge accumulates over time and is shared across contributors. The system supports scoped rules at the org, project, user, and role levels.

hookwise ships as a Claude Code plugin, bundling the Rust binary, hooks, agent instructions, and slash commands into a single installable package.

## Problem Statement

Claude Code's built-in permission system is per-session and binary: the user approves or denies each tool call. In multi-agent environments (agent teams, swarms), this creates two problems:

1. **Permission fatigue**: Dozens of agents generating hundreds of tool calls, each requiring human approval
2. **No institutional memory**: Permissions granted in one session don't carry to the next. Every new session starts cold.

hookwise solves both by building a learned permission policy that gets smarter over time, only involving a human for genuinely novel or ambiguous decisions.

## Architecture

```
                            +-----------------------------+
                            |   Main Claude Code Session   |
                            |                             |
                            |  +-----------------------+  |
                            |  |  Supervisor Agent      |  |
                            |  |  (LLM + policy eval)   |  |
                            |  +----------+------------+  |
                            |             | low confidence |
                            |             v               |
                            |  +-----------------------+  |
                            |  |  User (HITL)           |  |
                            |  +-----------------------+  |
                            +----------+------------------+
                                       | Unix domain socket
        +------------------------------+------------------------------+
        |                              |                              |
   Worker Session A              Worker Session B              Worker Session C
   (coder agent)                 (tester agent)                (docs agent)
        |                              |                              |
   PreToolUse hook               PreToolUse hook               PreToolUse hook
        |                              |                              |
        v                              v                              v
   +----------+                  +----------+                  +----------+
   |captain-   |                 |captain-   |                 |captain-   |
   |hook binary|                 |hook binary|                 |hook binary|
   +----------+                  +----------+                  +----------+
```

## Tri-State Decision Model

Every decision resolves to one of three states:

| State | Meaning | Cache behavior | Hook exit |
|-------|---------|---------------|-----------|
| **allow** | Permit the tool call | Auto-resolves on future matches | Exit 0 |
| **deny** | Block the tool call | Auto-resolves on future matches | Exit 1 |
| **ask** | Always prompt a human | Escalates to human on every match | Blocks until human responds |

**Allow** and **deny** are the converging states — once cached, they resolve instantly without human involvement. The system trends toward these over time.

**Ask** is the non-converging state. It means "I've seen this before, I know exactly what it is, and I still want a human to decide every time." The cache remembers the decision *type* is `ask`, and on every match it escalates to the human rather than auto-resolving.

Use cases for `ask`:
- Writing to `.claude/` configuration in the project or user home directory
- Modifying settings files that affect tooling behavior
- Commands that are safe in some contexts but dangerous in others (e.g., `git push` to different remotes)
- Any operation the user wants to stay aware of regardless of frequency

When a human is prompted (at tier 4, or because of an `ask` cache hit), they choose from:

```
[a]llow   [d]eny   [?]always ask   [a+r]llow & add rule   [d+r]eny & add rule   [?+r]ask & add rule
```

- `allow` / `deny` — one-time decision for this instance, cached as allow/deny for future matches
- `always ask` — one-time decision (allow or deny for *this* call is also required), but the cached state is `ask`, so the next match will prompt again
- `+r` variants — codify the decision as a persistent rule at the chosen scope

When a human responds to an `ask` prompt, they must also choose allow or deny for the current invocation:

```
Session "coder-1" (role: coder) wants to write:
  .claude/settings.json

This file is marked "always ask".
[a]llow this time   [d]eny this time
```

The current call proceeds with their choice, but the cached state remains `ask`.

## Decision Cascade

Every tool call passes through a 6-tier cascade. Each tier either resolves the decision or escalates:

```
Tool call -> Sanitize -> Path Policy -> Cache (exact) -> Token Jaccard -> Embedding HNSW -> LLM -> Human
               ~5us        ~1us          ~100ns            ~500ns           ~1-5ms        ~1-2s  interrupt
```

| Tier | Mechanism | Latency | When it resolves |
|------|-----------|---------|------------------|
| 0 | Path policy (deterministic) | ~1us | File path matches a role's allow/deny glob |
| 1 | Exact cache match | ~100ns | Command seen before, decision is allow or deny |
| 1* | Exact cache match (ask) | immediate escalation | Command seen before, but decision is `ask` — skip to tier 5 |
| 2a | Token-level Jaccard similarity | ~500ns | Token overlap with a cached entry exceeds threshold (default 0.7) |
| 2b | Embedding HNSW similarity | ~1-5ms | Semantically similar command was previously decided |
| 3 | LLM supervisor agent | ~1-2s | Novel command, but policy is clear enough for LLM |
| 4 | Human-in-the-loop | variable | LLM confidence too low, `ask` state, or needs human judgment |

Every decision at tiers 3 and 4 feeds back into tiers 1, 2a, and 2b. The system converges toward full autonomy over time — except for `ask` entries, which intentionally do not converge.

**Important**: When the cache returns an `ask` decision, it skips tiers 2a, 2b, and 3 entirely and goes straight to human. The LLM supervisor cannot override an `ask` — only the human can downgrade it to allow or deny.

**Similarity and ask**: If a similar command (via Jaccard or embedding) matches a cached `ask` entry with high confidence, it should also escalate to the human. The `ask` intent propagates through similarity — if `Write .claude/settings.json` is `ask`, then `Edit .claude/settings.json` should also `ask`, not silently resolve.

## Session Registration

### Registration Mechanism

Every session must be registered with a role before hookwise permits tool calls. Registration maps a session_id to a role, task description, and optional agent prompt.

**Registration file**: `/tmp/hookwise-<team-id>-sessions.json` (or `/tmp/hookwise-solo-sessions.json` for non-team use):

```json
{
  "abc-123": {
    "role": "coder",
    "task": "Implement auth handler for the API",
    "prompt_hash": "sha256:...",
    "prompt_path": "/tmp/.hookwise-prompt-abc-123",
    "registered_at": "2026-02-08T...",
    "registered_by": "team-lead"
  },
  "def-456": {
    "role": "tester",
    "task": "Run pytest suite for auth module",
    "prompt_hash": "sha256:...",
    "prompt_path": "/tmp/.hookwise-prompt-def-456",
    "registered_at": "2026-02-08T...",
    "registered_by": "team-lead"
  }
}
```

An exclusion list tracks disabled sessions:

```json
{
  "disabled": ["ghi-789"]
}
```

### Registration Flow

**In agent teams** — the team lead registers each worker after spawning:

```bash
hookwise register \
  --session-id "$SESSION_ID" \
  --role coder \
  --task "Implement auth handler" \
  --prompt-file /tmp/.hookwise-prompt-$SESSION_ID
```

**In interactive sessions** — hookwise prompts the user to pick a role on first use. This is triggered by the `user_prompt_submit` hook (see Interactive Registration below).

### Unregistered Session Behavior

When a `PreToolUse` hook fires for an unregistered session:

1. **Brief wait** — poll the registration file every 200ms for a configurable window (default: 5s). This covers the normal race where a team lead registers an agent moments after spawning it.

2. **If registration arrives during wait** — proceed normally through the cascade.

3. **If timeout** — return a block with instructions:

```json
{
  "decision": "block",
  "reason": "hookwise: Session not registered.\n\nThis session needs a role before tool calls are permitted.\n\nTo register: /hookwise register\nTo disable:  /hookwise disable\n\nOr from a terminal:\n  hookwise register --session-id abc-123 --role <role>\n  hookwise disable --session-id abc-123"
}
```

The tool call is blocked but the session continues. The next tool call retries the same check.

### Interactive Registration

For interactive (non-team) sessions, hookwise uses the `user_prompt_submit` hook to prompt the user before any work begins.

**Hook configuration:**

```json
{
  "hooks": {
    "user_prompt_submit": [
      {
        "matcher": ".*",
        "command": "hookwise session-check"
      }
    ],
    "PreToolUse": [
      {
        "matcher": ".*",
        "command": "hookwise check"
      }
    ]
  }
}
```

`hookwise session-check` runs when the user submits a prompt:
- If registered or disabled -> exit silently, no interference
- If unregistered -> output a message that Claude sees as system context:

```
hookwise: This session has no role assigned.

Before proceeding, ask the user to choose a role using AskUserQuestion.
Available roles grouped by type:

Implementation: coder, tester, integrator, devops
Knowledge:      researcher, architect, planner, reviewer, security-reviewer, docs
Full-access:    maintainer, troubleshooter

They may also choose to disable hookwise for this session.

After the user chooses, run: hookwise register --session-id <id> --role <chosen-role>
Or: hookwise disable --session-id <id>

Then proceed with their original request.
```

The plugin's agent instructions (injected into the system prompt) tell Claude how to handle this. Claude presents the role choice via `AskUserQuestion`, registers the session, and then continues with whatever the user originally asked.

The user sees something like:

```
> Fix the auth bug in the login handler

  hookwise: What role should this session use?

  Implementation roles:
  [coder]             - modify src/, lib/ (Recommended)
  [tester]            - modify tests/ only
  [integrator]        - terraform, IaC files
  [devops]            - CI/CD, config files

  Knowledge roles:
  [researcher]        - write to docs/research/
  [architect]         - write to docs/architecture/, docs/adr/
  [planner]           - write to docs/plans/
  [reviewer]          - write to docs/reviews/
  [security-reviewer] - write to docs/reviews/security/, run scanners
  [docs]              - write to docs/, *.md

  Full-access roles:
  [maintainer]        - full repository access
  [troubleshooter]    - full access for debugging

  [disable]           - turn off hookwise
```

One-time per session. Takes a few seconds. After that, tool calls flow through the cascade normally.

### Role Resolution Order

When the hook fires and needs to find the session's role:

```rust
fn resolve_role(session_id: &str) -> Option<RoleDefinition> {
    // 1. In-memory cache (nanoseconds)
    if let Some(ctx) = SESSIONS.get(session_id) {
        return ctx.role.clone();
    }
    // 2. Registration file (microseconds)
    if let Some(role) = read_registration_file(session_id) {
        return Some(role);
    }
    // 3. Env var fallback (nanoseconds, for scripted/CI use)
    if let Ok(role_name) = std::env::var("HOOKWISE_ROLE") {
        return load_role_from_config(&role_name);
    }
    // 4. Not registered
    None
}
```

## Components

### 1. The Rust Binary (`hookwise`)

The core binary serves multiple modes:

**Hook mode** — called by Claude Code on every `PreToolUse` event. Reads the hook payload from stdin as JSON:
```bash
echo '{"session_id":"abc-123","tool_name":"Bash","tool_input":{"command":"pytest --cov"},...}' | hookwise check
# Outputs JSON to stdout with hookSpecificOutput.permissionDecision = "allow"|"deny"|"ask"
```

**Session check mode** — called on `user_prompt_submit` to trigger registration:
```bash
hookwise session-check
# Outputs registration prompt if session is unregistered
```

**Queue mode** — human interface for pending decisions:
```bash
hookwise queue                        # List pending decisions
hookwise approve <id>                 # Approve (cached as allow)
hookwise deny <id>                    # Deny (cached as deny)
hookwise approve <id> --always-ask    # Allow this time, cache as ask
hookwise deny <id> --always-ask       # Deny this time, cache as ask
hookwise approve <id> --add-rule      # Approve and codify as rule
hookwise approve <id> --scope org     # Set rule scope
```

**Monitor mode** — observe decisions in real time:
```bash
hookwise monitor                      # Stream decisions live
hookwise stats                        # Cache hit rates, decision distribution
```

### 2. Secret Sanitization

All tool input is sanitized **before** any cache or vector lookup. Nothing unsanitized ever touches storage.

Three detection layers, all compiled once at binary startup:

**Layer 1: aho-corasick** — literal prefix matching (~100ns)
```
sk-ant-, sk-, ghp_, gho_, ghs_, github_pat_,
AKIA, ASIA, xoxb-, xoxp-, glpat-, glsa-,
npm_, pypi-, AGE-SECRET-KEY-, -----BEGIN
```

**Layer 2: RegexSet** — positional/contextual patterns (~1-5us)
```
bearer\s+[a-z0-9_\-\.]{20,}
(api[_-]?key|token|secret|password)\s*[=:]\s*\S{8,}
postgres://\S+:\S+@
(--password|--token|-p)\s+\S{8,}
```

**Layer 3: Shannon entropy** — catch unknown secret formats (~2-5us)
```
Flag any 20+ char token after '=' or ':' with entropy > 4.0
```

Pattern source: compile gitleaks' ~150 regex patterns from their public config into a Rust RegexSet for detection quality at native speed.

**Sanitization output** replaces detected secrets with `<REDACTED>`:
```
Input:  curl -H "Authorization: Bearer sk-ant-abc123xyz" https://api.example.com
Output: curl -H "Authorization: Bearer <REDACTED>" https://api.example.com
```

For Write/Edit tools, cache at the structural level (file path + change type), never the content:
```jsonl
{"tool":"Edit","file":"src/auth/handler.ts","decision":"allow"}
{"tool":"Write","file":".env","decision":"deny","reason":"writing to .env blocked by policy"}
{"tool":"Write","file":".claude/settings.json","decision":"ask","reason":"user wants to review all .claude/ modifications"}
```

### 3. Path Policy (Tier 0 — globset)

A deterministic, pre-cascade check that enforces file path boundaries per role. This runs before the cache, vector search, or LLM — it is a hard gate that cannot be overridden by cached decisions or LLM judgment.

Path policies are defined in `roles.yml` as glob patterns, compiled into `GlobSet` instances at startup for batch matching:

```yaml
roles:
  coder:
    paths:
      allow_write: ["src/**", "lib/**", "Cargo.toml", "Cargo.lock", "package.json"]
      deny_write: ["tests/**", "docs/**", ".github/**", "*.tf", "*.tfvars"]
      allow_read: ["**"]
```

**Evaluation logic:**
1. Extract the file path from the tool input (Write/Edit: `file_path` field; Read: `file_path` field)
2. Check `deny_write` globs first — if matched, immediate deny (deny wins)
3. Check `allow_write` globs — if matched and no deny match, immediate allow
4. If neither matches, fall through to the cascade (tiers 1-4 decide)

**Bash tool path extraction:**
For Write/Edit/Read tools, path extraction is trivial — the `file_path` field is explicit. For Bash commands, two layers:

1. **Deterministic regex** — extract paths from common write patterns: `rm`, `mv`, `cp`, `mkdir`, `touch`, redirects (`>`, `>>`, `tee`), `sed -i`, `chmod`, `chown`, `git checkout --`
2. **LLM fallback** — if the regex can't extract paths and the command contains write-like tokens, fall through to the LLM supervisor with the path policy as context

**Conservative default**: if a Bash command looks like it might write files but paths can't be extracted deterministically, it falls through to the LLM rather than auto-allowing.

### 4. Cache System

The cache stores exact decisions as JSONL, checked into git at the project level:

```jsonl
{"command":"pytest --cov","tool":"Bash","role":"tester","decision":"allow","reason":"test execution within role scope","timestamp":"2026-02-08T...","scope":"project"}
{"command":"npm publish","tool":"Bash","role":"coder","decision":"deny","reason":"publishing restricted to release role","timestamp":"2026-02-08T...","scope":"project"}
{"tool":"Write","file":".claude/settings.json","role":"*","decision":"ask","reason":"user wants to review all .claude/ modifications","timestamp":"2026-02-08T...","scope":"project"}
```

Cache lookup is a hash map keyed on (sanitized_command, tool, role). O(1) lookup, ~50-100ns.

**Tri-state cache behavior:**
- `allow` hit -> return allow immediately (tier resolved)
- `deny` hit -> return deny immediately (tier resolved)
- `ask` hit -> **skip tiers 2-3**, escalate directly to human (tier 4)

Sanitization improves cache generalization: `curl` with different bearer tokens normalizes to the same sanitized command, producing a single cache entry that covers all variants.

### 5. Token-Level Jaccard Similarity (Tier 2a)

When the exact cache misses, a lightweight token-level Jaccard similarity check runs before the heavier embedding search. This catches ~60% of near-matches at ~500ns, avoiding the 1-5ms embedding path.

**Token extraction:**
1. Split input on whitespace and punctuation
2. Lowercase all tokens
3. Deduplicate and sort

**Jaccard coefficient:** |A ∩ B| / |A ∪ B| where A and B are the token sets of the query and cached entry.

**Threshold:** 0.7 default, configurable in `policy.yml`.

**Short command handling:** Commands with fewer than 3 tokens skip Tier 2a and go directly to Tier 2b (embedding similarity). Single-word commands like `ls` have too little token signal for meaningful Jaccard comparison.

**Examples of what Jaccard catches:**
- `pytest --cov tests/auth/` vs `pytest --cov tests/api/` — same tokens except the path suffix → Jaccard ~0.85
- `cargo build --release` vs `cargo build --release --target x86_64` — high overlap → Jaccard ~0.75
- `rm -rf /tmp/build-output` vs `rm -rf /tmp/build-cache` — same structure → Jaccard ~0.80

**Examples that need embeddings (Tier 2b):**
- `npm test` vs `yarn test` — different tokens, same semantics
- `docker compose up` vs `docker-compose up` — tokenization difference
- `cat README.md` vs `less README.md` — different command, similar intent

**Jaccard behavior by decision type:**
- Jaccard match to `allow` entry → auto-approve
- Jaccard match to `deny` entry → fall through to Tier 2b (never auto-deny on token match alone)
- Jaccard match to `ask` entry → escalate to human (the `ask` intent propagates)

### 6. Embedding Similarity (Tier 2b — instant-distance + fastembed)

When both the exact cache and token Jaccard miss, instant-distance performs approximate nearest neighbor search using HNSW indexing over embeddings of previously-decided commands. Embeddings are generated locally using fastembed (ONNX Runtime, no external API calls).

This catches semantic similarity: if `rm -rf /tmp/build-output` was approved, then `rm -rf /tmp/build-cache` matches without hitting the LLM — even if the token overlap is below the Jaccard threshold.

The vector index is built from the cached decisions and is **not** checked into git — it's a derived artifact rebuilt locally:

```bash
hookwise build   # Rebuild .index/ from rules/
```

Or rebuilt lazily on first hook call if the index is missing.

**Embedding similarity behavior by decision type:**
- Similar to `allow` entry → auto-approve
- Similar to `deny` entry → fall through to LLM (never auto-deny on weak match)
- Similar to `ask` entry → escalate to human (the `ask` intent propagates through similarity)

If a command doesn't match the vector space with high confidence, it falls through to the LLM regardless of the matched decision type.

### 7. LLM Supervisor Agent (Tier 3 — Pluggable Backend)

The supervisor evaluates novel commands against the project's permission policy, role definitions, and path policies. It is accessed through the `SupervisorBackend` trait, which has two implementations:

**`UnixSocketSupervisor`** — for Claude Code agent teams:
- The supervisor is a **subagent** within the main Claude Code session, spawned at team initialization
- Receives permission requests via a Unix domain socket
- Has access to the requesting agent's prompt and task description for context
- Preserves the main session's context — it knows what tasks were delegated and why
- Routine decisions are handled silently; only uncertain cases surface to the user
- Socket location: `/tmp/hookwise-<team-id>.sock`

**`ApiSupervisor`** — for standalone use or CI:
- Calls the Anthropic API directly with the permission context
- Uses a dedicated system prompt with the project's policy, role definitions, and cached decisions
- Configurable model (default: claude-sonnet-4-5-20250929)
- API key from `ANTHROPIC_API_KEY` env var or `~/.config/hookwise/config.yml`
- Useful for CI pipelines, standalone development, or environments without a running Claude Code session

Both implementations:
- Return a decision with a confidence score
- Can recommend `ask` for operations they consider sensitive but context-dependent
- If confidence is below threshold, escalate to the human (tier 4)

**LLM supervisor can recommend ask**: When the supervisor encounters a command that is safe in some contexts but risky in others, it can return `{"decision": "ask", "confidence": 1.0}`. This is cached, and all future matches go to the human. Examples: writing to config files, modifying CI pipelines, running commands with side effects on shared infrastructure.

**SupervisorBackend trait:**
```rust
#[async_trait]
trait SupervisorBackend: Send + Sync {
    async fn evaluate(
        &self,
        request: &SupervisorRequest,
        policy: &PolicyConfig,
    ) -> Result<DecisionRecord, HookwiseError>;
}
```

**Unix domain socket** (not a named pipe) for IPC with `UnixSocketSupervisor`:
- Supports concurrent connections from multiple worker sessions
- Bidirectional — request/response over a single connection
- The requesting session's hook blocks on the socket while waiting for a response
- Socket location: `/tmp/hookwise-<team-id>.sock`

### 8. Human-in-the-Loop (Tier 4)

When the LLM supervisor's confidence is below threshold — or when the cache returns an `ask` state — the decision is written to a **pending queue**. The requesting session blocks until an answer arrives.

The pending decision can be answered from:

- **The main Claude Code session** — the supervisor agent surfaces it as a message
- **A terminal** — `hookwise queue` shows pending items, `hookwise approve/deny` answers them
- **Any future interface** — web UI, mobile notification, VS Code panel — anything that can read/write the queue

The queue is the universal interface. The decision source doesn't matter; the answer routes back through the socket to the blocked binary.

**Presentation to the human (novel command):**
```
Session "backend-coder" (role: tester) wants to run:
  docker compose -f docker-compose.prod.yml up

Policy says: "testers may use Docker if docker-compose.test.yml exists"
This targets docker-compose.prod.yml (not test).
Recommendation: DENY (confidence: 0.35)

[a]llow  [d]eny  [?]always ask  [a+r]llow & add rule  [d+r]eny & add rule  [?+r]ask & add rule
```

**Presentation to the human (ask state):**
```
Session "coder-1" (role: coder) wants to write:
  .claude/settings.json

This action is marked "always ask" (set 2026-02-06).
Reason: user wants to review all .claude/ modifications

[a]llow this time  [d]eny this time  [downgrade] change to allow/deny permanently
```

The `downgrade` option lets the human convert an `ask` entry to a permanent `allow` or `deny` when they decide they no longer need to be prompted.

The `+r` variants codify the decision as a persistent rule, feeding back into the cache and optionally into the policy document itself.

**Timeout behavior:** If no human response within a configurable duration (default: 60s), default to deny. The requesting session receives a clear error: "Permission request timed out — no human response."

### 9. Confidence Threshold

The confidence threshold determines how often the human is interrupted. Tuning:

- **Start conservative** (high threshold, e.g., 0.8) — more human involvement while trust builds
- **Lower over time** as the override rate decreases
- **Adaptive option**: track how often the human overrides the LLM's recommendation. High override rate -> raise threshold. Low override rate -> lower it.

The threshold is configurable per scope:
```yaml
confidence:
  org: 0.9       # High bar for org-wide auto-decisions
  project: 0.7   # Moderate for project context
  user: 0.6      # Lower for personal preferences
```

## Role Definitions

### Built-in Roles

Roles combine a natural language description (interpreted by the LLM for behavioral decisions) with deterministic path policies (enforced at tier 0 for file access).

Roles fall into three categories:
- **Implementation roles** (coder, tester, devops, integrator) — write to specific directories
- **Knowledge roles** (researcher, architect, planner, reviewer, security-reviewer, docs) — read the codebase, write artifacts to `docs/` subdirectories
- **Full-access roles** (maintainer, troubleshooter) — unrestricted, for leads and debugging

Knowledge roles produce file artifacts that implementation roles consume. This creates a natural coordination bus:
```
researcher  -> docs/research/       -> read by architect, planner
architect   -> docs/architecture/   -> read by planner, coder
              docs/adr/             -> read by all roles
planner     -> docs/plans/          -> read by coder, tester, devops
reviewer    -> docs/reviews/        -> read by coder, maintainer
sec-review  -> docs/reviews/security/ -> read by coder, maintainer, devops
```

```yaml
# .hookwise/roles.yml
roles:

  # ── Implementation Roles ────────────────────────────────────────────

  coder:
    description: |
      Autonomous implementation specialist. Reads and modifies application
      source code. Can run build tools and dev servers. Should not push
      to git, publish packages, or modify CI config. Should not modify
      test files — that's the tester's job.
    paths:
      allow_write:
        - "src/**"
        - "lib/**"
        - "Cargo.toml"
        - "Cargo.lock"
        - "package.json"
        - "package-lock.json"
        - "go.mod"
        - "go.sum"
        - "pyproject.toml"
        - "requirements*.txt"
      deny_write:
        - "tests/**"
        - "test-fixtures/**"
        - "*.test.*"
        - "*.spec.*"
        - "*_test.go"
        - "docs/**"
        - ".github/**"
        - ".gitlab-ci.yml"
        - "*.tf"
        - "*.tfvars"
      allow_read: ["**"]

  tester:
    description: |
      Autonomous test engineer. Writes and runs tests, coverage tools,
      and linters. Can read any source file for context. Should not
      modify source code, push to git, or install packages. May use
      Docker if docker-compose.test.yml exists.
    paths:
      allow_write:
        - "tests/**"
        - "test-fixtures/**"
        - "*.test.*"
        - "*.spec.*"
        - "*_test.go"
        - "test_*.py"
        - "**/test_*.py"
        - "**/*_test.go"
        - "jest.config.*"
        - "pytest.ini"
        - "vitest.config.*"
        - ".coveragerc"
        - "codecov.yml"
      deny_write:
        - "src/**"
        - "lib/**"
        - "docs/**"
        - ".github/**"
        - "*.tf"
      allow_read: ["**"]

  integrator:
    description: |
      Infrastructure-as-code specialist. Manages terraform, pulumi,
      CDK, ansible, and helm files. Thinks in terms of resources,
      dependencies, state, and blast radius. Should not modify
      application source or tests. NEVER runs terraform apply
      without explicit human approval.
    paths:
      allow_write:
        - "*.tf"
        - "*.tfvars"
        - "*.hcl"
        - "terraform/**"
        - "infra/**"
        - "pulumi/**"
        - "cdk/**"
        - "cloudformation/**"
        - "ansible/**"
        - "helm/**"
        - ".terraform.lock.hcl"
      deny_write:
        - "src/**"
        - "lib/**"
        - "tests/**"
        - "docs/**"
        - ".github/**"
      allow_read: ["**"]

  devops:
    description: |
      CI/CD and deployment specialist. Manages pipelines, Dockerfiles,
      tooling configuration, and developer environment. Should not
      modify application source, tests, or documentation. NEVER
      triggers a production deployment without explicit human approval.
    paths:
      allow_write:
        - ".github/**"
        - ".gitlab-ci.yml"
        - ".circleci/**"
        - "Jenkinsfile"
        - ".buildkite/**"
        - "Dockerfile*"
        - "docker-compose*"
        - ".dockerignore"
        - "Makefile"
        - ".eslintrc*"
        - ".prettierrc*"
        - ".editorconfig"
        - "tsconfig*"
        - ".*rc"
        - ".*rc.*"
        - ".tool-versions"
        - ".nvmrc"
        - ".python-version"
        - ".ruby-version"
        - "rust-toolchain.toml"
        - "lefthook.yml"
        - ".husky/**"
        - ".pre-commit-config.yaml"
      deny_write:
        - "src/**"
        - "lib/**"
        - "tests/**"
        - "docs/**"
        - "*.tf"
      allow_read: ["**"]

  # ── Knowledge Roles ─────────────────────────────────────────────────
  # These roles read the codebase and produce artifacts in docs/ subdirectories.
  # They have narrow write access to their specific output directory only.

  researcher:
    description: |
      Technical researcher and analyst. Gathers information, analyzes
      codebases, investigates technologies, and produces structured
      findings. Never writes code — writes research reports to
      docs/research/. Evidence-based: every claim cites a source
      (file:line for code, URL for external).
    paths:
      allow_write:
        - "docs/research/**"
      deny_write:
        - "src/**"
        - "lib/**"
        - "tests/**"
        - ".github/**"
        - "*.tf"
      allow_read: ["**"]

  architect:
    description: |
      Senior software architect. Designs systems, evaluates tradeoffs,
      and makes structural decisions. Writes design documents and ADRs
      to docs/architecture/ and docs/adr/. Never writes implementation
      code. This separation from coder prevents design bias toward
      solutions that are easy to express in code.
    paths:
      allow_write:
        - "docs/architecture/**"
        - "docs/adr/**"
      deny_write:
        - "src/**"
        - "lib/**"
        - "tests/**"
        - ".github/**"
        - "*.tf"
      allow_read: ["**"]

  planner:
    description: |
      Software architect and technical planner. Analyzes requirements,
      explores the codebase, and produces detailed implementation plans
      that other agents can execute without ambiguity. Never writes
      code — writes plan files to docs/plans/. Plans must be executable
      by a stranger with zero context.
    paths:
      allow_write:
        - "docs/plans/**"
      deny_write:
        - "src/**"
        - "lib/**"
        - "tests/**"
        - ".github/**"
        - "*.tf"
      allow_read: ["**"]

  reviewer:
    description: |
      Senior code reviewer. Evaluates code changes for correctness,
      maintainability, security, and adherence to project standards.
      Does not write source code — produces review artifacts in
      docs/reviews/. Can run linters and static analysis tools.
      Only reports issues with confidence >= 75/100 to prevent noise.
    paths:
      allow_write:
        - "docs/reviews/**"
      deny_write:
        - "src/**"
        - "lib/**"
        - "tests/**"
        - ".github/**"
        - "*.tf"
        - "docs/reviews/security/**"
      allow_read: ["**"]

  security-reviewer:
    description: |
      Security-focused code reviewer. Identifies vulnerabilities,
      insecure patterns, and compliance gaps. Reviews code through the
      lens of an attacker. Can run security scanning tools via Bash
      (cargo audit, npm audit, pip-audit, tfsec, checkov). Writes
      findings to docs/reviews/security/. Classifies findings as
      CRITICAL/HIGH/MEDIUM/LOW with attack vectors and remediations.
    paths:
      allow_write:
        - "docs/reviews/security/**"
      deny_write:
        - "src/**"
        - "lib/**"
        - "tests/**"
        - ".github/**"
        - "*.tf"
        - "docs/reviews/*.md"
      allow_read: ["**"]

  docs:
    description: |
      Technical documentation specialist. Creates and maintains
      documentation that is accurate, concise, and useful. Can read
      any file for context. Should follow existing documentation
      standards and formatting conventions. Verifies code examples
      against actual source. Accuracy above all — wrong documentation
      is worse than no documentation.
    paths:
      allow_write:
        - "docs/**"
        - "*.md"
        - "*.aisp"
        - "CHANGELOG.md"
        - "LICENSE"
      deny_write:
        - "src/**"
        - "lib/**"
        - "tests/**"
        - ".github/**"
        - "*.tf"
      allow_read: ["**"]

  # ── Full-Access Roles ───────────────────────────────────────────────
  # These roles have unrestricted file access. Use for project leads,
  # senior contributors, and debugging/incident response.

  maintainer:
    description: |
      Project maintainer — final authority on code quality, release
      readiness, and repository health. Full access to everything.
      Can create/delete branches and tags, merge PRs, modify CI/CD,
      update dependencies, create releases. Measure twice, cut once —
      verify state before every irreversible action.
    paths:
      allow_write: ["**"]
      deny_write: []
      allow_read: ["**"]

  troubleshooter:
    description: |
      Autonomous debugging specialist. Full repository access for
      diagnosing and fixing bugs, performance issues, and system
      failures. Follows the scientific method: reproduce, isolate,
      inspect, hypothesize, test, fix, verify, document. Should
      document findings and minimize unnecessary changes.
    paths:
      allow_write: ["**"]
      deny_write: []
      allow_read: ["**"]
```

### Custom Roles

Projects can define additional roles or override built-in ones in their `.hookwise/roles.yml`. Custom roles follow the same structure:

```yaml
roles:
  data-engineer:
    description: |
      Manages data pipelines, ETL scripts, and database migrations.
    paths:
      allow_write: ["pipelines/**", "migrations/**", "sql/**", "dbt/**"]
      deny_write: ["src/**", "tests/**", "docs/**"]
      allow_read: ["**"]
```

### Sensitive Path Defaults

Certain paths default to `ask` regardless of role, unless explicitly overridden in the project's policy. These are paths where modifications affect tooling behavior or security:

```yaml
# Built-in ask-by-default paths (configurable in policy.yml)
sensitive_paths:
  ask_write:
    - ".claude/**"
    - ".hookwise/**"
    - ".env*"
    - "**/.env*"
    - ".git/hooks/**"
    - "**/secrets/**"
    - "~/.claude/**"
    - "~/.config/**"
```

Even a `maintainer` with `allow_write: ["**"]` will be prompted when writing to `.claude/settings.json` because the sensitive path check runs before the role path policy. The human can downgrade individual entries from `ask` to `allow` if they find the prompts unnecessary.

### Role Registration

When the team lead spawns agents, roles are registered:

```bash
hookwise register \
  --session-id "$SESSION_ID" \
  --role tester \
  --task "Run pytest suite for auth module" \
  --prompt-file /tmp/.hookwise-prompt-$SESSION_ID
```

The prompt file is stored locally (sanitized) and available to the LLM supervisor when evaluating novel commands. It answers "what was this agent instructed to do?" which helps the supervisor judge whether a tool call is within the agent's intended scope.

### Cache Key Structure

Cache keys include the role, so two agents with different roles have separate decision caches for the same command:

```
Key: (sanitized_command, tool, role)
```

This means `pytest --cov` can be `allow` for a tester and `deny` for a docs agent without conflict.

### Cache Invalidation on Role Change

```bash
hookwise invalidate --role tester
```

Clears all cached decisions for the tester role. Next time each command is encountered, it's re-evaluated against the updated role definition. The system "recompiles" the role organically through usage.

### Explicit Overrides

Human-set overrides take priority over cached LLM interpretations:

```bash
hookwise override --role tester --command "docker compose up" --allow
hookwise override --role coder --tool Write --file ".claude/*" --ask
```

Decision priority:
1. Sensitive path defaults (`ask` for `.claude/**`, etc. — unless overridden)
2. Explicit overrides (deterministic, human-set)
3. Path policy (deterministic globs from roles.yml)
4. Cached decisions (allow/deny/ask from previous evaluations)
5. Live LLM interpretation (only for never-before-seen commands)

## Scope Hierarchy

### Four Levels

```
Org:     ============================  Everything possible
Project: ====    ============    ====  Narrower
User:    ====      ========      ====  Narrowest (preferences)
Role:    ====        ======        ==  Task-scoped least privilege
```

| Scope | What it governs | Where it lives |
|-------|----------------|----------------|
| Org | Security floor for all repos in the org | `~/.config/hookwise/org/<org>/` synced from central source |
| Project | Project-specific tool permissions | `<repo>/.hookwise/rules/` checked into git |
| User | Personal preferences across projects | `~/.config/hookwise/user/` local only |
| Role | Task-scoped permissions for agent team members | Set by team lead at agent spawn time |

### Precedence

**Deny at any level is authoritative. Ask at any level is authoritative (unless a higher scope denies).** Allow only applies if no higher-priority scope denies or asks:

| Org | Project | User | Result |
|-----|---------|------|--------|
| DENY | ALLOW | ALLOW | **DENY** (org is security floor) |
| ALLOW | DENY | ALLOW | **DENY** (project can restrict further) |
| ALLOW | ALLOW | DENY | **DENY** (user can restrict for themselves) |
| ALLOW | ALLOW | ALLOW | **ALLOW** |
| ALLOW | ASK | ALLOW | **ASK** (project requires human review) |
| ASK | ALLOW | ALLOW | **ASK** (org requires human review) |
| ALLOW | ALLOW | ASK | **ASK** (user wants to review personally) |
| DENY | ASK | ALLOW | **DENY** (deny wins over ask) |
| silent | silent | ALLOW | **ALLOW** (user fills in gaps) |

**Precedence order**: DENY > ASK > ALLOW > silent

### Three Logical Indexes

Each scope has its own instant-distance HNSW index. The binary loads all three (or four with role) and cascades:

```
check_role(session, command)     -> match? -> apply
  | no match
check_user(user, command)        -> match? -> apply
  | no match
check_project(repo, command)     -> match? -> apply
  | no match
check_org(org, command)          -> match? -> apply
  | no match
escalate to LLM supervisor
```

Separate indexes avoid cross-contamination in similarity search and make precedence explicit.

## Session Identity

The hook receives JSON on stdin from Claude Code:

```json
{
  "session_id": "abc-123",
  "cwd": "/Users/liam/repos/epiphytic/hookwise",
  "tool_name": "Bash",
  "tool_input": {"command": "pytest --cov"},
  "permission_mode": "default"
}
```

### Deriving Context

| Field | Source |
|-------|--------|
| Project + Org | `cwd` -> `git remote get-url origin` -> parse `github.com/<org>/<repo>` |
| User | OS user (`whoami`) or `git config user.email` |
| Role | Registration file -> match by session_id (see Session Registration) |
| Team | `~/.claude/teams/**/config.json` -> match by session_id/agent_id |
| Agent prompt | Registration file -> prompt_path -> read on-demand by LLM supervisor |

### Session Context Cache

The git/team/registration lookup runs once per new session_id, then is cached in-memory:

```rust
struct SessionContext {
    user: String,
    org: String,
    project: String,
    team: Option<String>,
    role: Option<RoleDefinition>,
    path_policy: PathPolicy,           // compiled globs from roles.yml
    agent_prompt_hash: Option<String>,
    agent_prompt_path: Option<PathBuf>,
    task_description: Option<String>,
}

static SESSIONS: LazyLock<DashMap<String, SessionContext>> = ...;
```

First call from a new session: ~10ms for git + registration + team config lookup.
Every subsequent call: DashMap lookup in nanoseconds.

## Agent Team Integration

In the context of Claude Code agent teams:

1. **Team lead** spawns a permission supervisor agent alongside worker agents
2. **Supervisor agent** loads all indexes, opens the Unix socket, evaluates requests
3. **Team lead** registers each worker with `hookwise register` after spawning
4. **Worker agents** have `PreToolUse` hooks pointing to `hookwise check`
5. **Each worker** is registered with a role, task description, and prompt
6. **Decisions flow**: worker hook -> binary -> path policy -> cache -> Jaccard -> HNSW -> supervisor -> (maybe human)
7. **Results flow back**: supervisor -> socket -> binary -> JSON stdout -> Claude Code allows/denies

The permission agent is lightweight — mostly routing to the cache/Jaccard/HNSW tiers, rarely invoking the LLM. Path policy decisions (tier 0) never reach the supervisor at all.

## Slash Commands

hookwise ships the following skills as part of its Claude Code plugin:

**`/hookwise register`** — interactive role selection for the current session:
```
> /hookwise register

What role should this session use?

  Implementation:     coder, tester, integrator, devops
  Knowledge:          researcher, architect, planner, reviewer, security-reviewer, docs
  Full-access:        maintainer, troubleshooter

> coder

Registered session abc-123 as "coder".
Path policy: allow src/**, lib/**, Cargo.toml, package.json | deny tests/**, docs/**, .github/**
```

**`/hookwise disable`** — opt out for the current session:
```
> /hookwise disable

hookwise disabled for session abc-123.
All tool calls will be permitted without gating.
To re-enable: /hookwise enable
```

**`/hookwise enable`** — re-enable after disable (prompts for role if not previously registered).

**`/hookwise switch`** — change role mid-session:
```
> /hookwise switch docs

Switched session abc-123 from "coder" to "docs".
Path restrictions: allow docs/**, *.md | deny src/**, tests/**
Session cache cleared — decisions will be re-evaluated for the new role.
```

**`/hookwise status`** — show current session state:
```
> /hookwise status

Session: abc-123
Role: coder
Path policy: allow src/**, lib/** | deny tests/**, docs/**
Cache: 47 allow, 3 deny, 2 ask
Uptime: 23m
```

## Storage Layout

```
<repo>/
+-- .hookwise/
|   +-- policy.yml              # Project policy, sensitive paths, confidence thresholds
|   +-- roles.yml               # Role definitions with path policies
|   +-- rules/                  # Checked into git
|   |   +-- allow.jsonl         # Cached allow decisions (sanitized)
|   |   +-- deny.jsonl          # Cached deny decisions (sanitized)
|   |   +-- ask.jsonl           # Cached ask decisions (sanitized)
|   +-- .index/                 # .gitignored — built locally
|   |   +-- project.hnsw    # HNSW index (instant-distance) rebuilt from rules/
|   |   +-- jaccard.bin     # Token sets for Jaccard similarity
|   +-- .user/                  # .gitignored — personal preferences
|       +-- user.jsonl          # User-level decisions

~/.config/hookwise/
+-- org/
|   +-- <org-name>/
|       +-- policy.yml          # Org-wide rules
|       +-- rules/
|       |   +-- allow.jsonl
|       |   +-- deny.jsonl
|       |   +-- ask.jsonl
|       +-- .index/
|           +-- org.hnsw
+-- user/
|   +-- rules.jsonl             # Personal cross-project rules
|   +-- .index/
|       +-- user.hnsw
+-- config.yml                  # Global hookwise configuration
```

### Git Integration

**Checked in** (`.hookwise/rules/`, `.hookwise/policy.yml`, `.hookwise/roles.yml`):
- Sanitized JSONL — no secrets, human-readable, diffable
- Permission decisions (including `ask` entries) appear in PRs and are reviewable
- New contributors get the project's permission baseline on clone

**Gitignored** (`.hookwise/.index/`, `.hookwise/.user/`):
- Vector indexes — binary, rebuilt from rules
- User preferences — personal, not shared

**Pre-commit safety net:**
A git pre-commit hook runs a secret scan over `.hookwise/rules/` before allowing commits. Belt and suspenders — sanitization should catch everything, but the hook prevents accidents:

```bash
# .git/hooks/pre-commit or via husky/lefthook
hookwise scan --staged .hookwise/rules/
```

## CLI Reference

```
hookwise <command> [options]

HOOK MODE (called by Claude Code):
  check                          Evaluate a tool call. Reads hook payload from stdin as JSON.
                                 Outputs JSON to stdout: {"hookSpecificOutput":{"permissionDecision":"allow|deny|ask"}}
                                 Exit 0 on success, exit 1 on error.

SESSION CHECK (called on user_prompt_submit):
  session-check                  Check if session is registered, output prompt if not

REGISTRATION:
  register                       Register a session with role context
    --session-id <id>            Claude Code session ID
    --role <name>                Role name from roles.yml
    --task <description>         Delegated task description
    --prompt-file <path>         Path to the agent's system prompt (sanitized, stored locally)
  disable                        Disable hookwise for a session
    --session-id <id>            Claude Code session ID
  enable                         Re-enable hookwise for a disabled session
    --session-id <id>            Claude Code session ID

QUEUE MODE (human interface):
  queue                          List pending permission decisions
  approve <id>                   Approve a pending decision (cached as allow)
  deny <id>                      Deny a pending decision (cached as deny)
    --always-ask                 Cache as ask instead of allow/deny
    --add-rule                   Codify as a persistent rule
    --scope <org|project|user>   Which scope to store the rule in (default: project)

CACHE MANAGEMENT:
  build                          Rebuild vector indexes from rules
  invalidate                     Clear cached decisions
    --role <name>                Invalidate for a specific role
    --scope <org|project|user>   Invalidate for a specific scope
    --all                        Invalidate everything

OVERRIDES:
  override                       Set an explicit permission override
    --role <name>                Role to override for
    --command <pattern>          Command pattern
    --tool <name>                Tool name
    --file <glob>                File path pattern (for Write/Edit/Read)
    --allow | --deny | --ask     Decision
    --scope <org|project|user>   Scope (default: project)

MONITORING:
  monitor                        Stream decisions in real time
  stats                          Cache hit rates, decision distribution, ask frequency
  scan --staged <path>           Pre-commit secret scan on staged files

ADMINISTRATION:
  init                           Initialize .hookwise/ in a repo
  config                         View/edit global configuration
  sync                           Pull latest org-level rules
```

## Claude Code Hook Configuration

In the project's `.claude/settings.json` or equivalent:

```json
{
  "hooks": {
    "user_prompt_submit": [
      {
        "matcher": ".*",
        "command": "hookwise session-check"
      }
    ],
    "PreToolUse": [
      {
        "matcher": ".*",
        "command": "hookwise check"
      }
    ]
  }
}
```

Claude Code passes the hook payload as JSON on stdin. The `hookwise check` command reads it, extracts `session_id`, `tool_name`, and `tool_input`, and outputs JSON to stdout:

```json
{
  "hookSpecificOutput": {
    "permissionDecision": "allow"
  }
}
```

Valid values for `permissionDecision`: `"allow"`, `"deny"`, `"ask"`.

The `user_prompt_submit` hook triggers interactive registration for unregistered sessions. The `PreToolUse` hook enforces decisions on every tool call, with the unregistered-session wait-and-block as a safety net.

## Performance Targets

| Operation | Target | Mechanism |
|-----------|--------|-----------|
| Secret sanitization | <10us | aho-corasick + RegexSet + entropy, compiled at startup |
| Path policy check | <1us | Compiled globset patterns |
| Cache hit (allow/deny) | <1us | In-memory HashMap, auto-resolves |
| Cache hit (ask) | immediate escalation | HashMap lookup, then straight to human |
| Token Jaccard similarity | <500ns | Token set intersection/union, precomputed sorted sets |
| Embedding similarity | <5ms | instant-distance HNSW index + fastembed |
| Session context lookup | <100ns (cached) | DashMap, populated once per session |
| Session context first call | <10ms | git remote + registration file + team config |
| LLM evaluation (socket) | 1-3s | Via Unix socket to supervisor subagent |
| LLM evaluation (API) | 1-5s | Via Anthropic API (ApiSupervisor) |
| End-to-end (path deny) | <15us | Sanitize + path policy |
| End-to-end (cache hit) | <15us | Sanitize + path policy + cache lookup |
| End-to-end (Jaccard hit) | <20us | Sanitize + path policy + cache miss + Jaccard |
| End-to-end (embedding hit) | <10ms | Sanitize + path policy + cache miss + Jaccard miss + HNSW |
| End-to-end (LLM) | 1-5s | Full cascade, novel command |
| End-to-end (ask) | variable | Cache hit + human response time |

## Security Considerations

1. **No raw secrets in storage**: All tool input sanitized before cache/vector/JSONL
2. **Pre-commit scanning**: Safety net to prevent accidental secret commits
3. **Deny-wins precedence**: Higher scopes cannot be overridden by lower scopes
4. **Ask-is-authoritative**: Ask decisions cannot be downgraded by lower scopes or LLM
5. **Timeout defaults to deny**: No response from human -> block, not allow
6. **Similarity only auto-approves**: Jaccard and embedding misses fall through, never auto-deny on weak match
7. **Similarity propagates ask**: Jaccard or embedding match against an `ask` entry escalates to human
8. **Socket permissions**: Unix socket file permissions restrict access to current user
9. **Org rules are centrally managed**: Individual contributors cannot weaken org-level policies
10. **Role registration is explicit**: Sessions must be registered before tool calls are permitted
11. **Path policies are deterministic**: File access boundaries are enforced by glob matching, not LLM judgment
12. **Sensitive paths default to ask**: `.claude/`, `.env`, and other sensitive paths prompt by default
13. **Agent prompts are sanitized**: Prompt files stored locally, never checked into git

## Feedback Loop

Every decision feeds back into the system:

```
Decision made (by path policy, cache, Jaccard, embedding, LLM, or human)
  -> Write to scope-appropriate JSONL (sanitized)
  -> Update in-memory cache (immediate)
  -> Update token Jaccard index (immediate, in-memory sorted set)
  -> Rebuild HNSW embedding index (lazy, on next miss or periodic)
  -> Optionally update policy.yml (if human chose +rule)
```

For `ask` decisions, the feedback loop works differently:
- The `ask` state itself is cached and persisted
- Each individual allow/deny response within an `ask` is **not** cached as allow/deny
- The `ask` entry remains until a human explicitly downgrades it

Over the lifetime of a project:
- **Week 1**: Human decides frequently, cache is cold
- **Week 2**: Cache handles 80%+ of decisions, LLM handles most of the rest
- **Month 1**: Human sees maybe 1 question per day, plus any `ask` entries they've set
- **Steady state**: Human is only consulted for genuinely novel tooling, workflow changes, and their chosen `ask` watchpoints

## Dependencies

| Crate | Purpose |
|-------|---------|
| `aho-corasick` | Literal prefix matching for secret detection |
| `regex` | RegexSet for positional secret patterns |
| `dashmap` | Concurrent session context cache |
| `globset` | Compiled glob set for batch path policy matching |
| `instant-distance` | HNSW-indexed vector similarity search (pure Rust, serde support) |
| `fastembed` | Local embedding generation via ONNX Runtime (no external API) |
| `tokio` | Async runtime for socket server and API calls |
| `serde` / `serde_json` | JSONL serialization |
| `clap` | CLI argument parsing |
| `async-trait` | Async trait support for SupervisorBackend |
| `reqwest` | HTTP client for ApiSupervisor |

## Plugin Structure

hookwise ships as a Claude Code plugin. The plugin layout follows the Claude Code plugin specification:

```
hookwise/
+-- .claude-plugin/
|   +-- plugin.json              # Plugin manifest: name, version, description, entrypoint
+-- hooks/
|   +-- hooks.json               # Hook definitions (PreToolUse, user_prompt_submit)
+-- skills/
|   +-- hookwise-register.md # /hookwise register skill
|   +-- hookwise-disable.md  # /hookwise disable skill
|   +-- hookwise-enable.md   # /hookwise enable skill
|   +-- hookwise-switch.md   # /hookwise switch skill
|   +-- hookwise-status.md   # /hookwise status skill
+-- agents/
|   +-- supervisor.md            # Supervisor agent instructions
+-- src/                         # Rust source code
+-- Cargo.toml                   # Rust project manifest
+-- target/release/hookwise  # Compiled binary (built on install)
```

### plugin.json

```json
{
  "name": "hookwise",
  "version": "0.1.0",
  "description": "Intelligent permission gating for Claude Code with learned decisions, role-based access, and multi-agent support.",
  "entrypoint": "target/release/hookwise"
}
```

### hooks.json

```json
{
  "hooks": {
    "user_prompt_submit": [
      {
        "matcher": ".*",
        "command": "hookwise session-check"
      }
    ],
    "PreToolUse": [
      {
        "matcher": ".*",
        "command": "hookwise check"
      }
    ]
  }
}
```

### Skills

Each skill is a markdown file with instructions for Claude Code to execute the corresponding CLI command. Skills are invoked via slash commands (e.g., `/hookwise register`) and map directly to CLI subcommands.

### Supervisor Agent

The `agents/supervisor.md` file contains the system prompt for the LLM supervisor subagent. It includes:
- The project's permission policy context
- Role definitions and path policies
- Instructions for evaluating permission requests
- Confidence scoring guidelines
- When to recommend `ask` vs `allow`/`deny`

The supervisor agent is spawned by the team lead and communicates with worker hooks via the Unix domain socket.

## Future Considerations

- **Web dashboard**: Read/write the pending queue via HTTP, provide org-wide analytics
- **Policy-as-code**: Define policies in a structured DSL rather than natural language
- **Cross-org federation**: Share anonymized permission patterns across organizations
- **IDE integration**: VS Code extension that shows decision stream inline
- **Audit log**: Immutable append-only log of all decisions for compliance
- **Policy suggestions**: LLM proposes policy updates based on accumulated decisions
- **Ask analytics**: Track which `ask` entries are consistently allowed/denied, suggest downgrades
- **Role templates**: Community-maintained role libraries for common project types (web app, CLI tool, data pipeline, etc.)
