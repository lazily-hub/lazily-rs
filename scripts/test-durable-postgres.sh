#!/usr/bin/env bash
set -euo pipefail

if [[ -n "${LAZILY_POSTGRES_URL:-}" ]]; then
  cargo test --locked --features durable-postgres --test durable_postgres_integration -- --test-threads=1
  exit
fi

for command in initdb pg_ctl psql; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "missing PostgreSQL test command: $command" >&2
    exit 1
  fi
done

test_root="$(mktemp -d)"
data_dir="$test_root/data"
log_file="$test_root/postgres.log"
port="$(python3 - <<'PY'
import socket
with socket.socket() as sock:
    sock.bind(("127.0.0.1", 0))
    print(sock.getsockname()[1])
PY
)"

cleanup() {
  status=$?
  pg_ctl -D "$data_dir" -m fast stop >/dev/null 2>&1 || true
  if [[ $status -ne 0 && -s "$log_file" ]]; then
    cat "$log_file" >&2
  fi
  rm -rf "$test_root"
  exit "$status"
}
trap cleanup EXIT

initdb -D "$data_dir" -A trust -U postgres --no-locale >/dev/null
pg_ctl -D "$data_dir" -l "$log_file" -o "-F -h 127.0.0.1 -k $test_root -p $port" start >/dev/null
export LAZILY_POSTGRES_URL="postgresql://postgres@127.0.0.1:$port/postgres"

cargo test --locked --features durable-postgres --test durable_postgres_integration -- --test-threads=1
