---
name: captain-hook disable
description: Disable captain-hook permission gating for this session
---

# captain-hook disable

Disable captain-hook for the current session. When disabled, all tool calls are permitted without permission gating.

## Instructions

1. Determine the session ID from the environment.

2. Run:
   ```bash
   captain-hook disable --session-id "$SESSION_ID"
   ```

3. Confirm to the user that captain-hook is disabled:
   - All tool calls will be permitted without gating
   - No path policies or role restrictions will be enforced
   - To re-enable: `/captain-hook enable`

4. If the command fails (e.g., session not found), report the error to the user.
