# captain-hook

Intelligent permission gating for Claude Code.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)

## Overview

Claude Code's built-in permission system is per-session and binary: approve or deny each tool call, every time. In multi-agent environments -- agent teams, swarms, parallel workers -- this creates permission fatigue (hundreds of prompts) and zero institutional memory (every session starts cold).

captain-hook solves both problems with a learned permission policy that gets smarter over time. It sits between Claude Code and your tools as a `PreToolUse` hook, running a fast decision cascade that resolves most tool calls in microseconds from cache. Only genuinely novel or ambiguous operations reach a human.

Decisions are cached as sanitized JSONL, checked into git, and shared across contributors. New team members inherit the project's permission baseline on clone.

## Installation

### From GitHub releases

Download the prebuilt binary for your platform from the [latest release](https://github.com/epiphytic/captain-hook/releases/latest):

```bash
# macOS (Apple Silicon)
curl -L https://github.com/epiphytic/captain-hook/releases/latest/download/captain-hook-aarch64-apple-darwin.tar.gz \
  | tar xz -C /usr/local/bin

# macOS (Intel)
curl -L https://github.com/epiphytic/captain-hook/releases/latest/download/captain-hook-x86_64-apple-darwin.tar.gz \
  | tar xz -C /usr/local/bin

# Linux (x86_64)
curl -L https://github.com/epiphytic/captain-hook/releases/latest/download/captain-hook-x86_64-unknown-linux-gnu.tar.gz \
  | tar xz -C /usr/local/bin
```

### From source

Requires Rust 1.75+:

```bash
cargo install --path .
```

Or build a release binary directly:

```bash
cargo build --release
# Binary at target/release/captain-hook
```

### Install as a Claude Code plugin

After the binary is in your PATH, install the plugin:

```bash
claude plugin add /path/to/captain-hook
```

This registers two hooks automatically:

- **`PreToolUse`** -- runs `captain-hook check` on every tool call
- **`UserPromptSubmit`** -- runs `captain-hook session-check` to prompt for role registration

It also provides slash commands (`/captain-hook register`, `disable`, `enable`, `switch`, `status`).

## Quick Start

### Initialize in a repository

```bash
cd your-repo
captain-hook init
```

This creates `.captain-hook/` with default `policy.yml`, `roles.yml`, and empty rule files.

### Register a role

```bash
# From a terminal
captain-hook register --session-id "$SESSION_ID" --role coder

# Or via environment variable (CI/scripted use)
export CAPTAIN_HOOK_ROLE=coder
```

Once the plugin is installed, your first prompt will trigger an interactive role selection automatically.

## How It Works

Every tool call passes through a 7-stage cascade. Each stage either resolves the decision or escalates to the next:

```
Tool call -> Sanitize -> Path Policy -> Cache -> Token Jaccard -> Embedding HNSW -> LLM -> Human
               ~5us        ~1us        ~100ns      ~500ns           ~1-5ms        ~1-2s  variable
```

| Tier | Mechanism | Latency | Resolves when |
|------|-----------|---------|---------------|
| -- | Secret sanitization | ~5us | Always runs first; redacts secrets before any lookup |
| 0 | Path policy (globset) | ~1us | File path matches a role's allow/deny glob |
| 1 | Exact cache match | ~100ns | Sanitized command seen before (allow/deny auto-resolve; ask escalates to human) |
| 2a | Token Jaccard similarity | ~500ns | Token overlap with cached entry exceeds threshold (default 0.7) |
| 2b | Embedding HNSW similarity | ~1-5ms | Semantically similar command was previously decided |
| 3 | LLM supervisor agent | ~1-2s | Novel command evaluated against policy by supervisor |
| 4 | Human-in-the-loop | variable | LLM confidence too low, or `ask` state requires human judgment |

Every decision at tiers 3 and 4 feeds back into tiers 1, 2a, and 2b. The system converges toward full autonomy over time.

### Tri-State Decisions

Three decision states, not two:

- **allow** -- permit the tool call, cached, auto-resolves on future matches
- **deny** -- block the tool call, cached, auto-resolves on future matches
- **ask** -- always prompt a human, cached as `ask` so it never auto-resolves

`ask` is for operations that should always get human eyes: writing to `.claude/`, `.env`, settings files, or any operation the user wants to stay aware of.

Precedence: **DENY > ASK > ALLOW > silent**

## Roles

Twelve built-in roles across three categories. Each role combines deterministic path globs (tier 0) with a natural language description (tier 3 LLM).

### Implementation roles

Write to specific code/config directories.

| Role | Writes to | Denied from |
|------|-----------|-------------|
| `coder` | `src/`, `lib/`, project config (`Cargo.toml`, `package.json`, etc.) | `tests/`, `docs/`, `.github/`, `*.tf` |
| `tester` | `tests/`, `test-fixtures/`, `*.test.*`, `*_test.go`, test configs | `src/`, `lib/`, `docs/`, `.github/` |
| `integrator` | `*.tf`, `*.tfvars`, `terraform/`, `infra/`, `pulumi/`, `helm/`, `ansible/` | `src/`, `lib/`, `tests/`, `docs/` |
| `devops` | `.github/`, `Dockerfile*`, `docker-compose*`, `.*rc`, tool version files | `src/`, `lib/`, `tests/`, `docs/` |

### Knowledge roles

Read the codebase, write artifacts to `docs/` subdirectories.

| Role | Writes to | Denied from |
|------|-----------|-------------|
| `researcher` | `docs/research/` | `src/`, `lib/`, `tests/`, `.github/` |
| `architect` | `docs/architecture/`, `docs/adr/` | `src/`, `lib/`, `tests/`, `.github/` |
| `planner` | `docs/plans/` | `src/`, `lib/`, `tests/`, `.github/` |
| `reviewer` | `docs/reviews/` (not `security/`) | `src/`, `lib/`, `tests/`, `.github/` |
| `security-reviewer` | `docs/reviews/security/` | `src/`, `lib/`, `tests/`, `.github/` |
| `docs` | `docs/`, `*.md`, `*.aisp` | `src/`, `lib/`, `tests/`, `.github/` |

### Full-access roles

Unrestricted file access for leads and debugging.

| Role | Writes to | Denied from |
|------|-----------|-------------|
| `maintainer` | `**` | (none) |
| `troubleshooter` | `**` | (none) |

Knowledge roles produce artifacts that implementation roles consume:
`researcher` -> `architect` -> `planner` -> `coder`/`tester` -> `reviewer` -> `maintainer`

Projects can define custom roles in `.captain-hook/roles.yml`.

## CLI Reference

```
captain-hook <command> [options]
```

### Hook mode

Called by Claude Code on every `PreToolUse` event. Reads hook payload from stdin as JSON, outputs a permission decision to stdout.

```bash
echo '{"session_id":"abc","tool_name":"Bash","tool_input":{"command":"pytest"}}' \
  | captain-hook check
# {"hookSpecificOutput":{"permissionDecision":"allow"}}
```

### Session check

Called on `UserPromptSubmit`. Outputs a registration prompt if the session is unregistered.

```bash
captain-hook session-check
```

### Registration

```bash
# Register a session with a role
captain-hook register --session-id <id> --role <role> \
  [--task <description>] [--prompt-file <path>]

# Disable captain-hook for a session
captain-hook disable --session-id <id>

# Re-enable after disable
captain-hook enable --session-id <id>
```

### Queue mode (human interface)

```bash
# List pending permission decisions
captain-hook queue

# Approve or deny a pending decision
captain-hook approve <id>
captain-hook deny <id>

# Cache as "ask" instead of allow/deny
captain-hook approve <id> --always-ask

# Codify as a persistent rule
captain-hook approve <id> --add-rule --scope project
```

### Monitoring

```bash
# Stream decisions in real time
captain-hook monitor

# View cache hit rates and decision distribution
captain-hook stats
```

### Cache management

```bash
# Rebuild vector indexes from rules
captain-hook build

# Clear cached decisions
captain-hook invalidate --role <role>
captain-hook invalidate --scope project
captain-hook invalidate --all
```

### Overrides

Set explicit permission overrides that take priority over cached LLM decisions.

```bash
captain-hook override --role tester --command "docker compose up" --allow
captain-hook override --role coder --tool Write --file ".claude/*" --ask
captain-hook override --role coder --command "npm publish" --deny --scope project
```

### Initialization and scanning

```bash
# Initialize .captain-hook/ in a repo
captain-hook init

# Pre-commit secret scan on staged files
captain-hook scan --staged .captain-hook/rules/
```

## Configuration

### policy.yml

Project-level policy: sensitive paths, confidence thresholds, and behavioral settings.

```yaml
sensitive_paths:
  ask_write:
    - ".claude/**"
    - ".captain-hook/**"
    - ".env*"
    - "**/.env*"
    - ".git/hooks/**"

confidence:
  org: 0.9
  project: 0.7
  user: 0.6
```

### roles.yml

Role definitions with path policies. See [Roles](#roles) for the built-in set. Add custom roles here:

```yaml
roles:
  data-engineer:
    description: |
      Manages data pipelines, ETL scripts, and database migrations.
    paths:
      allow_write: ["pipelines/**", "migrations/**", "sql/**"]
      deny_write: ["src/**", "tests/**", "docs/**"]
      allow_read: ["**"]
```

### Storage layout

```
<repo>/
  .captain-hook/
    policy.yml              # Project policy (checked into git)
    roles.yml               # Role definitions (checked into git)
    rules/                  # Cached decisions (checked into git)
      allow.jsonl
      deny.jsonl
      ask.jsonl
    .index/                 # Vector indexes (.gitignored, rebuilt locally)
    .user/                  # Personal preferences (.gitignored)

~/.config/captain-hook/
  config.yml                # Global configuration
  org/<org-name>/           # Org-wide rules
  user/                     # Personal cross-project rules
```

Rules are sanitized JSONL -- no secrets, human-readable, diffable, reviewable in PRs.

### Scope hierarchy

Four scopes with strict precedence:

| Scope | What it governs | Where it lives |
|-------|-----------------|----------------|
| Org | Security floor for all repos | `~/.config/captain-hook/org/<org>/` |
| Project | Project-specific permissions | `<repo>/.captain-hook/rules/` |
| User | Personal preferences | `~/.config/captain-hook/user/` |
| Role | Task-scoped least privilege | Set at registration time |

**DENY > ASK > ALLOW** at every level. A deny at any scope is authoritative.

## Plugin Setup

captain-hook ships as a Claude Code plugin. After building:

```bash
cargo build --release
claude plugin add /path/to/captain-hook
```

The plugin registers two hooks automatically:

- **`PreToolUse`** -- runs `captain-hook check` on every tool call
- **`UserPromptSubmit`** -- runs `captain-hook session-check` to prompt for role registration

It also provides slash commands:

| Command | Description |
|---------|-------------|
| `/captain-hook register` | Pick a role interactively |
| `/captain-hook disable` | Opt out for this session |
| `/captain-hook enable` | Re-enable after disable |
| `/captain-hook switch` | Change role mid-session |
| `/captain-hook status` | Show current role, path policy, cache stats |

### Agent team setup

In multi-agent environments, the team lead registers each worker after spawning:

```bash
captain-hook register \
  --session-id "$WORKER_SESSION_ID" \
  --role tester \
  --task "Run pytest suite for auth module" \
  --prompt-file /tmp/.captain-hook-prompt-$WORKER_SESSION_ID
```

The LLM supervisor agent communicates with worker hooks over a Unix domain socket at `/tmp/captain-hook-<team-id>.sock`.

## Troubleshooting

### Hook not firing

If captain-hook is not intercepting tool calls:

1. **Verify the plugin is installed**: Run `claude plugin list` and confirm `captain-hook` appears.
2. **Check hooks.json**: The plugin directory should contain a `hooks.json` with `PreToolUse` and `UserPromptSubmit` entries. If missing, reinstall with `claude plugin add /path/to/captain-hook`.
3. **Confirm the binary is in PATH**: Run `which captain-hook` -- if it returns nothing, add the install directory to your PATH.
4. **Check for errors**: Run `captain-hook check` manually with sample input to see if the binary starts correctly:
   ```bash
   echo '{"session_id":"test","tool_name":"Bash","tool_input":{"command":"ls"}}' \
     | captain-hook check
   ```

### Session registration timeout

If you see "session not registered" errors or the hook blocks after 5 seconds:

1. **Use env var fallback**: Set `CAPTAIN_HOOK_ROLE=coder` (or your desired role) as an environment variable. This bypasses the interactive registration flow.
2. **Check registration file permissions**: The registration state is stored under `.captain-hook/`. Ensure the current user has read/write access.
3. **Register explicitly**: Run `captain-hook register --session-id "$SESSION_ID" --role <role>` before starting your session.

### Permission denied on socket

The LLM supervisor agent communicates over a Unix domain socket at `/tmp/captain-hook-<team-id>.sock`:

1. **Check file permissions**: Ensure the socket file is readable/writable by the current user.
2. **Stale socket**: If a previous session crashed, a stale socket may remain. Remove it manually: `rm /tmp/captain-hook-*.sock` and restart.
3. **tmpdir restrictions**: On some systems, `/tmp/` has restrictive permissions. Check your OS security settings (e.g., macOS sandboxing).

### Secret false positives

If the sanitizer is flagging non-secret strings:

1. **Adjust entropy threshold**: In `.captain-hook/policy.yml`, increase the Shannon entropy threshold above the default 4.0:
   ```yaml
   sanitization:
     entropy_threshold: 4.5
   ```
2. **Check what triggered it**: Run `captain-hook scan --staged` to see which patterns matched. The sanitizer uses three layers (aho-corasick prefixes, regex patterns, entropy) -- the output indicates which layer flagged the string.
3. **Add allowlist entries**: For known safe patterns that repeatedly trigger false positives, add them to the allowlist in `policy.yml`.

### Vector index needs rebuild

If similarity search returns stale or no results after editing rule files:

```bash
captain-hook build
```

This rebuilds the HNSW index from the current JSONL rule files. The index is stored in `.captain-hook/.index/` (gitignored) and must be rebuilt locally after cloning or pulling new rules.

## Contributing

1. Fork the repository
2. Create a feature branch
3. Run `cargo test` and `cargo clippy` before submitting
4. Ensure `captain-hook scan --staged` passes (no secrets in committed files)
5. Open a pull request

## License

MIT
