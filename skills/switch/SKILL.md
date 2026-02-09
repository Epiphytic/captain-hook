---
name: captain-hook switch
description: Switch the current session to a different role
---

# captain-hook switch

Change the role for the current session. This clears cached decisions for the old role and applies the new role's path policies.

## Instructions

1. Determine the session ID from the environment.

2. If the user provided a role name as an argument (e.g., `/captain-hook switch docs`), use it directly.

3. If no role name was provided, present the available roles (same list as `/captain-hook register`) and ask the user to choose via AskUserQuestion.

4. Run:
   ```bash
   captain-hook register --session-id "$SESSION_ID" --role <new-role>
   ```

5. Confirm the role switch to the user, showing:
   - Previous role (if known)
   - New role
   - New path policy summary (allowed and denied write paths)
   - Note that cached decisions for the previous role have been cleared and will be re-evaluated under the new role
