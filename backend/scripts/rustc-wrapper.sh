#!/usr/bin/env bash
# rustc-wrapper.sh — Suppress warnings for ruvector path dependencies.
#
# Cargo treats path dependencies as local code and shows all warnings.
# Since ruvector is a git submodule we don't control, this wrapper
# injects --cap-lints allow for ruvector crates only, matching the
# behavior Cargo uses for registry/git dependencies.
#
# When used as rustc-wrapper, Cargo calls: <wrapper> <rustc> <args...>
# So $1 is the rustc binary, and ${@:2} are the compiler arguments.
#
# See: https://github.com/rust-lang/cargo/issues/8546

RUSTC_BIN="$1"
shift

# Crate names from the ruvector submodule (hyphens become underscores).
SUPPRESS_CRATES="ruvector_core|ruvector_collections|ruvector_gnn"

# Find the --crate-name argument to identify what's being compiled.
CRATE_NAME=""
PREV=""
for arg in "$@"; do
    if [ "$PREV" = "--crate-name" ]; then
        CRATE_NAME="$arg"
        break
    fi
    PREV="$arg"
done

# Suppress warnings for ruvector crates; pass through everything else.
if echo "$CRATE_NAME" | grep -qE "^($SUPPRESS_CRATES)$"; then
    exec "$RUSTC_BIN" "$@" --cap-lints allow
else
    exec "$RUSTC_BIN" "$@"
fi
