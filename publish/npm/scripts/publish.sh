#!/usr/bin/env bash
# Copies built binaries from dist/ into platform-specific npm packages,
# syncs versions, then publishes all packages to npm.
#
# Usage: bash scripts/publish.sh [--dry-run]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PKG_ROOT="$(dirname "$SCRIPT_DIR")"
DIST_DIR="$(dirname "$(dirname "$PKG_ROOT")")/dist"
DRY_RUN="${1:-}"

# Read version from main package.json
VERSION=$(node -p "require('${PKG_ROOT}/package.json').version")
echo "📦 Publishing codei v${VERSION}"

# Map targets to package dirs and binary names
declare -A TARGETS=(
  ["x86_64-unknown-linux-musl"]="linux-x64-musl|codei-x86_64-unknown-linux-musl"
  ["aarch64-unknown-linux-musl"]="linux-arm64-musl|codei-aarch64-unknown-linux-musl"
  ["x86_64-apple-darwin"]="darwin-x64|codei-x86_64-apple-darwin"
  ["aarch64-apple-darwin"]="darwin-arm64|codei-aarch64-apple-darwin"
  ["x86_64-pc-windows-gnullvm"]="win32-x64|codei-x86_64-pc-windows-gnullvm.exe"
  ["aarch64-pc-windows-gnullvm"]="win32-arm64|codei-aarch64-pc-windows-gnullvm.exe"
)

NPM_CMD="npm publish"
if [[ -n "$DRY_RUN" ]]; then
  NPM_CMD="echo [dry-run] npm publish"
fi

for target in "${!TARGETS[@]}"; do
  IFS='|' read -r pkg_dir bin_name <<< "${TARGETS[$target]}"

  src="${DIST_DIR}/codei-${VERSION}-${target}"
  pkg_path="${PKG_ROOT}/packages/${pkg_dir}"
  bin_dir="${pkg_path}/bin"

  if [[ ! -f "$src" ]]; then
    echo "⚠️  Skipping ${target}: binary not found at ${src}"
    continue
  fi

  echo "🔧 Preparing ${pkg_dir}..."

  # Create bin dir and copy binary
  mkdir -p "$bin_dir"
  cp "$src" "${bin_dir}/${bin_name}"
  chmod +x "${bin_dir}/${bin_name}" 2>/dev/null || true

  # Sync version
  node -e "
    const p = require('${pkg_path}/package.json');
    p.version = '${VERSION}';
    require('fs').writeFileSync('${pkg_path}/package.json', JSON.stringify(p, null, 2) + '\n');
  "

  # Publish
  echo "📤 Publishing ${pkg_dir}..."
  (cd "$pkg_path" && eval "$NPM_CMD")
done

# Publish main package last
echo "📤 Publishing main codei package..."
# Sync version in main package.json
node -e "
  const p = require('${PKG_ROOT}/package.json');
  p.version = '${VERSION}';
  require('fs').writeFileSync('${PKG_ROOT}/package.json', JSON.stringify(p, null, 2) + '\n');
"
(cd "$PKG_ROOT" && eval "$NPM_CMD")

echo "✅ All packages published!"
