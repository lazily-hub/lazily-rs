#!/usr/bin/env python3
"""Generate or verify the reviewed all-features dependency inventory."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
import tomllib
from collections import defaultdict, deque
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parent.parent
LOCK_PATH = ROOT / "Cargo.lock"
INVENTORY_PATH = ROOT / "docs" / "dependency-license-inventory.json"
PROVENANCE_PATH = ROOT / "docs" / "durable-runtime-provenance.md"
METADATA_COMMAND = [
    "cargo",
    "metadata",
    "--locked",
    "--all-features",
    "--format-version",
    "1",
]

# Each declaration was reviewed as a complete expression. `selected_spdx` names
# the permissive branch used for this repository when the declaration offers a
# choice. Legacy slash spellings are retained verbatim in the inventory and
# normalized here rather than silently treated as SPDX syntax.
LICENSE_REVIEWS: dict[str, dict[str, str]] = {
    "(MIT OR Apache-2.0) AND Unicode-3.0": {
        "selected_spdx": "MIT AND Unicode-3.0",
        "review": "Both conjunctive obligations apply; select MIT for the alternative branch.",
    },
    "Apache-2.0": {
        "selected_spdx": "Apache-2.0",
        "review": "Approved permissive license.",
    },
    "Apache-2.0 / MIT": {
        "selected_spdx": "Apache-2.0",
        "review": "Legacy dual-license spelling; select Apache-2.0.",
    },
    "Apache-2.0 AND ISC": {
        "selected_spdx": "Apache-2.0 AND ISC",
        "review": "Both permissive-license obligations apply.",
    },
    "Apache-2.0 OR BSL-1.0 OR MIT": {
        "selected_spdx": "Apache-2.0",
        "review": "Select Apache-2.0; BSL-1.0 is not selected.",
    },
    "Apache-2.0 OR GPL-2.0-only": {
        "selected_spdx": "Apache-2.0",
        "review": "Select Apache-2.0; GPL-2.0-only is not selected.",
    },
    "Apache-2.0 OR ISC OR MIT": {
        "selected_spdx": "Apache-2.0",
        "review": "Select Apache-2.0.",
    },
    "Apache-2.0 OR MIT": {
        "selected_spdx": "Apache-2.0",
        "review": "Select Apache-2.0.",
    },
    "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT": {
        "selected_spdx": "Apache-2.0",
        "review": "Select plain Apache-2.0.",
    },
    "Apache-2.0/MIT": {
        "selected_spdx": "Apache-2.0",
        "review": "Legacy dual-license spelling; select Apache-2.0.",
    },
    "BSD-2-Clause OR Apache-2.0 OR MIT": {
        "selected_spdx": "Apache-2.0",
        "review": "Select Apache-2.0.",
    },
    "BSD-3-Clause": {
        "selected_spdx": "BSD-3-Clause",
        "review": "Approved permissive license.",
    },
    "CC0-1.0 OR MIT-0 OR Apache-2.0": {
        "selected_spdx": "Apache-2.0",
        "review": "Select Apache-2.0.",
    },
    "CDLA-Permissive-2.0": {
        "selected_spdx": "CDLA-Permissive-2.0",
        "review": "Approved permissive data license for the packaged root certificates.",
    },
    "ISC": {
        "selected_spdx": "ISC",
        "review": "Approved permissive license.",
    },
    "ISC AND (Apache-2.0 OR ISC)": {
        "selected_spdx": "ISC",
        "review": "Select ISC in the alternative branch; the resulting obligation is ISC.",
    },
    "ISC AND (Apache-2.0 OR ISC) AND Apache-2.0 AND MIT AND BSD-3-Clause AND (Apache-2.0 OR ISC OR MIT) AND (Apache-2.0 OR ISC OR MIT-0)": {
        "selected_spdx": "Apache-2.0 AND BSD-3-Clause AND ISC AND MIT",
        "review": "The bundled components require all four permissive-license obligations.",
    },
    "MIT": {
        "selected_spdx": "MIT",
        "review": "Approved permissive license.",
    },
    "MIT OR Apache-2.0": {
        "selected_spdx": "Apache-2.0",
        "review": "Select Apache-2.0.",
    },
    "MIT OR Apache-2.0 OR BSD-1-Clause": {
        "selected_spdx": "Apache-2.0",
        "review": "Select Apache-2.0.",
    },
    "MIT OR Apache-2.0 OR LGPL-2.1-or-later": {
        "selected_spdx": "Apache-2.0",
        "review": "Select Apache-2.0; LGPL-2.1-or-later is not selected.",
    },
    "MIT-0": {
        "selected_spdx": "MIT-0",
        "review": "Approved permissive license.",
    },
    "MIT/Apache-2.0": {
        "selected_spdx": "Apache-2.0",
        "review": "Legacy dual-license spelling; select Apache-2.0.",
    },
    "Unicode-3.0": {
        "selected_spdx": "Unicode-3.0",
        "review": "Approved permissive Unicode data/software license.",
    },
    "Unlicense OR MIT": {
        "selected_spdx": "MIT",
        "review": "Select MIT; Unlicense is not selected.",
    },
    "Unlicense/MIT": {
        "selected_spdx": "MIT",
        "review": "Legacy dual-license spelling; select MIT.",
    },
    "Zlib": {
        "selected_spdx": "Zlib",
        "review": "Approved permissive license.",
    },
    "Zlib OR Apache-2.0 OR MIT": {
        "selected_spdx": "Apache-2.0",
        "review": "Select Apache-2.0.",
    },
}

REQUIRED_PROVENANCE_HEADINGS = (
    "# Durable runtime implementation provenance",
    "## Official public sources",
    "## Independent authorship",
    "## Selected dependencies",
    "## Synthetic fixtures",
    "## Excluded inputs",
)
REQUIRED_PUBLIC_SOURCE_URLS = (
    "https://www.postgresql.org/docs/18/",
    "https://docs.nats.io/nats-concepts/jetstream",
    "https://github.com/sfackler/rust-postgres",
    "https://github.com/nats-io/nats.rs",
)
DURABLE_SOURCE_PATTERNS = (
    "src/*durable*.rs",
    "tests/*durable*.rs",
    "docs/durable-*.md",
    "scripts/test-durable-*.sh",
)


class InventoryError(RuntimeError):
    """A fail-closed inventory or provenance violation."""


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def relative_file_hashes(directory: Path, recursive: bool) -> tuple[list[dict[str, str]], list[dict[str, str]]]:
    if not directory.is_dir():
        raise InventoryError(f"package source directory is unavailable: {directory}")
    candidates = directory.rglob("*") if recursive else directory.iterdir()
    license_files: list[dict[str, str]] = []
    notice_files: list[dict[str, str]] = []
    for path in candidates:
        if not path.is_file() or any(part in {".git", "target"} for part in path.parts):
            continue
        upper = path.name.upper()
        record = {
            "path": path.relative_to(directory).as_posix(),
            "sha256": sha256_bytes(path.read_bytes()),
        }
        if upper.startswith(("LICENSE", "LICENCE", "COPYING", "COPYRIGHT")):
            license_files.append(record)
        elif upper.startswith("NOTICE"):
            notice_files.append(record)
    key = lambda item: item["path"]
    return sorted(license_files, key=key), sorted(notice_files, key=key)


def load_metadata() -> dict[str, Any]:
    process = subprocess.run(
        METADATA_COMMAND,
        cwd=ROOT,
        check=False,
        stdout=subprocess.PIPE,
        stderr=sys.stderr,
        text=True,
    )
    if process.returncode != 0:
        raise InventoryError(f"cargo metadata failed with exit {process.returncode}")
    return json.loads(process.stdout)


def load_lock_checksums() -> dict[tuple[str, str, str | None], str | None]:
    with LOCK_PATH.open("rb") as handle:
        lock = tomllib.load(handle)
    return {
        (package["name"], package["version"], package.get("source")): package.get("checksum")
        for package in lock["package"]
    }


def dependency_roles(metadata: dict[str, Any]) -> dict[str, list[str]]:
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    workspace = set(metadata["workspace_members"])
    roles: dict[str, set[str]] = defaultdict(set)
    pending: deque[tuple[str, str]] = deque((package_id, "runtime") for package_id in sorted(workspace))
    while pending:
        package_id, role = pending.popleft()
        if role in roles[package_id]:
            continue
        roles[package_id].add(role)
        for dependency in nodes[package_id]["deps"]:
            for dep_kind in dependency["dep_kinds"]:
                kind = dep_kind["kind"] or "normal"
                if kind == "dev" and package_id not in workspace:
                    continue
                child_role = "build" if kind == "build" else "dev" if kind == "dev" else role
                pending.append((dependency["pkg"], child_role))
    unresolved = sorted(set(nodes) - set(roles))
    if unresolved:
        raise InventoryError(f"resolved packages lack runtime/build/dev reachability: {unresolved}")
    order = {"runtime": 0, "build": 1, "dev": 2}
    return {package_id: sorted(found, key=order.__getitem__) for package_id, found in roles.items()}


def source_record(package: dict[str, Any], workspace: set[str]) -> tuple[str, str, str]:
    source = package["source"]
    if source is None:
        if package["id"] not in workspace:
            raise InventoryError(f"unreviewed path dependency outside the workspace: {package['id']}")
        repository = package.get("repository")
        if not repository:
            raise InventoryError(f"workspace package lacks repository provenance: {package['id']}")
        return "workspace", repository, "workspace-authored"
    if source.startswith("registry+"):
        return source, f"https://crates.io/crates/{package['name']}/{package['version']}", "registry-checksum"
    if source.startswith("git+"):
        url = source.removeprefix("git+").split("?", 1)[0].split("#", 1)[0]
        return source, url, "git-revision"
    raise InventoryError(f"unsupported dependency source for {package['id']}: {source}")


def durable_source_digest() -> tuple[str, list[str]]:
    paths = sorted(
        {
            path
            for pattern in DURABLE_SOURCE_PATTERNS
            for path in ROOT.glob(pattern)
            if path != PROVENANCE_PATH
        }
    )
    if not paths:
        raise InventoryError("durable implementation source set is empty")
    digest = hashlib.sha256()
    names: list[str] = []
    for path in paths:
        name = path.relative_to(ROOT).as_posix()
        names.append(name)
        digest.update(name.encode())
        digest.update(b"\0")
        digest.update(path.read_bytes())
        digest.update(b"\0")
    return digest.hexdigest(), names


def verify_provenance(metadata: dict[str, Any]) -> tuple[str, str, list[str]]:
    if not PROVENANCE_PATH.is_file():
        raise InventoryError(f"missing provenance note: {PROVENANCE_PATH.relative_to(ROOT)}")
    text = PROVENANCE_PATH.read_text()
    for heading in REQUIRED_PROVENANCE_HEADINGS:
        if heading not in text:
            raise InventoryError(f"provenance note is missing heading: {heading}")
    for url in REQUIRED_PUBLIC_SOURCE_URLS:
        if url not in text:
            raise InventoryError(f"provenance note is missing official source URL: {url}")
    for phrase in ("independently authored", "synthetic", "employer-owned", "customer-specific"):
        if phrase not in text:
            raise InventoryError(f"provenance note is missing required exclusion/authorship phrase: {phrase}")
    versions = {package["name"]: package["version"] for package in metadata["packages"]}
    for dependency in ("postgres", "async-nats"):
        version = versions.get(dependency)
        if version is None or f"`{dependency} {version}`" not in text:
            raise InventoryError(f"provenance note does not name resolved {dependency} version {version}")
    source_digest, source_paths = durable_source_digest()
    match = re.search(r"^Implementation source digest: `sha256:([0-9a-f]{64})`$", text, re.MULTILINE)
    if match is None:
        raise InventoryError("provenance note lacks the implementation source digest")
    if match.group(1) != source_digest:
        raise InventoryError(
            "durable implementation changed without a matching provenance-note review: "
            f"expected sha256:{source_digest}, found sha256:{match.group(1)}"
        )
    return sha256_bytes(text.encode()), source_digest, source_paths


def build_inventory(metadata: dict[str, Any]) -> dict[str, Any]:
    packages = {package["id"]: package for package in metadata["packages"]}
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    workspace = set(metadata["workspace_members"])
    roles = dependency_roles(metadata)
    checksums = load_lock_checksums()
    provenance_hash, source_digest, source_paths = verify_provenance(metadata)
    rows: list[dict[str, Any]] = []
    used_expressions: set[str] = set()
    for package_id in sorted(packages, key=lambda item: (packages[item]["name"], packages[item]["version"], item)):
        package = packages[package_id]
        expression = package.get("license")
        if not expression:
            raise InventoryError(f"package has no declared license: {package_id}")
        review = LICENSE_REVIEWS.get(expression)
        if review is None:
            raise InventoryError(f"package has an unreviewed license expression: {package_id}: {expression}")
        used_expressions.add(expression)
        source, source_url, provenance = source_record(package, workspace)
        lock_source = None if package["source"] is None else package["source"]
        lock_key = (package["name"], package["version"], lock_source)
        if lock_key not in checksums:
            raise InventoryError(f"package is absent from Cargo.lock: {package_id}")
        manifest_dir = Path(package["manifest_path"]).parent
        license_files, notice_files = relative_file_hashes(manifest_dir, recursive=package["source"] is not None)
        if notice_files:
            notice_disposition = "preserve-listed-upstream-notices"
        elif license_files:
            notice_disposition = "no-upstream-notice-file-found; preserve-listed-license-files"
        else:
            notice_disposition = "no-upstream-notice-or-license-file-found; declaration-and-source-reviewed"
        rows.append(
            {
                "name": package["name"],
                "version": package["version"],
                "dependency_kinds": roles[package_id],
                "enabled_features": sorted(nodes[package_id]["features"]),
                "declared_license": expression,
                "selected_spdx": review["selected_spdx"],
                "license_review": "reviewed",
                "source": source,
                "source_url": source_url,
                "source_provenance": provenance,
                "checksum": checksums[lock_key],
                "license_files": license_files,
                "notice_files": notice_files,
                "notice_disposition": notice_disposition,
            }
        )
    unused_reviews = sorted(set(LICENSE_REVIEWS) - used_expressions)
    if unused_reviews:
        raise InventoryError(f"stale license reviews are no longer used: {unused_reviews}")
    return {
        "schema_version": 1,
        "generated_by": "scripts/dependency-inventory.py",
        "metadata_command": " ".join(METADATA_COMMAND),
        "cargo_lock_sha256": sha256_bytes(LOCK_PATH.read_bytes()),
        "package_count": len(rows),
        "license_expression_count": len(used_expressions),
        "license_reviews": {key: LICENSE_REVIEWS[key] for key in sorted(LICENSE_REVIEWS)},
        "provenance": {
            "note": PROVENANCE_PATH.relative_to(ROOT).as_posix(),
            "note_sha256": provenance_hash,
            "implementation_source_sha256": source_digest,
            "implementation_paths": source_paths,
        },
        "packages": rows,
    }


def render(inventory: dict[str, Any]) -> str:
    return json.dumps(inventory, indent=2, sort_keys=True) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true", help="verify the committed inventory and provenance note")
    mode.add_argument("--write", action="store_true", help="write the inventory after an explicit current-graph review")
    mode.add_argument("--print-source-digest", action="store_true", help="print the durable implementation digest")
    parser.add_argument(
        "--accept-current-review",
        action="store_true",
        help="required with --write; acknowledges review of every current package/license/notice row",
    )
    args = parser.parse_args()
    try:
        if args.print_source_digest:
            digest, _ = durable_source_digest()
            print(f"sha256:{digest}")
            return 0
        if args.write and not args.accept_current_review:
            raise InventoryError("--write requires --accept-current-review")
        expected = render(build_inventory(load_metadata()))
        if args.write:
            INVENTORY_PATH.write_text(expected)
            print(f"wrote {INVENTORY_PATH.relative_to(ROOT)}")
            return 0
        if not INVENTORY_PATH.is_file():
            raise InventoryError(f"missing committed inventory: {INVENTORY_PATH.relative_to(ROOT)}")
        actual = INVENTORY_PATH.read_text()
        if actual != expected:
            raise InventoryError(
                "dependency inventory drifted; review the changed graph, licenses, source URLs, and notices, then run "
                "scripts/dependency-inventory.py --write --accept-current-review"
            )
        inventory = json.loads(actual)
        print(
            "dependency inventory ok: "
            f"{inventory['package_count']} packages, "
            f"{inventory['license_expression_count']} reviewed license expressions"
        )
        return 0
    except (InventoryError, OSError, subprocess.SubprocessError, json.JSONDecodeError) as error:
        print(f"dependency inventory error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
