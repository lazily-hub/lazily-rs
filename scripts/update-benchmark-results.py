#!/usr/bin/env python3
"""Refresh the generated benchmark results section in README.md."""

from __future__ import annotations

import argparse
import csv
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
DEFAULT_PROFILE_OUTPUT = Path("target/lazily-instrumentation-profile.csv")
GROUP_ORDER = {
    "cached_reads": 0,
    "cold_first_get": 1,
    "dependency_fan_out": 2,
    "set_cell_invalidation": 3,
    "memo_equality_suppression": 4,
    "effect_flushing": 5,
    "batch_storms": 6,
    "thread_safe_contention": 7,
    "profile_instrumentation": 8,
}
SET_CELL_INVALIDATION_CASE_ORDER = {
    "high_fan_out": 0,
    "same_slot_contention": 1,
    "independent_slot_contention": 2,
    "batched_write_bursts": 3,
}
THREAD_SAFE_CONTENTION_CASE_ORDER = {
    "same_slot_write_read": 0,
    "independent_slots": 1,
    "read_mostly_waiters": 2,
    "batched_write_bursts": 3,
}


@dataclass(frozen=True)
class BenchmarkResult:
    group: str
    case: str
    mean_ns: float
    lower_ns: float
    upper_ns: float


@dataclass(frozen=True)
class InstrumentationProfile:
    profile: str
    node_allocations: int
    slot_recomputes: int
    duplicate_speculative_recomputes: int
    dependency_edges_added: int
    dependency_edges_removed: int
    effect_queue_pushes: int
    max_effect_queue_depth: int
    lock_acquisitions: int
    lock_wait_nanos: int
    lock_hold_nanos: int
    lock_attribution: tuple["LockAttribution", ...]


@dataclass(frozen=True)
class LockAttribution:
    site: str
    lock_acquisitions: int
    lock_wait_nanos: int
    lock_hold_nanos: int


@dataclass(frozen=True)
class LockAttributionBudget:
    site: str
    max_lock_acquisitions: int


@dataclass(frozen=True)
class InstrumentationBudget:
    profile: str
    max_lock_acquisitions: int
    site_budgets: tuple[LockAttributionBudget, ...] = ()


REGRESSION_BUDGETS: tuple[InstrumentationBudget, ...] = (
    InstrumentationBudget(
        "thread_safe_set_cell_invalidation_independent_slot_contention_16",
        max_lock_acquisitions=192,
        site_budgets=(
            LockAttributionBudget("set_cell_invalidation", 0),
            LockAttributionBudget("dependency_edge", 16),
            LockAttributionBudget("get_refresh", 32),
            LockAttributionBudget("publish", 32),
        ),
    ),
    InstrumentationBudget(
        "thread_safe_set_cell_invalidation_batched_write_bursts_16",
        max_lock_acquisitions=900,
        site_budgets=(
            LockAttributionBudget("other", 800),
            LockAttributionBudget("set_cell_invalidation", 16),
            LockAttributionBudget("dependency_edge", 64),
            LockAttributionBudget("get_refresh", 2),
            LockAttributionBudget("publish", 2),
        ),
    ),
    InstrumentationBudget(
        "thread_safe_contention_same_slot_write_read_16",
        max_lock_acquisitions=1_600,
        site_budgets=(
            LockAttributionBudget("get_refresh", 700),
            LockAttributionBudget("publish", 450),
            LockAttributionBudget("in_flight_wait", 500),
            LockAttributionBudget("set_cell_invalidation", 32),
        ),
    ),
    InstrumentationBudget(
        "thread_safe_contention_independent_slots_16",
        max_lock_acquisitions=1_500,
        site_budgets=(
            LockAttributionBudget("other", 160),
            LockAttributionBudget("get_refresh", 700),
            LockAttributionBudget("publish", 700),
            LockAttributionBudget("dependency_edge", 16),
            LockAttributionBudget("set_cell_invalidation", 64),
        ),
    ),
    InstrumentationBudget(
        "thread_safe_contention_read_mostly_waiters_16",
        max_lock_acquisitions=256,
        site_budgets=(
            LockAttributionBudget("get_refresh", 128),
            LockAttributionBudget("publish", 64),
            LockAttributionBudget("in_flight_wait", 64),
        ),
    ),
    InstrumentationBudget(
        "thread_safe_contention_batched_write_bursts_16",
        max_lock_acquisitions=950,
        site_budgets=(
            LockAttributionBudget("other", 800),
            LockAttributionBudget("get_refresh", 128),
            LockAttributionBudget("dependency_edge", 64),
            LockAttributionBudget("set_cell_invalidation", 16),
            LockAttributionBudget("publish", 64),
            LockAttributionBudget("in_flight_wait", 64),
        ),
    ),
    InstrumentationBudget(
        "thread_safe_effect_contention_queue_coalescing_16",
        max_lock_acquisitions=1_800,
        site_budgets=(
            LockAttributionBudget("other", 900),
            LockAttributionBudget("dependency_edge", 900),
            LockAttributionBudget("set_cell_invalidation", 16),
            LockAttributionBudget("get_refresh", 0),
            LockAttributionBudget("publish", 0),
        ),
    ),
    InstrumentationBudget(
        "thread_safe_effect_contention_cleanup_execution_16",
        max_lock_acquisitions=1_300,
        site_budgets=(
            LockAttributionBudget("other", 400),
            LockAttributionBudget("dependency_edge", 700),
            LockAttributionBudget("set_cell_invalidation", 220),
            LockAttributionBudget("get_refresh", 0),
            LockAttributionBudget("publish", 0),
        ),
    ),
    InstrumentationBudget(
        "thread_safe_effect_contention_batch_flush_16",
        max_lock_acquisitions=1_500,
        site_budgets=(
            LockAttributionBudget("other", 1_300),
            LockAttributionBudget("get_refresh", 8),
            LockAttributionBudget("dependency_edge", 96),
            LockAttributionBudget("set_cell_invalidation", 16),
            LockAttributionBudget("publish", 8),
        ),
    ),
)


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
        if group == "thread_safe_contention" and case.isdigit():
            continue
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
            benchmark_case_key(item),
        ),
    )


def run_instrumentation_profile(output: Path) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    command = [
        "cargo",
        "run",
        "--example",
        "instrumentation_profile",
        "--features",
        "instrumentation",
        "--quiet",
    ]
    print("$ " + " ".join(command), flush=True)
    result = subprocess.run(command, check=True, capture_output=True, text=True)
    output.write_text(result.stdout, encoding="utf-8")


def read_instrumentation_profiles(path: Path) -> list[InstrumentationProfile]:
    rows: list[InstrumentationProfile] = []
    with path.open(encoding="utf-8", newline="") as handle:
        for row in csv.DictReader(handle):
            rows.append(
                InstrumentationProfile(
                    profile=row["profile"],
                    node_allocations=int(row["node_allocations"]),
                    slot_recomputes=int(row["slot_recomputes"]),
                    duplicate_speculative_recomputes=int(
                        row["duplicate_speculative_recomputes"]
                    ),
                    dependency_edges_added=int(row["dependency_edges_added"]),
                    dependency_edges_removed=int(row["dependency_edges_removed"]),
                    effect_queue_pushes=int(row["effect_queue_pushes"]),
                    max_effect_queue_depth=int(row["max_effect_queue_depth"]),
                    lock_acquisitions=int(row["lock_acquisitions"]),
                    lock_wait_nanos=int(row["lock_wait_nanos"]),
                    lock_hold_nanos=int(row["lock_hold_nanos"]),
                    lock_attribution=parse_lock_attribution(
                        row.get("lock_attribution", "")
                    ),
                )
            )
    return rows


def parse_lock_attribution(value: str) -> tuple[LockAttribution, ...]:
    if not value:
        return ()

    sites: list[LockAttribution] = []
    for item in value.split("|"):
        site, counters = item.split("=", 1)
        acquisitions, wait_nanos, hold_nanos = counters.split(":", 2)
        sites.append(
            LockAttribution(
                site=site,
                lock_acquisitions=int(acquisitions),
                lock_wait_nanos=int(wait_nanos),
                lock_hold_nanos=int(hold_nanos),
            )
        )
    return tuple(sites)


def lock_attribution_by_site(profile: InstrumentationProfile) -> dict[str, int]:
    return {
        attribution.site: attribution.lock_acquisitions
        for attribution in profile.lock_attribution
    }


def regression_budget_failures(
    profiles: list[InstrumentationProfile],
) -> list[str]:
    by_profile = {profile.profile: profile for profile in profiles}
    failures: list[str] = []

    for budget in REGRESSION_BUDGETS:
        profile = by_profile.get(budget.profile)
        if profile is None:
            failures.append(f"{budget.profile}: missing instrumentation profile")
            continue

        if profile.lock_acquisitions > budget.max_lock_acquisitions:
            failures.append(
                "{profile}: lock_acquisitions {actual} > budget {budget}".format(
                    profile=budget.profile,
                    actual=profile.lock_acquisitions,
                    budget=budget.max_lock_acquisitions,
                )
            )

        by_site = lock_attribution_by_site(profile)
        for site_budget in budget.site_budgets:
            actual = by_site.get(site_budget.site, 0)
            if actual > site_budget.max_lock_acquisitions:
                failures.append(
                    "{profile}: {site} lock_acquisitions {actual} > budget {budget}".format(
                        profile=budget.profile,
                        site=site_budget.site,
                        actual=actual,
                        budget=site_budget.max_lock_acquisitions,
                    )
                )

    return failures


def natural_case_key(value: str) -> list[tuple[int, object]]:
    parts: list[tuple[int, object]] = []
    current = ""
    for char in value:
        if char.isdigit():
            current += char
        else:
            if current:
                parts.append((0, int(current)))
                current = ""
            parts.append((1, char))
    if current:
        parts.append((0, int(current)))
    return parts


def benchmark_case_key(result: BenchmarkResult) -> tuple[int, list[tuple[int, object]]]:
    if result.group == "set_cell_invalidation":
        case_name, _, worker = result.case.partition(" / ")
        return (
            SET_CELL_INVALIDATION_CASE_ORDER.get(
                case_name, len(SET_CELL_INVALIDATION_CASE_ORDER)
            ),
            natural_case_key(worker or result.case),
        )

    if result.group == "thread_safe_contention":
        case_name, _, worker = result.case.partition(" / ")
        return (
            THREAD_SAFE_CONTENTION_CASE_ORDER.get(
                case_name, len(THREAD_SAFE_CONTENTION_CASE_ORDER)
            ),
            natural_case_key(worker or result.case),
        )

    return (0, natural_case_key(result.case))


def format_duration(ns: float) -> str:
    if ns >= 1_000_000_000:
        return f"{ns / 1_000_000_000:.3f} s"
    if ns >= 1_000_000:
        return f"{ns / 1_000_000:.3f} ms"
    if ns >= 1_000:
        return f"{ns / 1_000:.3f} us"
    return f"{ns:.3f} ns"


def build_section(
    package: str,
    version: str,
    results: list[BenchmarkResult],
    profiles: list[InstrumentationProfile],
) -> str:
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
        "Regression budgets enforced by `python3 scripts/update-benchmark-results.py --check`:",
        "",
        "| Profile | Max lock acquisitions | Site lock budgets |",
        "|---|---:|---|",
    ]

    for budget in REGRESSION_BUDGETS:
        site_budgets = ", ".join(
            f"{site.site}<={site.max_lock_acquisitions}"
            for site in budget.site_budgets
        )
        lines.append(
            "| {profile} | {max_locks} | {site_budgets} |".format(
                profile=budget.profile,
                max_locks=budget.max_lock_acquisitions,
                site_budgets=site_budgets or "-",
            )
        )

    lines.extend(
        [
            "",
            "Budgets use deterministic lock acquisition counts instead of elapsed wait/hold time.",
            "",
        ]
    )

    lines.extend(
        [
            "Criterion estimates are local mean wall-clock time per iteration.",
            "",
            "| Group | Case | Mean | 95% CI |",
            "|---|---|---:|---:|",
        ]
    )

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

    lines.extend(
        [
            "",
            "Instrumentation snapshots are single local profile runs captured by",
            "`examples/instrumentation_profile.rs`.",
            "",
            "| Profile | Alloc | Recomputes | Duplicate recomputes | Edges + | Edges - | Effect pushes | Max queue | Lock acquisitions | Lock wait | Lock hold |",
            "|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
        ]
    )

    for profile in profiles:
        lines.append(
            "| {profile} | {alloc} | {recomputes} | {duplicates} | {edges_added} | "
            "{edges_removed} | {effect_pushes} | {max_queue} | {locks} | "
            "{lock_wait} | {lock_hold} |".format(
                profile=profile.profile,
                alloc=profile.node_allocations,
                recomputes=profile.slot_recomputes,
                duplicates=profile.duplicate_speculative_recomputes,
                edges_added=profile.dependency_edges_added,
                edges_removed=profile.dependency_edges_removed,
                effect_pushes=profile.effect_queue_pushes,
                max_queue=profile.max_effect_queue_depth,
                locks=profile.lock_acquisitions,
                lock_wait=format_duration(profile.lock_wait_nanos),
                lock_hold=format_duration(profile.lock_hold_nanos),
            )
        )

    attribution_rows = [
        (profile, attribution)
        for profile in profiles
        if profile.profile.startswith("thread_safe_contention_")
        or profile.profile.startswith("thread_safe_set_cell_invalidation_")
        or profile.profile.startswith("thread_safe_effect_contention_")
        for attribution in profile.lock_attribution
        if attribution.lock_acquisitions > 0
    ]
    if attribution_rows:
        lines.extend(
            [
                "",
                "ThreadSafe lock attribution for contention profiles:",
                "",
                "| Profile | Site | Lock acquisitions | Lock wait | Lock hold |",
                "|---|---|---:|---:|---:|",
            ]
        )
        for profile, attribution in attribution_rows:
            lines.append(
                "| {profile} | {site} | {locks} | {lock_wait} | {lock_hold} |".format(
                    profile=profile.profile,
                    site=attribution.site,
                    locks=attribution.lock_acquisitions,
                    lock_wait=format_duration(attribution.lock_wait_nanos),
                    lock_hold=format_duration(attribution.lock_hold_nanos),
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
    parser.add_argument(
        "--profile-output",
        default=DEFAULT_PROFILE_OUTPUT,
        type=Path,
        help="CSV path for instrumentation profile snapshots",
    )
    args = parser.parse_args()

    if args.check:
        pass
    elif not args.no_run:
        run(["cargo", "bench", "--features", "instrumentation"])
        run_instrumentation_profile(args.profile_output)
    else:
        run_instrumentation_profile(args.profile_output)

    results = discover_results(args.criterion_dir)
    if not results:
        print(
            f"no Criterion estimates found under {args.criterion_dir}; run without --no-run",
            file=sys.stderr,
        )
        return 2
    if not args.profile_output.exists():
        print(
            f"no instrumentation profile found at {args.profile_output}; run without --check",
            file=sys.stderr,
        )
        return 2
    profiles = read_instrumentation_profiles(args.profile_output)
    if not profiles:
        print(
            f"no instrumentation profile rows found in {args.profile_output}",
            file=sys.stderr,
        )
        return 2
    budget_failures = regression_budget_failures(profiles)
    if budget_failures:
        print("instrumentation regression budget failure(s):", file=sys.stderr)
        for failure in budget_failures:
            print(f"- {failure}", file=sys.stderr)
        return 1

    package, version = read_package_metadata(args.cargo_toml)
    section = build_section(package, version, results, profiles)
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
