#!/usr/bin/env python3
"""Refresh the generated benchmark results section in README.md."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - Python < 3.11 fallback.
    tomllib = None


START_MARKER = "<!-- benchmark-results:start -->"
END_MARKER = "<!-- benchmark-results:end -->"
INSERT_BEFORE = "\n## Multi-Language\n"
GROUP_ORDER = {
    "cached_reads": 0,
    "cold_first_get": 1,
    "dependency_fan_out": 2,
    "memo_equality_suppression": 3,
    "effect_flushing": 4,
    "batch_storms": 5,
    "thread_safe_contention": 6,
    "profile_instrumentation": 7,
}


@dataclass(frozen=True)
class BenchmarkResult:
    group: str
    case: str
    mean_ns: float
    lower_ns: float
    upper_ns: float


def run(command: list[str]) -> None:
    print("$ " + " ".join(command), flush=True)
    subprocess.run(command, check=True)


def read_package_metadata(cargo_toml: Path) -> tuple[str, str]:
    if tomllib is not None:
        package = tomllib.loads(cargo_toml.read_text(encoding="utf-8"))["package"]
        return str(package["name"]), str(package["version"])

    in_package = False
    values: dict[str, str] = {}
    for raw_line in cargo_toml.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if line == "[package]":
            in_package = True
            continue
        if line.startswith("[") and in_package:
            break
        if in_package and "=" in line:
            key, value = line.split("=", 1)
            values[key.strip()] = value.strip().strip('"')
    return values["name"], values["version"]


def rustc_version() -> str:
    result = subprocess.run(
        ["rustc", "--version"],
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip()


def rustc_host() -> str:
    result = subprocess.run(
        ["rustc", "-vV"],
        check=True,
        capture_output=True,
        text=True,
    )
    for line in result.stdout.splitlines():
        if line.startswith("host: "):
            return line.split(":", 1)[1].strip()
    return "unknown"


def read_estimate(path: Path) -> tuple[float, float, float]:
    data = json.loads(path.read_text(encoding="utf-8"))
    mean = data["mean"]
    interval = mean["confidence_interval"]
    return (
        float(mean["point_estimate"]),
        float(interval["lower_bound"]),
        float(interval["upper_bound"]),
    )


def discover_results(criterion_dir: Path) -> list[BenchmarkResult]:
    results: list[BenchmarkResult] = []
    for estimates in criterion_dir.glob("**/new/estimates.json"):
        rel_parts = estimates.relative_to(criterion_dir).parts
        case_parts = rel_parts[:-2]
        if not case_parts:
            continue

        group = case_parts[0]
        case = " / ".join(case_parts[1:]) if len(case_parts) > 1 else group
        mean_ns, lower_ns, upper_ns = read_estimate(estimates)
        results.append(
            BenchmarkResult(
                group=group,
                case=case,
                mean_ns=mean_ns,
                lower_ns=lower_ns,
                upper_ns=upper_ns,
            )
        )

    return sorted(
        results,
        key=lambda item: (
            GROUP_ORDER.get(item.group, len(GROUP_ORDER)),
            item.group,
            natural_case_key(item.case),
        ),
    )


def natural_case_key(value: str) -> list[object]:
    parts: list[object] = []
    current = ""
    for char in value:
        if char.isdigit():
            current += char
        else:
            if current:
                parts.append(int(current))
                current = ""
            parts.append(char)
    if current:
        parts.append(int(current))
    return parts


def format_duration(ns: float) -> str:
    if ns >= 1_000_000_000:
        return f"{ns / 1_000_000_000:.3f} s"
    if ns >= 1_000_000:
        return f"{ns / 1_000_000:.3f} ms"
    if ns >= 1_000:
        return f"{ns / 1_000:.3f} us"
    return f"{ns:.3f} ns"


def build_section(package: str, version: str, results: list[BenchmarkResult]) -> str:
    lines = [
        START_MARKER,
        f"Generated for package `{package}` version `{version}`.",
        "",
        f"Environment: `{rustc_version()}` on `{rustc_host()}`.",
        "",
        "Refresh command:",
        "",
        "```bash",
        "python3 scripts/update-benchmark-results.py",
        "```",
        "",
        "Regression workflow:",
        "",
        "```bash",
        "cargo bench --features instrumentation -- --save-baseline before",
        "# apply the performance patch",
        "cargo bench --features instrumentation -- --baseline before",
        "python3 scripts/update-benchmark-results.py --no-run",
        "```",
        "",
        "Criterion estimates are local mean wall-clock time per iteration.",
        "",
        "| Group | Case | Mean | 95% CI |",
        "|---|---|---:|---:|",
    ]

    for result in results:
        lines.append(
            "| {group} | {case} | {mean} | {lower} - {upper} |".format(
                group=result.group,
                case=result.case,
                mean=format_duration(result.mean_ns),
                lower=format_duration(result.lower_ns),
                upper=format_duration(result.upper_ns),
            )
        )

    lines.extend(["", END_MARKER])
    return "\n".join(lines)


def replace_section(readme: str, section: str) -> str:
    if START_MARKER in readme and END_MARKER in readme:
        start = readme.index(START_MARKER)
        end = readme.index(END_MARKER, start) + len(END_MARKER)
        return readme[:start] + section + readme[end:]

    new_section = "\n## Benchmark Results\n\n" + section + "\n"
    if INSERT_BEFORE in readme:
        return readme.replace(INSERT_BEFORE, new_section + INSERT_BEFORE, 1)
    return readme.rstrip() + "\n" + new_section + "\n"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true", help="fail if README.md is stale")
    parser.add_argument(
        "--no-run",
        action="store_true",
        help="reuse existing target/criterion results instead of running benches",
    )
    parser.add_argument("--readme", default=Path("README.md"), type=Path)
    parser.add_argument("--cargo-toml", default=Path("Cargo.toml"), type=Path)
    parser.add_argument(
        "--criterion-dir",
        default=Path("target/criterion"),
        type=Path,
    )
    args = parser.parse_args()

    if not args.no_run:
        run(["cargo", "bench", "--features", "instrumentation"])

    results = discover_results(args.criterion_dir)
    if not results:
        print(
            f"no Criterion estimates found under {args.criterion_dir}; run without --no-run",
            file=sys.stderr,
        )
        return 2

    package, version = read_package_metadata(args.cargo_toml)
    section = build_section(package, version, results)
    current = args.readme.read_text(encoding="utf-8")
    updated = replace_section(current, section)

    if args.check:
        if current != updated:
            print(
                "README.md benchmark results are stale; run "
                "`python3 scripts/update-benchmark-results.py`",
                file=sys.stderr,
            )
            return 1
        return 0

    args.readme.write_text(updated, encoding="utf-8")
    print(f"updated {args.readme}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
