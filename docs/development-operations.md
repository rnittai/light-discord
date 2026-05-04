# Development Operations

This repository is being developed inside a disposable Docker container. The user has explicitly allowed destructive operations inside this container when they are useful for the task.

Allowed in this development environment:

- Installing and removing OS packages.
- Starting, stopping, reconfiguring, or deleting local services such as PostgreSQL.
- Removing generated build artifacts, local caches, temporary files, and local test data.
- Running database setup/reset scripts against local development databases.
- Performing destructive git operations when they are needed for this dedicated development repository or AI-managed branches.
- Creating commits and pushing them to the configured remote at reasonable task-sized checkpoints without waiting for separate confirmation.

Still treat these as protected unless the user explicitly asks for them:

- Production deployments.
- Production databases, production storage, billing, or cloud resources.
- Secrets or credentials that could be exposed outside the workspace.
- Protected `main`, `master`, `release`, or production branches if they exist outside this disposable development context.
- Release tags.

If a destructive operation only affects this Docker container or local development data, proceed when it materially helps the task and report what was done.

Git workflow preference:

- Prefer assigning git operations to Claude Code when the tooling is available, including `git status`, `git add`, `git commit`, and `git push`.
- Codex should decide the checkpoint, commit scope, and commit message intent, then review the resulting status/log after Claude Code runs the git commands.
- Commit completed work in coherent chunks.
- Push after successful verification when remote access is available.
- If Claude Code or push fails because authentication, network access, or environment constraints are unavailable, keep the local state intact and report the exact failure before falling back.

AI workflow preference:

- Codex should own overall design, task decomposition, sequencing, code review, verification decisions, and final integration.
- For actual implementation work, Codex should delegate code-writing tasks to Claude Code workers when the tooling is available.
- Git operations should also be delegated to Claude Code workers when practical, with Codex retaining review and integration responsibility.
- Choose the Claude Code model according to the implementation task complexity instead of using one fixed model for every task.
- Codex should review Claude Code changes before committing, run the relevant tests/checks, and integrate or adjust the result as needed.
- If Claude Code delegation is blocked by the environment, record the blocker clearly before falling back to direct implementation.
