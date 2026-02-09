---
name: captain-hook status
description: Show the current session's captain-hook status and cache statistics
---

# captain-hook status

Display the current captain-hook status for this session, including role information, path policies, and cache statistics.

## Instructions

1. Run:
   ```bash
   captain-hook stats
   ```

2. Present the output to the user in a clear format, including:
   - **Session ID**: the current session identifier
   - **Status**: active (with role name), disabled, or unregistered
   - **Role**: the current role name and its description
   - **Path policy**: summary of allowed and denied write paths for the role
   - **Cache statistics**:
     - Total entries (allow / deny / ask breakdown)
     - Hit rate (percentage of tool calls resolved from cache)
     - Number of pending decisions in the queue
   - **Sensitive paths**: list of paths that always prompt regardless of role
