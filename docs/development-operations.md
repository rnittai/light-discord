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

- Commit completed work in coherent chunks.
- Push after successful verification when remote access is available.
- If push fails because authentication or network access is unavailable, keep the local commit and report the exact failure.
