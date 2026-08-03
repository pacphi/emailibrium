#!/usr/bin/env bash
# extract-shortcuts.sh — extract the frontend's ACTUAL registered keyboard-shortcut keys
# and VERIFY them against docs/user-guide.md's Keyboard Shortcuts table.
#
# Ground truth for the doc table (keyboard-shortcuts pipeline; hardened after the
# integration court caught this script silently missing metaOrCtrl-generated keys).
# Reproducible, deterministic, no network, no prompts. Exit 0 = the sets match;
# exit 1 = drift, with both directions of the difference printed to stderr.
#
# How it works: every shortcut goes through frontend/apps/web/src/shared/hooks/useKeyboard.ts,
# built from a `useMemo<ShortcutMap>(...)` block immediately followed by the `useKeyboard(...)`
# call in the same hook. This script finds every non-test file that calls useKeyboard(, slices
# out just that block (so it never sees unrelated object literals elsewhere in the file), and
# extracts shortcut keys four ways:
#   - object-literal entries        `c: onCompose`, `'cmd+k': toggle`, `'?': toggleHelp`
#   - dot-assignment entries        `map.escape = close;`
#   - bracket-assignment entries    `map['#'] = onDelete;`
#   - metaOrCtrl helper calls       `metaOrCtrl('shift+a', fn)` -> cmd+shift+a AND ctrl+shift+a
#     (both spread `...metaOrCtrl(...)` and direct `= metaOrCtrl(...)` / Object.assign forms)
#
# The doc side parses the table under "## Keyboard Shortcuts" in docs/user-guide.md, taking
# every backticked token in each row's first column. Rows implemented as scoped local React
# onKeyDown handlers (NOT useKeyboard registrations) are listed in DOC_ONLY_KEYS below and
# excluded from the comparison -- they are documented behavior, just not this mechanism's.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

python3 - "$REPO_ROOT" <<'PY'
import re
import subprocess
import sys

REPO_ROOT = sys.argv[1]
SRC = f"{REPO_ROOT}/frontend/apps/web/src"
DOC = f"{REPO_ROOT}/docs/user-guide.md"

# Documented shortcuts that are intentionally NOT useKeyboard registrations: the email
# list's arrow-key navigation and the ReplyBox/Chat Enter handling are scoped local React
# onKeyDown handlers on their own elements. Documented in the same table, excluded here.
DOC_ONLY_KEYS = {"↓", "↑", "cmd+enter", "enter", "shift+enter"}

# ---- code side -------------------------------------------------------------------

try:
    grep_out = subprocess.run(
        ["grep", "-rl", "useKeyboard(", SRC, "--include=*.ts", "--include=*.tsx"],
        capture_output=True, text=True, check=True,
    ).stdout
except subprocess.CalledProcessError:
    grep_out = ""
files = [f for f in grep_out.splitlines() if f and ".test." not in f]

# A bare identifier key ('c', 'r', 'f', 'e', ...) -- this codebase only ever uses single/double
# lowercase letters for these, which keeps the pattern from matching unrelated identifiers.
#
# LIMITATION: the trailing (?:\(|[a-zA-Z_]) only looks at the value's first character, to
# distinguish a handler reference/arrow function from a non-shortcut key like `mode: 'reply'`
# (whose value starts with a quote). A future shortcut entry whose value starts with something
# else would silently be dropped -- but the bidirectional doc comparison below now turns any
# such silent drop into a hard failure instead of an unnoticed gap.
LITERAL_KEY_RE = re.compile(
    r"(?:^|[{,]\s*)(?:'([a-zA-Z0-9+,#?]+)'|([a-z]{1,2}))\s*:\s*(?:\(|[a-zA-Z_])"
)
ASSIGN_DOT_RE = re.compile(r"\bmap\.([a-zA-Z][a-zA-Z0-9]*)\s*=")
ASSIGN_BRACKET_RE = re.compile(r"\bmap\['([^']+)'\]\s*=")
META_OR_CTRL_RE = re.compile(r"\bmetaOrCtrl\(\s*'([^']+)'")

registered = set()

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
    # Strip line comments -- a `//` explainer between two entries is not whitespace, so it
    # would otherwise break the comma-then-key adjacency the key regex relies on.
    block = re.sub(r"//[^\n]*", "", block)

    for m in LITERAL_KEY_RE.finditer(block):
        registered.add((m.group(1) or m.group(2)).lower())
    for m in ASSIGN_DOT_RE.finditer(block):
        registered.add(m.group(1).lower())
    for m in ASSIGN_BRACKET_RE.finditer(block):
        registered.add(m.group(1).lower())
    for m in META_OR_CTRL_RE.finditer(block):
        key = m.group(1).lower()
        registered.add(f"cmd+{key}")
        registered.add(f"ctrl+{key}")

# ---- doc side --------------------------------------------------------------------

with open(DOC, encoding="utf-8") as f:
    doc = f.read()

section_match = re.search(r"^## Keyboard Shortcuts\n(.*?)(?=^## )", doc, re.M | re.S)
if not section_match:
    print("ERROR: no '## Keyboard Shortcuts' section found in docs/user-guide.md", file=sys.stderr)
    sys.exit(1)

documented = set()
for line in section_match.group(1).splitlines():
    line = line.strip()
    if not line.startswith("|"):
        continue
    first_cell = line.split("|")[1]
    if set(first_cell.strip()) <= {"-", " "}:  # header separator row
        continue
    for token in re.findall(r"`([^`]+)`", first_cell):
        documented.add(token.lower())
documented.discard("shortcut")  # header row's own cell, if backticked

# ---- compare ---------------------------------------------------------------------

for key in sorted(registered):
    print(key)

expected_from_doc = documented - DOC_ONLY_KEYS
missing_in_doc = registered - documented
missing_in_code = expected_from_doc - registered
stale_exclusions = DOC_ONLY_KEYS & registered

ok = True
if missing_in_doc:
    print(f"DRIFT: registered in code but not documented: {sorted(missing_in_doc)}", file=sys.stderr)
    ok = False
if missing_in_code:
    print(f"DRIFT: documented but not registered in code: {sorted(missing_in_code)}", file=sys.stderr)
    ok = False
if stale_exclusions:
    print(f"DRIFT: DOC_ONLY_KEYS now registered through useKeyboard: {sorted(stale_exclusions)}", file=sys.stderr)
    ok = False

if not ok:
    sys.exit(1)
print(f"OK: {len(registered)} registered shortcuts all documented; no doc-only drift", file=sys.stderr)
PY
