#!/usr/bin/env bash
# Removes what earlier CI runs left in the system's temporary folder of a
# self-hosted runner (ASDLC-3YRA).
#
# A job now keeps its temporary files in the runner's temp folder, which the
# runner empties when the job ends, whatever its outcome. Before that, the
# method's steps (`mktemp`), the tests and their Rust temporary files wrote to
# the system's folder, and nothing removed them. This sweep removes those
# leftovers, so that the disk corrects itself:
#
# - only the entries of the folder's top level owned by the user running the
#   job, never another account's;
# - only the names those runs gave: `tmp.*` (mktemp), `.tmp*` (Rust's
#   tempfile), `spl-asdlc-wf-*` (the workflow tests' scratch);
# - only an entry with nothing modified for SPL_ASDLC_SWEEP_MINUTES
#   (default 1440, a day): a job stops at six hours by default, so a run in
#   progress, on this runner or another of the same account, is never touched;
# - never a socket nor a pipe.
#
# The folder is SPL_ASDLC_SWEEP_DIR, by default /tmp. The sweep never fails
# the job: an entry it cannot remove is reported as a warning.
set -uo pipefail

dir="${SPL_ASDLC_SWEEP_DIR:-/tmp}"
minutes="${SPL_ASDLC_SWEEP_MINUTES:-1440}"
if ! [[ "$minutes" =~ ^[0-9]+$ ]]; then
  echo "::warning title=Runner sweep skipped::SPL_ASDLC_SWEEP_MINUTES is not a number of minutes: $minutes"
  exit 0
fi
if ! [ -d "$dir" ]; then
  echo "No folder $dir: nothing to sweep."
  exit 0
fi

removed=0
failed=0
while IFS= read -r -d '' entry; do
  # Anything in the entry modified recently: a run may still use it.
  if [ -n "$(find "$entry" -mmin "-$minutes" -print -quit 2>/dev/null)" ]; then
    continue
  fi
  if rm -rf -- "$entry" 2>/dev/null && ! [ -e "$entry" ]; then
    removed=$((removed + 1))
  else
    failed=$((failed + 1))
    echo "::warning title=Runner sweep::Could not remove $entry"
  fi
done < <(find "$dir" -mindepth 1 -maxdepth 1 -user "$(id -u)" \
  ! -type s ! -type p \
  \( -name 'tmp.*' -o -name '.tmp*' -o -name 'spl-asdlc-wf-*' \) \
  -print0 2>/dev/null)

echo "Swept $dir: $removed leftover(s) of earlier runs removed, $failed not removed."
exit 0
