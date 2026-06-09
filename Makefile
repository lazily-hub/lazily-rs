.RECIPEPREFIX := >

CARGO ?= cargo
PYTHON ?= python3

.PHONY: \
	check \
	fmt \
	clippy \
	build \
	build-ffi \
	ffi-headers \
	test \
	test-tokio \
	test-async \
	test-loom \
	test-distributed \
	test-ffi \
	test-ffi-binary \
	test-ipc \
	test-ipc-binary \
	test-ipc-conformance \
	test-signaling-client \
	benchmark-check \
	benchmark-update \
	instrumentation-profile

check: fmt clippy build test test-tokio test-async test-loom test-distributed test-ffi test-ffi-binary test-ipc test-ipc-binary test-ipc-conformance test-signaling-client benchmark-check

fmt:
>$(CARGO) fmt --all --check

clippy:
>$(CARGO) clippy --locked --all-targets --all-features -- -D warnings

build:
>$(CARGO) build --locked --all-targets --all-features

build-ffi:
>$(CARGO) build --locked --features ffi

ffi-headers: build-ffi
>cbindgen --config cbindgen.toml --crate lazily -o target/lazily.h

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

test-ffi:
>$(CARGO) test --locked --features ffi --test ffi

test-ffi-binary:
>$(CARGO) test --locked --features "ffi ipc-binary" --test ffi

test-ipc:
>$(CARGO) test --locked --features ipc --test ipc

test-ipc-binary:
>$(CARGO) test --locked --features ipc-binary --test ipc

test-ipc-conformance:
>$(CARGO) test --locked --features ipc --test conformance

test-signaling-client:
>$(CARGO) test --locked --features signaling-client

benchmark-check:
>$(PYTHON) scripts/update-benchmark-results.py --check

benchmark-update:
>$(PYTHON) scripts/update-benchmark-results.py

instrumentation-profile:
>$(CARGO) run --example instrumentation_profile --features instrumentation --quiet
