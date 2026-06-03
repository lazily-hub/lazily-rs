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
	test-loom \
	benchmark-check \
	benchmark-update \
	instrumentation-profile

check: fmt clippy build test test-tokio test-loom benchmark-check

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

test-loom:
>$(CARGO) test --locked --features loom --test thread_safe_loom

benchmark-check:
>$(PYTHON) scripts/update-benchmark-results.py --check

benchmark-update:
>$(PYTHON) scripts/update-benchmark-results.py

instrumentation-profile:
>$(CARGO) run --example instrumentation_profile --features instrumentation --quiet
