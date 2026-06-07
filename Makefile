.RECIPEPREFIX := >

CARGO ?= cargo
PYTHON ?= python3

.PHONY: \
	check \
	fmt \
	clippy \
	build \
	test \
	test-tokio \
	test-async \
	test-loom \
	test-distributed \
	test-ipc \
	test-signaling-client \
	benchmark-check \
	benchmark-update \
	instrumentation-profile

check: fmt clippy build test test-tokio test-async test-loom test-distributed test-ipc test-signaling-client benchmark-check

fmt:
>$(CARGO) fmt --all --check

clippy:
>$(CARGO) clippy --locked --all-targets --all-features -- -D warnings

build:
>$(CARGO) build --locked --all-targets --all-features

test:
>$(CARGO) test --locked

test-tokio:
>$(CARGO) test --locked --features tokio

test-async:
>$(CARGO) test --locked --features async

test-loom:
>$(CARGO) test --locked --features loom --test thread_safe_loom

test-distributed:
>$(CARGO) test --locked --features "distributed serde"

test-ipc:
>$(CARGO) test --locked --features ipc --test ipc

test-signaling-client:
>$(CARGO) test --locked --features signaling-client

benchmark-check:
>$(PYTHON) scripts/update-benchmark-results.py --check

benchmark-update:
>$(PYTHON) scripts/update-benchmark-results.py

instrumentation-profile:
>$(CARGO) run --example instrumentation_profile --features instrumentation --quiet
