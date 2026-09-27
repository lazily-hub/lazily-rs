#!/usr/bin/env python3
"""Pin the DEFAULT-feature dependency graph of `lazily` and prove the pin measures.

Every backend and codec dependency in this crate is optional and feature-gated,
which is easy to assert in prose and easy to lose in a diff: an unconditional
`[build-dependencies]` entry is resolved, downloaded, and compiled for every
build of the crate even when the feature that uses it is off. That is how
prost-build plus all nine `protoc-bin-vendored-*` platform crates -- each
embedding a full protoc binary -- ended up in the default graph of a
reactive-signals library, taking it from 9 crates to 45.

So this guard measures the resolved graph rather than reading the manifest:

  1. the default-feature graph (normal + build edges) must equal ALLOWED_DEFAULT
     exactly -- any new unconditional dependency reddens it, in either direction
  2. every crate in GATED_BACKENDS must appear under `--all-features`

(2) is not decoration. Without it, a `cargo tree` invocation that silently
produced nothing -- a flag rename, a manifest error, a filtered wrapper -- would
satisfy (1) vacuously and report a green result while measuring an empty set.
Both halves read the crate names `cargo` actually resolved; neither reads an
exit status alone.

Exit 0 when both hold, 1 when the pin is violated, 2 on refusal (a measurement
this script cannot trust).
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# The complete default-feature graph, normal + build edges. `lazily` itself and
# its own macro crate are in the list because the measurement must be able to
# prove it parsed real output.
ALLOWED_DEFAULT = frozenset(
    {
        "arc-swap",
        "lazily",
        "lazily-macros",
        "proc-macro2",
        "quote",
        "rustversion",
        "smallvec",
        "syn",
        "unicode-ident",
    }
)

# Gated dependencies that must be absent by default and present under
# `--all-features`. The second half is what keeps the first half honest.
GATED_BACKENDS = frozenset(
    {
        "async-nats",
        "base64",
        "futures-util",
        "leptos_reactive",
        "libc",
        "parking_lot",
        "postcard",
        "postgres",
        "prost",
        "prost-build",
        "protoc-bin-vendored",
        "rmp-serde",
        "rusqlite",
        "serde_json",
        "str0m",
        "tokio",
        "tokio-tungstenite",
    }
)

_VERSION_SUFFIX = re.compile(r" v\d.*$")


class GuardRefusal(RuntimeError):
    """The measurement itself cannot be trusted, so nothing is asserted from it."""


def measure_graph(extra_args: list[str], runner=None) -> frozenset[str]:
    """Return the crate names cargo resolved for this feature selection."""
    argv = [
        "cargo",
        "tree",
        "--locked",
        "--edges",
        "normal,build",
        "--prefix",
        "none",
        "--no-dedupe",
        *extra_args,
    ]
    if runner is None:
        completed = subprocess.run(
            argv, cwd=str(ROOT), capture_output=True, text=True, check=False
        )
        code, out, err = completed.returncode, completed.stdout, completed.stderr
    else:
        code, out, err = runner(argv)

    names = {
        _VERSION_SUFFIX.sub("", line).strip()
        for line in out.splitlines()
        if line.strip() and not line.startswith("[")
    }
    names.discard("")
    if code != 0:
        raise GuardRefusal(
            f"`{' '.join(argv)}` exited {code}; the dependency graph was not measured"
            + (f": {err.strip().splitlines()[-1]}" if err.strip() else "")
        )
    if not names:
        raise GuardRefusal(f"`{' '.join(argv)}` produced no crate names to measure")
    if "lazily" not in names:
        raise GuardRefusal(
            f"`{' '.join(argv)}` output does not contain `lazily` itself, so the parse is "
            f"not reading a dependency tree (got {len(names)} name(s), e.g. "
            f"{sorted(names)[:3]})"
        )
    return frozenset(names)


def check_default_graph(runner=None) -> list[str]:
    measured = measure_graph([], runner=runner)
    added = sorted(measured - ALLOWED_DEFAULT)
    removed = sorted(ALLOWED_DEFAULT - measured)
    failures: list[str] = []
    if added:
        failures.append(
            f"{len(added)} crate(s) entered the DEFAULT dependency graph: {', '.join(added)}"
            " -- make the dependency optional and gate it behind its feature, or add it to"
            " ALLOWED_DEFAULT with a reason"
        )
    if removed:
        failures.append(
            f"{len(removed)} pinned crate(s) left the DEFAULT dependency graph: "
            f"{', '.join(removed)} -- update ALLOWED_DEFAULT"
        )
    return failures


def check_gating_is_observable(runner=None) -> list[str]:
    """The control: the gated crates must be reachable at all."""
    measured = measure_graph(["--all-features"], runner=runner)
    missing = sorted(GATED_BACKENDS - measured)
    if missing:
        return [
            f"{len(missing)} gated crate(s) are absent even under --all-features: "
            f"{', '.join(missing)} -- the default-graph check cannot distinguish "
            "'correctly gated' from 'not measured', so it proves nothing until this is fixed"
        ]
    return []


def run() -> int:
    failures = check_default_graph() + check_gating_is_observable()
    if failures:
        for failure in failures:
            print(f"[default-build-guard] {failure}", file=sys.stderr)
        return 1
    print(
        f"[default-build-guard] default graph is the pinned {len(ALLOWED_DEFAULT)} crate(s); "
        f"all {len(GATED_BACKENDS)} gated crate(s) observable under --all-features",
        file=sys.stderr,
    )
    return 0


# --------------------------------------------------------------------------- #
# Negative controls: prove each assertion can fail.
# --------------------------------------------------------------------------- #


def _fake(code: int, crates: list[str]):
    text = "".join(f"{name} v1.0.0\n" for name in crates)
    return lambda argv: (code, text, "")


def _expect_refusal(label: str, thunk, expected: str) -> None:
    try:
        thunk()
    except GuardRefusal as refusal:
        assert expected in str(refusal), f"{label}: expected {expected!r} in {refusal}"
        return
    raise AssertionError(f"{label}: the measurement returned when it should have refused")


def self_test() -> int:
    pinned = sorted(ALLOWED_DEFAULT)
    everything = sorted(ALLOWED_DEFAULT | GATED_BACKENDS)

    # The happy path over faked output, so the controls below mean something.
    assert check_default_graph(runner=_fake(0, pinned)) == []
    assert check_gating_is_observable(runner=_fake(0, everything)) == []

    # A new unconditional dependency.
    added = check_default_graph(runner=_fake(0, [*pinned, "protoc-bin-vendored"]))
    assert added and "entered the DEFAULT dependency graph" in added[0], added
    assert "protoc-bin-vendored" in added[0], added

    # A pinned crate disappearing is also drift worth a human.
    dropped = check_default_graph(runner=_fake(0, pinned[1:]))
    assert dropped and "left the DEFAULT dependency graph" in dropped[0], dropped

    # The control: a gated crate that cannot be reached under --all-features
    # means the absence check is measuring nothing.
    blind = check_gating_is_observable(runner=_fake(0, [*pinned, *sorted(GATED_BACKENDS)[1:]]))
    assert blind and "absent even under --all-features" in blind[0], blind
    assert "cannot distinguish" in blind[0], blind

    # Refusals: a failed command, empty output, and output that is not a tree.
    _expect_refusal(
        "cargo tree failed",
        lambda: measure_graph([], runner=_fake(101, pinned)),
        "the dependency graph was not measured",
    )
    _expect_refusal(
        "empty output",
        lambda: measure_graph([], runner=_fake(0, [])),
        "produced no crate names",
    )
    _expect_refusal(
        "output is not a dependency tree",
        lambda: measure_graph([], runner=_fake(0, ["some-unrelated-crate"])),
        "does not contain `lazily` itself",
    )
    # An exit status of 0 must not rescue empty output, and a non-zero status
    # must not be rescued by plausible output.
    _expect_refusal(
        "green exit over empty output",
        lambda: measure_graph([], runner=lambda argv: (0, "\n \n", "")),
        "produced no crate names",
    )
    _expect_refusal(
        "red exit over good output",
        lambda: measure_graph([], runner=_fake(1, pinned)),
        "exited 1",
    )

    print("[self-test] default_build_dependency_guard: ok", file=sys.stderr)
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--self-test", action="store_true", help="run the negative controls")
    args = parser.parse_args(argv)
    if args.self_test:
        return self_test()
    try:
        return run()
    except GuardRefusal as refusal:
        print(f"[default-build-guard] refused: {refusal}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
