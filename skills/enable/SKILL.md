---
name: hookwise enable
description: Re-enable hookwise permission gating for this session
---

# hookwise enable

Re-enable hookwise for a session that was previously disabled.

## Instructions

1. Determine the session ID from the environment.

2. Run:
   ```bash
   hookwise enable --session-id "$SESSION_ID"
   ```

3. If the session was previously registered with a role, confirm re-enablement with:
   - The restored role name
   - The path policy summary for that role

4. If the session was never registered with a role (only disabled), prompt the user to choose a role using the same flow as `/hookwise register`.

5. If the session is not currently disabled, inform the user that hookwise is already active and show the current role.
