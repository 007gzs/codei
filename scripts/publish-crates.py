#!/usr/bin/env python3
"""Publish workspace crates to crates.io in dependency order.

Crate list and publish order are discovered from ``cargo metadata`` (workspace
members, topo-sorted by path dependencies). New crates under ``crates/`` are
picked up automatically when publishable.

Do NOT use bare ``cargo publish --allow-dirty`` or ``cargo publish --workspace``
for this repo: interdependent path crates plus crates.io index propagation can
trigger Cargo's "awaiting confirmation" internal error (rust-lang/cargo#17028).

Environment variables:
  WORKSPACE_VERSION     Override workspace version (default: from codei-config)
  PUBLISH_MAX_ATTEMPTS  Max retries per crate (default: 8)
  PUBLISH_INDEX_WAIT_SECS  Seconds to wait after publish/retry (default: 45)
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import time
from collections import deque


def log(message: str) -> None:
    print(f"[publish] {message}", flush=True)


def workspace_version() -> str:
    override = os.environ.get("WORKSPACE_VERSION")
    if override:
        return override
    result = subprocess.run(
        ["cargo", "pkgid", "-p", "codei-config"],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise SystemExit("failed to read workspace version from codei-config")
    match = re.search(r"#(.+)$", result.stdout.strip())
    if not match:
        raise SystemExit(f"unexpected cargo pkgid output: {result.stdout!r}")
    return match.group(1)


def discover_publish_crates() -> list[str]:
    data = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--format-version=1", "--no-deps"],
            text=True,
        )
    )
    members = set(data["workspace_members"])
    workspace = {
        pkg["name"]: pkg
        for pkg in data["packages"]
        if pkg["id"] in members and pkg.get("publish") is not False
    }

    names = set(workspace)
    deps = {name: set() for name in names}
    for name, pkg in workspace.items():
        for dep in pkg.get("dependencies", []):
            if dep["name"] in names and dep.get("kind") != "dev":
                deps[name].add(dep["name"])

    in_degree = {name: len(deps[name]) for name in names}
    dependents = {name: set() for name in names}
    for name in names:
        for dep in deps[name]:
            dependents[dep].add(name)

    queue = deque(sorted(name for name in names if in_degree[name] == 0))
    order: list[str] = []
    while queue:
        name = queue.popleft()
        order.append(name)
        for dependent in sorted(dependents[name]):
            in_degree[dependent] -= 1
            if in_degree[dependent] == 0:
                queue.append(dependent)

    if len(order) != len(names):
        raise SystemExit("cycle in workspace crate dependencies")
    return order


def already_on_crates_io(crate: str, version: str) -> bool:
    result = subprocess.run(
        ["cargo", "search", crate, "--limit", "1"],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        return False
    first_line = result.stdout.splitlines()[0] if result.stdout else ""
    return first_line.startswith(f'{crate} = "{version}"')


ALREADY_EXISTS = re.compile(
    r"already exists|found package .+ with the same version",
    re.MULTILINE,
)
TRANSIENT = re.compile(
    r"awaiting confirmation|no packages ready to publish",
    re.MULTILINE,
)
DEP_NOT_VISIBLE = re.compile(
    r"no matching package named|failed to select a version",
    re.MULTILINE,
)


def publish_crate(crate: str, version: str, max_attempts: int, index_wait_secs: int) -> None:
    if already_on_crates_io(crate, version):
        log(f"skip {crate} {version} (already on crates.io)")
        return

    for attempt in range(1, max_attempts + 1):
        log(f"upload {crate} {version} (attempt {attempt}/{max_attempts})")
        result = subprocess.run(
            ["cargo", "publish", "-p", crate, "--allow-dirty"],
            check=False,
            capture_output=True,
            text=True,
        )
        output = result.stdout + result.stderr
        if output:
            print(output, end="", flush=True)

        if result.returncode == 0:
            log(f"waiting {index_wait_secs}s for crates.io index ({crate})")
            time.sleep(index_wait_secs)
            return

        if ALREADY_EXISTS.search(output):
            log(f"skip {crate} {version} (version already exists)")
            return

        if TRANSIENT.search(output) or DEP_NOT_VISIBLE.search(output):
            reason = (
                "transient cargo workspace publish error"
                if TRANSIENT.search(output)
                else "dependency not yet visible on crates.io"
            )
            log(f"{reason}; retrying after {index_wait_secs}s")
            time.sleep(index_wait_secs)
            continue

        log(f"fatal publish error for {crate}")
        raise SystemExit(result.returncode)

    log(f"gave up publishing {crate} after {max_attempts} attempts")
    raise SystemExit(1)


def main() -> None:
    version = workspace_version()
    max_attempts = int(os.environ.get("PUBLISH_MAX_ATTEMPTS", "8"))
    index_wait_secs = int(os.environ.get("PUBLISH_INDEX_WAIT_SECS", "45"))
    crates = discover_publish_crates()

    log(f"workspace version {version}")
    log(f"publish order: {' '.join(crates)}")
    for crate in crates:
        publish_crate(crate, version, max_attempts, index_wait_secs)
    log("done")


if __name__ == "__main__":
    main()
