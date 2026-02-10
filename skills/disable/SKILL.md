---
name: hookwise disable
description: Disable hookwise permission gating for this session
---

# hookwise disable

Disable hookwise for the current session. When disabled, all tool calls are permitted without permission gating.

## Instructions

1. Determine the session ID from the environment.

2. Run:
   ```bash
   hookwise disable --session-id "$SESSION_ID"
   ```

3. Confirm to the user that hookwise is disabled:
   - All tool calls will be permitted without gating
   - No path policies or role restrictions will be enforced
   - To re-enable: `/hookwise enable`

4. If the command fails (e.g., session not found), report the error to the user.
