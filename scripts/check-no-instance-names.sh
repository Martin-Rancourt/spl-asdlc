#!/usr/bin/env bash
# BR-ASDLC-001: the repository carries no instance data.
#
# Scans every tracked file for the FORMS of installation identifiers (e-mail
# addresses, home paths, long numeric ids, hexadecimal uids, GitHub App bot
# logins) and for the words given in SPL_ASDLC_FORBIDDEN_WORDS (comma-separated).
# The word list lives outside the repository (a CI secret or repository variable,
# or a local untracked file sourced into the variable) so that the list itself
# never enters the tree.
#
# Exit 0 when nothing is found, 1 otherwise. No baseline, no exemption list.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

status=0
report() { printf '%s\n' "$1"; status=1; }

files=$(git ls-files)

# 1. Forms of installation identifiers.
patterns=(
  '[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}'   # e-mail address
  '/home/[a-z][a-z0-9_-]*'                              # Linux home path
  '/Users/[A-Za-z][A-Za-z0-9_-]*'                       # macOS home path
  '(^|[^0-9])[0-9]{9,}([^0-9]|$)'                        # 9+-digit account / chat id
  '\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b'  # uuid
  '\b[a-z0-9-]+\[bot\]'                                 # GitHub App bot login
)
for p in "${patterns[@]}"; do
  # Never decide on grep's exit status through xargs (123 when one batch has no match): read the output.
  hits=$(printf '%s\n' "$files" | xargs -r grep -nE -- "$p" 2>/dev/null || true)
  # A digit run inside a long hexadecimal hash (a lockfile checksum, a commit id) is not an
  # identifier: strip such hashes from the hit lines and keep only what still matches.
  hits=$(printf '%s\n' "$hits" | sed -E 's/[0-9a-fA-F]{32,}//g' | grep -E -- "$p" || true)
  # The licence text and this script's own pattern list are the only allowed carriers.
  hits=$(printf '%s\n' "$hits" | grep -v -e '^LICENSE:' -e '^scripts/check-no-instance-names.sh:' -e '^$' || true)
  [ -n "$hits" ] && report "$hits"
done

# 2. Forbidden words (names of products, agents, people, installations).
if [ -n "${SPL_ASDLC_FORBIDDEN_WORDS:-}" ]; then
  IFS=',' read -r -a words <<< "$SPL_ASDLC_FORBIDDEN_WORDS"
  for w in "${words[@]}"; do
    w=$(printf '%s' "$w" | sed 's/^ *//;s/ *$//'); [ -z "$w" ] && continue
    hits=$(printf '%s\n' "$files" | xargs -r grep -niwF -- "$w" 2>/dev/null || true)
    [ -n "$hits" ] && report "$hits"
  done
else
  printf 'warning: SPL_ASDLC_FORBIDDEN_WORDS is not set; only identifier forms were checked\n' >&2
fi

if [ "$status" -ne 0 ]; then
  printf 'BR-ASDLC-001 violated: instance data found (see above)\n' >&2
fi
exit "$status"
