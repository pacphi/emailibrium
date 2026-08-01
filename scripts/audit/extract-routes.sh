#!/usr/bin/env bash
# extract-routes.sh — emit the backend's ACTUAL HTTP route surface.
#
# Ground truth for docs/audit/drift-report.md and docs/api/openapi.yaml (phase 3 of the
# docs-accuracy-audit pipeline). Reproducible, deterministic, no network, no prompts — safe
# to run in CI (phase 6 reuses it as the drift gate).
#
# How it works: Axum routers in this codebase are assembled from `.route("path", handler)`
# leaves composed via `.nest("prefix", module::routes())` and `.merge(module::routes())`
# calls, starting at main.rs's `Router::new().nest("/api/v1", api::routes())`. This script
# walks that composition tree using Rust's standard (non-`#[path]`) module-path convention —
# a file's module path is derived purely from its location under backend/src — to resolve
# every `module::routes()` reference to the file that defines it, then concatenates prefixes
# down to each leaf `.route()` call. `pub use X::routes;` re-exports (e.g. cleanup/mod.rs) are
# followed transparently.
#
# Output: one full route path per line (METHOD-agnostic — a path may carry multiple methods),
# sorted and de-duplicated. Opaque service mounts (`.nest_service`, e.g. the MCP transport)
# are emitted as "<prefix> (opaque service mount)".
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SRC="$REPO_ROOT/backend/src"

python3 - "$SRC" <<'PY'
import re, sys, os

SRC = sys.argv[1]

def module_path_for(filepath):
    """Derive a file's Rust module path purely from its location under SRC (no #[path] in this repo)."""
    rel = os.path.relpath(filepath, SRC)
    rel = rel[:-3] if rel.endswith(".rs") else rel
    parts = rel.split(os.sep)
    if parts == ["main"]:
        return []
    if parts[-1] == "mod":
        parts = parts[:-1]
    return parts

# Index every .rs file under SRC by its derived module path.
files = []
for root, _dirs, names in os.walk(SRC):
    for name in names:
        if name.endswith(".rs"):
            files.append(os.path.join(root, name))

mod_to_file = {}
for f in files:
    mod_to_file[tuple(module_path_for(f))] = f

def read(f):
    with open(f, encoding="utf-8") as fh:
        return fh.read()

def find_routes_fn_body(src):
    """Extract the brace-matched body of the first `pub fn routes(` in src, or None."""
    m = re.search(r'pub fn routes\s*\([^)]*\)[^{]*\{', src)
    if not m:
        return None
    start = m.end()
    depth = 1
    i = start
    while i < len(src) and depth > 0:
        if src[i] == '{':
            depth += 1
        elif src[i] == '}':
            depth -= 1
        i += 1
    return src[start:i-1]

def find_reexport_target(src):
    """`pub use X::routes;` (with optional leading `crate::`) — returns the dotted segments."""
    m = re.search(r'pub use\s+([\w:]+)::routes\s*;', src)
    if not m:
        return None
    return m.group(1).split("::")

def resolve(segments, caller_mod_path, is_mod_rs):
    """Resolve a (possibly crate-/super-/bare-) module reference to a module path tuple."""
    if segments[0] == "crate":
        target = segments[1:]
    elif segments[0] == "super":
        target = list(caller_mod_path[:-1]) + segments[1:]
    else:
        base = list(caller_mod_path) if is_mod_rs else list(caller_mod_path[:-1])
        target = base + segments
    return tuple(target)

def leaves_for(mod_path, seen=None):
    """Return list of (prefix, path) tuples: fully-resolved leaf routes under this module."""
    seen = seen or set()
    if mod_path in seen:
        return []  # cycle guard
    seen = seen | {mod_path}

    f = mod_to_file.get(mod_path)
    if f is None:
        return [("", f"<unresolved module: {'::'.join(mod_path)}>")]

    src = read(f)
    body = find_routes_fn_body(src)
    is_mod_rs = os.path.basename(f) == "mod.rs"

    if body is None:
        reexport = find_reexport_target(src)
        if reexport:
            target = resolve(reexport, mod_path, is_mod_rs)
            return leaves_for(target, seen)
        return [("", f"<no pub fn routes in: {'::'.join(mod_path)}>")]

    out = []

    # .route("path", ...) — string literal may be on the next line for multi-line calls.
    for m in re.finditer(r'\.route\s*\(\s*"([^"]*)"', body):
        out.append((m.group(1), None))

    # .nest_service("prefix", ...) — opaque mount, not further resolved.
    for m in re.finditer(r'\.nest_service\s*\(\s*"([^"]*)"', body):
        out.append((m.group(1), "OPAQUE"))

    # .nest("prefix", MODEXPR::routes())
    for m in re.finditer(r'\.nest\s*\(\s*"([^"]*)"\s*,\s*([\w:]+)::routes\s*\(\s*\)', body):
        prefix, modexpr = m.group(1), m.group(2)
        target = resolve(modexpr.split("::"), mod_path, is_mod_rs)
        for sub_prefix, sub in leaves_for(target, seen):
            combined = prefix.rstrip("/") + "/" + sub_prefix.lstrip("/") if sub_prefix else prefix
            out.append((combined, sub))

    # .merge(MODEXPR::routes()) — same level, no added prefix segment.
    for m in re.finditer(r'\.merge\s*\(\s*([\w:]+)::routes\s*\(\s*\)', body):
        modexpr = m.group(1)
        target = resolve(modexpr.split("::"), mod_path, is_mod_rs)
        out.extend(leaves_for(target, seen))

    return out

def normalize(path):
    path = re.sub(r'/+', '/', path)
    if not path.startswith("/"):
        path = "/" + path
    if len(path) > 1 and path.endswith("/"):
        path = path[:-1]
    return path

# Entry point: main.rs's top-level Router::new() chain.
main_file = mod_to_file.get(())
main_src = read(main_file)

results = set()
for m in re.finditer(r'\.nest\s*\(\s*"([^"]*)"\s*,\s*([\w:]+)::routes\s*\(\s*\)', main_src):
    prefix, modexpr = m.group(1), m.group(2)
    target = resolve(modexpr.split("::"), (), True)
    for sub_prefix, sub in leaves_for(target):
        full = normalize(prefix.rstrip("/") + "/" + sub_prefix.lstrip("/") if sub_prefix else prefix)
        if sub == "OPAQUE":
            results.add(f"{full} (opaque service mount)")
        elif sub and sub.startswith("<"):
            results.add(f"{full} {sub}")
        else:
            results.add(full)

for m in re.finditer(r'\.nest_service\s*\(\s*"([^"]*)"', main_src):
    results.add(f"{normalize(m.group(1))} (opaque service mount)")

for m in re.finditer(r'^\s*\.route\s*\(\s*"([^"]*)"', main_src, re.MULTILINE):
    results.add(normalize(m.group(1)))

for line in sorted(results):
    print(line)
PY
