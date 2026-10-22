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
# The rule allows this product's repository slug (`<owner>/spl-asdlc`), whose
# owner part may be a forbidden word: the slug is read from GITHUB_REPOSITORY
# or, failing that, from the GitHub URL of the `origin` remote, and removed
# from a hit line before the word is looked for again. Only the exact slug is
# allowed; the owner name on its own is still a hit. No slug found, nothing is
# removed.
#
# Without SPL_ASDLC_FORBIDDEN_WORDS (a pull request from a fork gets no secret),
# only the forms are checked, and the skip is reported: a warning on stderr and,
# under GitHub Actions, a warning annotation and a line in the step summary.
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
slug=${GITHUB_REPOSITORY:-}
if [ -z "$slug" ]; then
  url=$(git remote get-url origin 2>/dev/null || true)
  slug=$(printf '%s\n' "$url" | sed -nE 's#^(https://([^@/]+@)?github\.com/|git@github\.com:|ssh://git@github\.com/)([A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)$#\3#p' | sed 's/\.git$//')
fi
case "$slug" in */spl-asdlc) ;; *) slug= ;; esac
if [ -n "${SPL_ASDLC_FORBIDDEN_WORDS:-}" ]; then
  IFS=',' read -r -a words <<< "$SPL_ASDLC_FORBIDDEN_WORDS"
  for w in "${words[@]}"; do
    w=$(printf '%s' "$w" | sed 's/^ *//;s/ *$//'); [ -z "$w" ] && continue
    hits=$(printf '%s\n' "$files" | xargs -r grep -niwF -- "$w" 2>/dev/null || true)
    if [ -n "$hits" ] && [ -n "$slug" ]; then
      # Keep a line only if the word is still there once the slug is gone.
      hits=$(printf '%s\n' "$hits" | while IFS= read -r line; do
        rest=$(printf '%s\n' "$line" | awk -v s="$slug" '{ while ((i = index($0, s)) > 0) $0 = substr($0, 1, i - 1) substr($0, i + length(s)); print }')
        if printf '%s\n' "$rest" | grep -iwF -- "$w" >/dev/null; then printf '%s\n' "$line"; fi
      done)
    fi
    [ -n "$hits" ] && report "$hits"
  done
else
  # A pull request from a fork gets no secret, so no word list: the forms above
  # were still checked and the scan passes on them, but the skip must be seen.
  msg='SPL_ASDLC_FORBIDDEN_WORDS is not set (a pull request from a fork gets no secret): only identifier forms were checked, not the forbidden words'
  printf 'warning: %s\n' "$msg" >&2
  if [ "${GITHUB_ACTIONS:-}" = true ]; then
    printf '::warning title=BR-ASDLC-001::%s\n' "$msg"
    if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
      printf '> [!WARNING]\n> BR-ASDLC-001: %s.\n' "$msg" >> "$GITHUB_STEP_SUMMARY"
    fi
  fi
fi

if [ "$status" -ne 0 ]; then
  printf 'BR-ASDLC-001 violated: instance data found (see above)\n' >&2
fi
exit "$status"
