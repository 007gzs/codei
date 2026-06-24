"""CodeI ai-coding agent."""

__version__ = "0.0.8"

import os
import platform
import subprocess
import sys
from pathlib import Path


def _get_binary_path() -> str:
    """Find the path to the platform-specific binary."""
    pkg_dir = Path(__file__).parent / "bin"

    plat = platform.system().lower()
    arch = platform.machine().lower()

    # Normalize arch names
    arch_map = {
        "x86_64": "x86_64",
        "amd64": "x86_64",
        "aarch64": "aarch64",
        "arm64": "aarch64",
    }
    normalized_arch = arch_map.get(arch, arch)

    # Map to dist binary naming
    binary_map = {
        ("linux", "x86_64"): "codei-x86_64-unknown-linux-musl",
        ("linux", "aarch64"): "codei-aarch64-unknown-linux-musl",
        ("darwin", "x86_64"): "codei-x86_64-apple-darwin",
        ("darwin", "aarch64"): "codei-aarch64-apple-darwin",
        ("windows", "x86_64"): "codei-x86_64-pc-windows-gnullvm.exe",
        ("windows", "aarch64"): "codei-aarch64-pc-windows-gnullvm.exe",
    }

    key = (plat, normalized_arch)
    binary_name = binary_map.get(key)

    if binary_name is None:
        print(
            f"Unsupported platform: {platform.system()} {platform.machine()}\n"
            f"codei supports: linux/darwin/windows on x86_64/aarch64",
            file=sys.stderr,
        )
        sys.exit(1)

    binary_path = pkg_dir / binary_name

    if not binary_path.exists():
        print(
            f"codei binary not found for {platform.system()} {platform.machine()}.\n"
            f"Expected: {binary_path}",
            file=sys.stderr,
        )
        sys.exit(1)

    return str(binary_path)


def main() -> None:
    """Entry point: exec the bundled binary."""
    binary_path = _get_binary_path()

    # Replace the Python process with the codei binary
    try:
        os.execv(binary_path, [binary_path] + sys.argv[1:])
    except OSError as e:
        print(f"Failed to execute codei: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
