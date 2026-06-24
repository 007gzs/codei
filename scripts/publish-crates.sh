#!/usr/bin/env bash
# Publish workspace crates to crates.io in dependency order.
#
# Do NOT use bare `cargo publish --allow-dirty` or `cargo publish --workspace` for
# this repo: interdependent path crates plus crates.io index propagation can
# trigger Cargo's "awaiting confirmation" internal error (rust-lang/cargo#17028).
set -euo pipefail

CRATES=(
  codei-config
  codei-i18n
  codei-llm
  codei-mcp
  codei-session
  codei-tools
  codei-commands
  codei-agent
  codei-tui
  codei-sdk
  codei
)

VERSION="${WORKSPACE_VERSION:-$(cargo pkgid -p codei-config 2>/dev/null | sed 's/.*#//')}"
MAX_ATTEMPTS="${PUBLISH_MAX_ATTEMPTS:-8}"
INDEX_WAIT_SECS="${PUBLISH_INDEX_WAIT_SECS:-45}"

log() { printf '[publish] %s\n' "$*"; }

already_on_crates_io() {
  local crate=$1
  local out
  out=$(cargo search "${crate}" --limit 1 2>/dev/null || true)
  [[ "${out}" == "${crate} = \"${VERSION}\""* ]]
}

publish_crate() {
  local crate=$1
  local attempt=1
  local output

  if already_on_crates_io "${crate}"; then
    log "skip ${crate} ${VERSION} (already on crates.io)"
    return 0
  fi

  while (( attempt <= MAX_ATTEMPTS )); do
    log "upload ${crate} ${VERSION} (attempt ${attempt}/${MAX_ATTEMPTS})"
    set +e
    output=$(cargo publish -p "${crate}" --allow-dirty 2>&1)
    local status=$?
    set -e
    printf '%s\n' "${output}"

    if (( status == 0 )); then
      log "waiting ${INDEX_WAIT_SECS}s for crates.io index (${crate})"
      sleep "${INDEX_WAIT_SECS}"
      return 0
    fi

    if grep -qE 'already exists|found package .+ with the same version' <<<"${output}"; then
      log "skip ${crate} ${VERSION} (version already exists)"
      return 0
    fi

    if grep -qE 'awaiting confirmation|no packages ready to publish' <<<"${output}"; then
      log "transient cargo workspace publish error; retrying after ${INDEX_WAIT_SECS}s"
      sleep "${INDEX_WAIT_SECS}"
      ((attempt++))
      continue
    fi

    if grep -qE 'no matching package named|failed to select a version' <<<"${output}"; then
      log "dependency not yet visible on crates.io; retrying after ${INDEX_WAIT_SECS}s"
      sleep "${INDEX_WAIT_SECS}"
      ((attempt++))
      continue
    fi

    log "fatal publish error for ${crate}"
    return "${status}"
  done

  log "gave up publishing ${crate} after ${MAX_ATTEMPTS} attempts"
  return 1
}

main() {
  log "workspace version ${VERSION}"
  for crate in "${CRATES[@]}"; do
    publish_crate "${crate}"
  done
  log "done"
}

main "$@"
