#!/usr/bin/env bash
set -euo pipefail

DB_NAME="${LD_PG_DB:-light_discord}"
DB_USER="${LD_PG_USER:-light_discord}"
DB_PASSWORD="${LD_PG_PASSWORD:-}"

if [[ -z "${DB_PASSWORD}" ]]; then
  cat >&2 <<'EOF'
LD_PG_PASSWORD is required.

Example:
  export LD_PG_PASSWORD='replace-with-a-long-random-password'
  scripts/setup-postgres-linux.sh
EOF
  exit 1
fi

if [[ "${EUID}" -ne 0 ]]; then
  if command -v sudo >/dev/null 2>&1; then
    exec sudo -E bash "$0" "$@"
  fi

  echo "This script needs root privileges to install/start PostgreSQL." >&2
  exit 1
fi

install_postgres() {
  if command -v apt-get >/dev/null 2>&1; then
    apt-get update
    DEBIAN_FRONTEND=noninteractive apt-get install -y postgresql postgresql-client
    return
  fi

  if command -v dnf >/dev/null 2>&1; then
    dnf install -y postgresql-server postgresql-contrib
    if command -v postgresql-setup >/dev/null 2>&1 && [[ ! -d /var/lib/pgsql/data/base ]]; then
      postgresql-setup --initdb
    fi
    return
  fi

  if command -v yum >/dev/null 2>&1; then
    yum install -y postgresql-server postgresql-contrib
    if command -v postgresql-setup >/dev/null 2>&1 && [[ ! -d /var/lib/pgsql/data/base ]]; then
      postgresql-setup --initdb
    fi
    return
  fi

  if command -v zypper >/dev/null 2>&1; then
    zypper --non-interactive install postgresql-server postgresql-contrib
    return
  fi

  echo "Unsupported package manager. Install PostgreSQL server/client manually first." >&2
  exit 1
}

start_postgres() {
  if command -v systemctl >/dev/null 2>&1; then
    systemctl enable --now postgresql 2>/dev/null && return
    systemctl enable --now postgresql.service 2>/dev/null && return
  fi

  if command -v service >/dev/null 2>&1; then
    service postgresql start 2>/dev/null && return
  fi

  if command -v pg_ctlcluster >/dev/null 2>&1; then
    local cluster
    cluster="$(pg_lsclusters --no-header | awk 'NR == 1 {print $1, $2}')"
    if [[ -n "${cluster}" ]]; then
      read -r version name <<<"${cluster}"
      pg_ctlcluster "${version}" "${name}" start 2>/dev/null || true
      return
    fi
  fi

  echo "PostgreSQL was installed, but the script could not start the service automatically." >&2
  echo "Start it manually, then rerun this script." >&2
  exit 1
}

psql_as_postgres() {
  if command -v runuser >/dev/null 2>&1; then
    runuser -u postgres -- psql "$@"
    return
  fi

  if command -v sudo >/dev/null 2>&1; then
    sudo -u postgres psql "$@"
    return
  fi

  echo "Cannot run psql as the postgres OS user." >&2
  exit 1
}

create_database_and_user() {
  psql_as_postgres \
    -v ON_ERROR_STOP=1 \
    -v db_name="${DB_NAME}" \
    -v db_user="${DB_USER}" \
    -v db_password="${DB_PASSWORD}" <<'SQL'
SELECT format('CREATE ROLE %I LOGIN PASSWORD %L', :'db_user', :'db_password')
WHERE NOT EXISTS (
  SELECT 1 FROM pg_roles WHERE rolname = :'db_user'
)\gexec

ALTER ROLE :"db_user" WITH LOGIN PASSWORD :'db_password';

SELECT format('CREATE DATABASE %I OWNER %I', :'db_name', :'db_user')
WHERE NOT EXISTS (
  SELECT 1 FROM pg_database WHERE datname = :'db_name'
)\gexec

GRANT ALL PRIVILEGES ON DATABASE :"db_name" TO :"db_user";
SQL
}

install_postgres
start_postgres
create_database_and_user

cat <<EOF
PostgreSQL is ready for Light Discord.

Use this server environment variable:
  export LD_DATABASE_URL='postgres://${DB_USER}:${DB_PASSWORD}@localhost:5432/${DB_NAME}'

If your password contains URL special characters, URL-encode it in LD_DATABASE_URL.
EOF

