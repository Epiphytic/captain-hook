# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-02-08

### Added
- 6-tier decision cascade: path policy (~1us), exact cache (~100ns), token Jaccard (~500ns), embedding HNSW (~1-5ms), LLM supervisor (~1-2s), and human-in-the-loop
- Tri-state decision model (Allow/Deny/Ask) where Ask never auto-resolves, ensuring sensitive operations always get human review
- 12 built-in roles across three categories:
  - Implementation roles: coder, tester, integrator, devops
  - Knowledge roles: researcher, architect, planner, reviewer, security-reviewer, docs
  - Full-access roles: maintainer, troubleshooter
- 4-layer secret sanitization pipeline:
  - aho-corasick literal prefix matching (sk-ant-, ghp_, AKIA, etc.)
  - RegexSet for positional/contextual patterns (bearer tokens, API keys, connection strings)
  - Shannon entropy detection for unknown token formats (20+ chars, entropy > 4.0)
  - Encoding-aware base64/URL detection
- Session registration system with three mechanisms: interactive prompt, CLI registration, and CAPTAIN_HOOK_ROLE env var fallback
- File-backed human decision queue for cross-process communication
- Unix domain socket IPC for supervisor agent communication at /tmp/captain-hook-<team-id>.sock
- Scope hierarchy with DENY > ASK > ALLOW precedence across Org > Project > User > Role levels
- CLI commands:
  - `check` -- PreToolUse hook handler, reads JSON from stdin, outputs permission decision
  - `session-check` -- UserPromptSubmit hook handler, prompts for role registration
  - `register` -- Register a session with a role
  - `disable` / `enable` -- Opt out/in for a session
  - `queue` / `approve` / `deny` -- Human decision queue interface
  - `monitor` -- Stream decisions in real time
  - `stats` -- View cache hit rates and decision distribution
  - `build` -- Rebuild vector indexes from rules
  - `invalidate` -- Clear cached decisions by role, scope, or all
  - `override` -- Set explicit allow/deny/ask overrides per role
  - `init` -- Initialize .captain-hook/ directory in a repository
  - `scan` -- Pre-commit secret scan on staged files
- Claude Code plugin with PreToolUse and UserPromptSubmit hooks, 5 slash commands (register, disable, enable, switch, status), and supervisor agent
- JSONL-based rule storage -- sanitized, git-reviewable, diffable in PRs
- HNSW vector index via instant-distance for semantic similarity search
- Path policy with deterministic globset matching per role
- Concurrent session context cache via DashMap
- Token-level Jaccard similarity for fast approximate matching before embedding lookup
- Custom role definitions via .captain-hook/roles.yml
- Configurable confidence thresholds per scope level in policy.yml
- Sensitive path defaults (`.claude/**`, `.env*`, `.git/hooks/**`) that always resolve to `ask`

[0.1.0]: https://github.com/epiphytic/captain-hook/releases/tag/v0.1.0
