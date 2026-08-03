#!/usr/bin/env bash
# extract-shortcuts.sh — emit the frontend's ACTUAL registered keyboard-shortcut keys.
#
# Ground truth for docs/user-guide.md's Keyboard Shortcuts table (phase 4 of the
# keyboard-shortcuts pipeline). Reproducible, deterministic, no network, no prompts.
#
# How it works: every shortcut goes through frontend/apps/web/src/shared/hooks/useKeyboard.ts,
# built from a `useMemo<ShortcutMap>(...)` block immediately followed by the `useKeyboard(...)`
# call in the same hook. This script finds every non-test file that calls useKeyboard(, slices
# out just that block (so it never sees unrelated object literals elsewhere in the file, e.g. a
# Zustand store's own action map), and extracts shortcut keys two ways: object-literal entries
# (`c: onCompose`, `'cmd+k': toggle`) and the conditional-addition pattern this codebase uses for
# an entry that should only be registered part of the time (`map.escape = close;`, guarded by an
# `if (isOpen) { ... }` above it).
#
# Output: one shortcut key per line (lowercased, e.g. "cmd+k", "shift+#", "e"), sorted and
# de-duplicated. NOTE: this only covers shortcuts registered through useKeyboard -- EmailList's
# arrow-key navigation and ReplyBox/ChatInput's Enter/Ctrl+Enter handling are scoped local React
# onKeyDown handlers, not useKeyboard registrations, and are intentionally out of scope here (see
# docs/user-guide.md's own doc comment on those rows).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SRC="$REPO_ROOT/frontend/apps/web/src"

python3 - "$SRC" <<'PY'
import re
import subprocess
import sys

SRC = sys.argv[1]

# Every non-test file that actually calls useKeyboard(...).
try:
    grep_out = subprocess.run(
        ["grep", "-rl", "useKeyboard(", SRC, "--include=*.ts", "--include=*.tsx"],
        capture_output=True, text=True, check=True,
    ).stdout
except subprocess.CalledProcessError:
    grep_out = ""
files = [f for f in grep_out.splitlines() if f and ".test." not in f]

# A bare identifier key ('c', 'r', 'f', 'e', ...) -- this codebase only ever uses single/double
# lowercase letters for these, which keeps the pattern from matching unrelated identifiers
# (e.g. a Zustand action name) that happen to precede a colon.
#
# LIMITATION: the trailing (?:\(|[a-zA-Z_]) only looks at the value's first character, to
# distinguish a handler reference/arrow function from a non-shortcut key like `mode: 'reply'`
# (whose value starts with a quote). A future shortcut entry whose value starts with something
# else (e.g. a ternary or a negation) would silently be dropped rather than erroring -- if this
# script's output ever looks short, check for that before assuming the doc table is wrong.
LITERAL_KEY_RE = re.compile(
    r"(?:^|[{,]\s*)(?:'([a-zA-Z0-9+,#?]+)'|([a-z]{1,2}))\s*:\s*(?:\(|[a-zA-Z_])"
)
# The conditional-addition pattern: `map.<key> = ...;`
ASSIGN_RE = re.compile(r"\bmap\.([a-zA-Z][a-zA-Z0-9]*)\s*=")

keys = set()

for path in files:
    with open(path, encoding="utf-8") as f:
        source = f.read()

    start = source.find("useMemo<ShortcutMap>")
    if start == -1:
        continue
    end = source.find("useKeyboard(", start)
    if end == -1:
        continue
    block = source[start:end]
    # Strip line comments -- a `//` explainer between two entries (e.g. documenting why a
    # shift-modifier is needed) is not whitespace, so it would otherwise break the
    # comma-then-key adjacency the key regex below relies on.
    block = re.sub(r"//[^\n]*", "", block)

    for m in LITERAL_KEY_RE.finditer(block):
        keys.add((m.group(1) or m.group(2)).lower())
    for m in ASSIGN_RE.finditer(block):
        keys.add(m.group(1).lower())

for key in sorted(keys):
    print(key)
PY
