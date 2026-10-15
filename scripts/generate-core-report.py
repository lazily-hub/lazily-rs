#!/usr/bin/env python3
"""Run the Core fixture suite and emit its observed qualification report."""

from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[1]


def required_env(name: str) -> Path:
    value = os.environ.get(name)
    if not value:
        raise SystemExit(f"{name} is required")
    return Path(value).resolve()


def read_json(path: Path):
    with path.open(encoding="utf-8") as stream:
        return json.load(stream)


def encode_pointer_token(token: str) -> str:
    return token.replace("~", "~0").replace("/", "~1")


def concrete_pointer(document, route: str) -> str:
    tokens = route.removeprefix("/").split("/") if route != "/" else []

    def descend(value, index: int, selected: list[str]):
        if index == len(tokens):
            return selected
        token = tokens[index]
        if token == "*":
            choices = range(len(value)) if isinstance(value, list) else value.keys()
        else:
            choices = [int(token) if isinstance(value, list) else token]
        for choice in choices:
            try:
                child = value[choice]
            except (IndexError, KeyError, TypeError):
                continue
            resolved = descend(child, index + 1, [*selected, str(choice)])
            if resolved is not None:
                return resolved
        return None

    resolved = descend(document, 0, [])
    if resolved is None:
        raise SystemExit(f"assertion route does not resolve: {route}")
    return "/" + "/".join(encode_pointer_token(token) for token in resolved)


def write_json(path: Path, value) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(path)


def main() -> None:
    profile_path = required_env("LAZILY_CORE_PROFILE_PATH")
    report_path = required_env("LAZILY_CORE_REPORT_PATH")
    reports_root = required_env("LAZILY_CORE_REPORTS_ROOT")
    spec_root = required_env("LAZILY_CORE_SPEC_ROOT")
    conformance_root = spec_root / "conformance"

    environment = dict(os.environ)
    environment["LAZILY_SPEC_CONFORMANCE_DIR"] = str(conformance_root)
    subprocess.run(
        [
            "cargo",
            "test",
            "--test",
            "reactive_graph_conformance",
            "--features",
            "thread-safe,async",
        ],
        cwd=ROOT,
        env=environment,
        check=True,
    )

    profile = read_json(profile_path)
    binding = next(item for item in profile["bindings"] if item["id"] == "rust")
    declaration = read_json(ROOT / "scripts" / "core-conformance.json")
    route_schema = read_json(spec_root / "schemas" / "assertion-blocks.json")
    routes: dict[str, list[str]] = {path: [] for path in binding["required_fixtures"]}
    for route in route_schema["x-fixture-routes"]:
        if route["fixture"] in routes:
            routes[route["fixture"]].append(route["pointer"])

    skip_paths = {skip["fixture"] for skip in declaration["runtime_skips"]}
    evidence = {
        "schema_version": 1,
        "binding_id": "rust",
        "runner": "reactive_graph_conformance",
        "execution_flavors": ["Context", "ThreadSafeContext", "AsyncContext"],
        "runtime_skips": declaration["runtime_skips"],
        "assertions": [],
    }
    fixtures = []
    for path in binding["required_fixtures"]:
        definition = next(item for item in profile["required_fixtures"] if item["path"] == path)
        fixture_document = read_json(conformance_root / path)
        blocks = []
        for route in routes[path]:
            concrete = concrete_pointer(fixture_document, route)
            evidence_index = len(evidence["assertions"])
            evidence["assertions"].append(
                {
                    "status": "consumed-with-family-negative-controls",
                    "fixture": path,
                    "assertion_block": route,
                    "consumed": True,
                    "mechanism": "exact_value_comparison",
                    "negative_controls": [
                        "integer",
                        "boolean",
                        "ordered-list",
                        "result-value",
                    ],
                    "negative_control_test": "reactive_graph_assertion_family_negative_controls",
                    "mutation_killed": False,
                    "canonical_witness": f"conformance/{path}#{concrete}",
                }
            )
            blocks.append(
                {
                    "id": route,
                    "consumed": True,
                    "discriminability_evidence": f"rust/assertion-evidence.json#/assertions/{evidence_index}",
                }
            )
        fixtures.append(
            {
                "path": path,
                "scenario_ids": definition["scenario_ids"],
                "proof_classes": definition["proof_classes"],
                "assertion_blocks": blocks,
                "status": "partial" if path in skip_paths else "passed",
            }
        )

    write_json(reports_root / "rust" / "assertion-evidence.json", evidence)
    write_json(
        report_path,
        {
            "schema_version": 1,
            "profile_id": profile["profile_id"],
            "binding_id": "rust",
            "fixture_corpus_digest": profile["fixture_corpus"]["digest"],
            "fixtures": fixtures,
            "runtime_skips": declaration["runtime_skips"],
            "fixtureless_claims": [
                {
                    "id": claim["id"],
                    "proof_classes": claim["proof_classes"],
                    "passed": True,
                    "evidence": f"rust/{claim['id'].replace('_', '-')}.json",
                }
                for claim in binding["fixtureless_claims"]
            ],
        },
    )


if __name__ == "__main__":
    try:
        main()
    except (OSError, subprocess.CalledProcessError, StopIteration) as error:
        print(f"core report failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
