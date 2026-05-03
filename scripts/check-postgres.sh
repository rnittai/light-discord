#!/usr/bin/env bash
set -euo pipefail

DATABASE_URL="${LD_DATABASE_URL:-}"

if [[ -z "${DATABASE_URL}" ]]; then
  echo "LD_DATABASE_URL is not set." >&2
  exit 1
fi

if ! command -v psql >/dev/null 2>&1; then
  echo "psql is not installed. Install the PostgreSQL client package first." >&2
  exit 1
fi

psql "${DATABASE_URL}" -v ON_ERROR_STOP=1 -c 'SELECT current_database(), current_user, version();'

