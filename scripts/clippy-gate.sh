#!/usr/bin/env bash
# ============================================================================
# Release gate: the tree must be clippy-clean.
#
# Runs `cargo clippy --all-targets` with every warning promoted to an error.
# A non-zero exit means the gate is CLOSED: fix the reported lints before
# tagging a release or merging. Warnings are never allowed to pile up
# again — the baseline is zero (audit M3).
#
# Requires: cargo + clippy (rustup component).
# Usage:    scripts/clippy-gate.sh        (works from any cwd)
# ============================================================================
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "[clippy-gate] cargo clippy --all-targets -- -D warnings (repo: $ROOT)"

if output=$(cargo clippy --all-targets --quiet -- -D warnings 2>&1); then
    echo "[clippy-gate] PASS — zero clippy warnings."
    exit 0
else
    echo "[clippy-gate] FAIL — clippy reported problems:" >&2
    echo "$output" >&2
    echo "[clippy-gate] Gate CLOSED: fix the lints above (never silence with blanket allows)." >&2
    exit 1
fi
