---
name: captain-hook enable
description: Re-enable captain-hook permission gating for this session
---

# captain-hook enable

Re-enable captain-hook for a session that was previously disabled.

## Instructions

1. Determine the session ID from the environment.

2. Run:
   ```bash
   captain-hook enable --session-id "$SESSION_ID"
   ```

3. If the session was previously registered with a role, confirm re-enablement with:
   - The restored role name
   - The path policy summary for that role

4. If the session was never registered with a role (only disabled), prompt the user to choose a role using the same flow as `/captain-hook register`.

5. If the session is not currently disabled, inform the user that captain-hook is already active and show the current role.
