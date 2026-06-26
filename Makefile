.RECIPEPREFIX := >

CARGO ?= cargo
PYTHON ?= python3
LAKE ?= lake
LEAN_SPEC_DIR ?= ../lazily-spec/formal/lean

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
	test-async-resolve \
	test-loom \
	test-distributed \
	test-ffi \
	test-ffi-binary \
	test-ipc \
	test-ipc-binary \
	test-ipc-conformance \
	test-lean-formal \
	test-signaling-client \
	test-webrtc \
	test-websocket \
	benchmark-check \
	benchmark-update \
	instrumentation-profile

check: fmt clippy build test test-tokio test-async test-async-resolve test-loom test-distributed test-crdt-plane test-ffi test-ffi-binary test-ipc test-ipc-binary test-ipc-conformance test-lean-formal test-signaling-client test-webrtc test-webrtc-signaling test-websocket benchmark-check

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

# Deterministic #k03k resolve-loop window coverage needs the instrumentation
# seam (window 1) alongside the async feature; test-async alone compiles it out.
test-async-resolve:
>$(CARGO) test --locked --features "async instrumentation" --test async_resolve_loop

test-loom:
>$(CARGO) test --locked --features loom --test thread_safe_loom

test-distributed:
>$(CARGO) test --locked --features "distributed serde"

# Distributed CRDT plane runtime integration (#lzcrdtplane5b): the
# CrdtPlaneRuntime glue + the end-to-end two-replica-over-transport convergence
# test need BOTH the plane primitives (`distributed`) and the wire + in-memory
# DataChannel transport (`webrtc`), a combo no other target exercises.
test-crdt-plane:
>$(CARGO) test --locked --features "distributed webrtc"

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

test-lean-formal:
>test -d "$(LEAN_SPEC_DIR)" || { echo "missing $(LEAN_SPEC_DIR); clone lazily-spec as a sibling or set LEAN_SPEC_DIR"; exit 1; }
>cd "$(LEAN_SPEC_DIR)" && $(LAKE) build

test-signaling-client:
>$(CARGO) test --locked --features signaling-client

# WebRTC DataChannel transport (#webrtc2/#webrtc3) + concrete str0m backends:
# the deterministic in-memory/synthetic-clock loopback (#webrtcbackend) plus the
# networked Str0mNet backend (#lzwebrtcnet), whose test does a real two-socket
# round trip over 127.0.0.1 (real UDP/DTLS/SCTP/timers).
test-webrtc:
>$(CARGO) test --locked --features webrtc-str0m

# Full WebRTC handshake driven THROUGH SignalingClient over a loopback signaling
# relay (#lzwebrtcwire): real WebSocket offer/answer/ICE on 127.0.0.1 plus the
# real Str0mNet UDP/DTLS/SCTP transport. Needs both feature trees.
test-webrtc-signaling:
>$(CARGO) test --locked --features "signaling-client webrtc-str0m" --test webrtc_signaling

# WebSocket DataChannel backend (#akp3): in-process loopback over a real WS
# handshake, no real network.
test-websocket:
>$(CARGO) test --locked --features websocket

benchmark-check:
>$(PYTHON) scripts/update-benchmark-results.py --check

benchmark-update:
>$(PYTHON) scripts/update-benchmark-results.py

instrumentation-profile:
>$(CARGO) run --example instrumentation_profile --features instrumentation --quiet
