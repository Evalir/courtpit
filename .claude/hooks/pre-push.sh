#!/usr/bin/env bash
set -euo pipefail

# Consume stdin (hook protocol)
input=$(cat)

# Some Claude Code versions ignore the hook's `if` filter and run it for every Bash call, so
# check the command here too. Empty stdin (run by hand) always runs the checks.
if [ -n "$input" ]; then
    if command -v jq > /dev/null; then
        cmd=$(jq -r '.tool_input.command // ""' <<< "$input")
    else
        cmd=$input
    fi
    if ! [[ "$cmd" =~ (^|[^[:alnum:]_-])git[[:space:]]+push([[:space:]]|$|\") ]]; then
        exit 0
    fi
fi

cd "$(dirname "$0")/../.."

echo "Running pre-push checks..." >&2

if ! cargo +nightly fmt --all -- --check 2>&1; then
    echo "Format check failed. Run 'cargo +nightly fmt --all' to fix." >&2
    exit 2
fi

if ! cargo clippy --workspace --all-targets --all-features -- -D warnings 2>&1; then
    echo "Clippy (--all-features) failed." >&2
    exit 2
fi

if ! cargo clippy --workspace --all-targets --no-default-features -- -D warnings 2>&1; then
    echo "Clippy (--no-default-features) failed." >&2
    exit 2
fi

# `--document-private-items` matches the repo's quality gate (CLAUDE.md) and CI.
if ! RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items 2>&1; then
    echo "Doc check failed." >&2
    exit 2
fi

echo "All pre-push checks passed." >&2
exit 0
