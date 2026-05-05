# Claude Code Delegation Rule

When a request contains multiple implementation areas, do not ask Claude Code to implement the whole request in one prompt.

Codex must first split the work into small, bounded implementation tasks, then delegate those tasks to Claude Code with clear ownership.

Each Claude Code task should include:

- The specific goal.
- Files or directories it may edit.
- Files or directories it must not edit.
- Acceptance criteria.
- Verification commands.
- A reminder not to revert unrelated user or worker changes.

Preferred flow:

1. Codex decomposes the request.
2. Codex identifies dependencies between subtasks.
3. Claude Code receives one focused implementation task at a time.
4. Codex reviews each Claude Code diff.
5. Codex integrates, fixes conflicts or gaps, and runs final verification.

Do not use the pattern "try one large Claude Code task first, then split if it stalls." For multi-part implementation work, split first.

