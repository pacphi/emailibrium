#!/usr/bin/env bash
# imap-probe.sh — read-only IMAP capability probe for one mailbox.
#
# Logs in, asks for CAPABILITY, opens INBOX read-only with EXAMINE (no flags
# change, RFC 3501 section 6.3.2), and logs out. Writes only the server's
# responses to an output file, so you can see which move strategy (MOVE /
# UIDPLUS) and keyword support (PERMANENTFLAGS) emailibrium will get.
#
# Usage:
#   scripts/imap-probe.sh <host> <port> <username> [--starttls] [output-file]
#
# Examples:
#   scripts/imap-probe.sh imap.gmail.com 993 you@gmail.com
#   scripts/imap-probe.sh imap.mail.me.com 993 you@icloud.com
#   scripts/imap-probe.sh 127.0.0.1 1143 you@proton.me --starttls     # Proton Bridge
#
# The password (an app password) is read from the terminal without echo. It
# is never put on a command line and never written to the output file.
# Outlook / Microsoft 365 can't be probed this way: they refuse password
# login (LOGINDISABLED) and accept only OAuth (XOAUTH2).
set -euo pipefail

if [ "$#" -lt 3 ]; then
  sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
  exit 2
fi

host="$1"
port="$2"
user="$3"
shift 3

# Expanded as ${starttls[@]+"${starttls[@]}"}: an empty array is an "unbound
# variable" under `set -u` in the bash 3.2 macOS ships.
starttls=()
if [ "${1:-}" = "--starttls" ]; then
  starttls=(-starttls imap)
  shift
fi
out="${1:-imap-probe-${host}-$(date -u +%Y%m%dT%H%M%SZ).txt}"

command -v openssl >/dev/null 2>&1 || { echo "openssl is required" >&2; exit 1; }

printf 'App password for %s (not shown): ' "$user" >&2
IFS= read -rs password
echo >&2

# SASL PLAIN with initial response (RFC 4616, RFC 4959): avoids IMAP quoting
# of the password. base64 of "\0user\0password", one line.
auth="$(printf '\0%s\0%s' "$user" "$password" | base64 | tr -d '\n')"
unset password

{
  echo "# imap-probe ${host}:${port} as ${user} at $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  {
    printf 'p1 AUTHENTICATE PLAIN %s\r\n' "$auth"
    sleep 3
    printf 'p2 CAPABILITY\r\n'
    sleep 2
    printf 'p3 EXAMINE INBOX\r\n'
    sleep 2
    printf 'p4 LOGOUT\r\n'
    sleep 2
  } | openssl s_client -quiet -connect "${host}:${port}" -servername "$host" ${starttls[@]+"${starttls[@]}"} 2>/dev/null \
    | tr -d '\r'
} >"$out" || true
unset auth

if ! grep -q '^\* ' "$out"; then
  echo "No response from ${host}:${port}. Check the host, the port, and --starttls" >&2
  echo "(Proton Bridge on 1143 needs --starttls; try 127.0.0.1 instead of localhost)." >&2
  exit 1
fi

echo "Wrote $out" >&2
echo "Look for: a 'p1 OK' line (login worked), the '* CAPABILITY' line (MOVE, UIDPLUS)," >&2
echo "and '[PERMANENTFLAGS (...)]' after p3 (\\* means custom keywords are kept)." >&2
