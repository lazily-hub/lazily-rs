#!/usr/bin/env bash
set -euo pipefail

test_root="$(mktemp -d)"
data_dir="$test_root/data"
postgres_log="$test_root/postgres.log"
nats_log="$test_root/nats.log"
postgres_started=0
nats_container=""

free_port() {
  python3 - <<'PY'
import socket
with socket.socket() as sock:
    sock.bind(("127.0.0.1", 0))
    print(sock.getsockname()[1])
PY
}

cleanup() {
  status=$?
  if [[ -n "$nats_container" ]]; then
    if [[ $status -ne 0 ]]; then
      docker logs "$nats_container" >"$nats_log" 2>&1 || true
    fi
    docker rm -f "$nats_container" >/dev/null 2>&1 || true
  fi
  if [[ $postgres_started -eq 1 ]]; then
    pg_ctl -D "$data_dir" -m fast stop >/dev/null 2>&1 || true
  fi
  if [[ $status -ne 0 ]]; then
    [[ ! -s "$postgres_log" ]] || cat "$postgres_log" >&2
    [[ ! -s "$nats_log" ]] || cat "$nats_log" >&2
  fi
  rm -rf "$test_root"
  exit "$status"
}
trap cleanup EXIT

if [[ -z "${LAZILY_POSTGRES_URL:-}" ]]; then
  for command in initdb pg_ctl psql; do
    if ! command -v "$command" >/dev/null 2>&1; then
      echo "missing PostgreSQL test command: $command" >&2
      exit 1
    fi
  done
  postgres_port="$(free_port)"
  initdb -D "$data_dir" -A trust -U postgres --no-locale >/dev/null
  pg_ctl -D "$data_dir" -l "$postgres_log" -o "-F -h 127.0.0.1 -k $test_root -p $postgres_port" start >/dev/null
  postgres_started=1
  export LAZILY_POSTGRES_URL="postgresql://postgres@127.0.0.1:$postgres_port/postgres"
fi

if [[ -z "${LAZILY_NATS_URL:-}" ]]; then
  if ! command -v docker >/dev/null 2>&1; then
    echo "missing Docker for the real NATS JetStream test service" >&2
    exit 1
  fi
  nats_port="$(free_port)"
  nats_container="lazily-nats-${$}-${nats_port}"
  docker run -d --rm --name "$nats_container" \
    -p "127.0.0.1:$nats_port:4222" \
    nats:2.12.1-alpine -js >/dev/null
  export LAZILY_NATS_URL="nats://127.0.0.1:$nats_port"
  python3 - "$nats_port" <<'PY'
import socket
import sys
import time

port = int(sys.argv[1])
deadline = time.monotonic() + 15
while True:
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=0.2):
            break
    except OSError:
        if time.monotonic() >= deadline:
            raise SystemExit("NATS did not become ready within 15 seconds")
        time.sleep(0.05)
PY
fi

cargo test --locked --features durable-jetstream --test durable_jetstream_integration -- --test-threads=1
