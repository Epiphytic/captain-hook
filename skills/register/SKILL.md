---
name: hookwise register
description: Register the current session with a role for permission gating
---

# hookwise register

Register this session with a role. The role determines what file paths and tool calls are permitted without human approval. Each session must be registered before hookwise will allow tool calls.

## Instructions

1. Determine the session ID from the environment. Use the `SESSION_ID` environment variable if available, or derive it from the Claude Code session context.

2. Present the available roles to the user grouped by category. Use AskUserQuestion to let them choose:

   **Implementation roles** (write to specific code/config directories):
   - `coder` -- modify src/, lib/, project config (Cargo.toml, package.json, etc.)
   - `tester` -- modify tests/, test configs, coverage configs
   - `integrator` -- terraform, pulumi, CDK, ansible, helm files
   - `devops` -- CI/CD pipelines, Dockerfiles, tooling config files

   **Knowledge roles** (read codebase, write artifacts to docs/ subdirectories):
   - `researcher` -- write to docs/research/
   - `architect` -- write to docs/architecture/, docs/adr/
   - `planner` -- write to docs/plans/
   - `reviewer` -- write to docs/reviews/ (not security/)
   - `security-reviewer` -- write to docs/reviews/security/, run security scanners
   - `docs` -- write to docs/, *.md, *.aisp

   **Full-access roles** (unrestricted file access):
   - `maintainer` -- full repository access
   - `troubleshooter` -- full access for debugging

   **Other options:**
   - `disable` -- turn off hookwise for this session

3. If the user chooses `disable`, run:
   ```bash
   hookwise disable --session-id "$SESSION_ID"
   ```

4. Otherwise, register with the chosen role:
   ```bash
   hookwise register --session-id "$SESSION_ID" --role <chosen-role>
   ```

5. Confirm the registration to the user, showing:
   - The registered role name
   - A summary of allowed and denied write paths for that role
   - A note that sensitive paths (.claude/, .env, etc.) always prompt regardless of role
