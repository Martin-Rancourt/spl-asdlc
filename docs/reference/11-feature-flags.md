# Feature flags

A project that ships work behind feature flags can make sure that no flag stays in its code once the flag is gone, and that no flag is read without being declared. It adds one file, `feature-flags.toml`, at its repository's root; it adds no workflow line. The PR guards (`asdlc-pr-checks.yml`, `asdlc-pr-guards.yml`) check the code against that file on every ready PR, prose-only PRs included (rule 256D). With no file, nothing runs.

The check never reads nor writes a flag service. The life of a flag (creating it, rolling it out, turning it off) belongs to the flag service and the tracker; the repository only says which flags its code may use.

## The file

TOML, three keys, all optional (an absent key is an empty list). Any other key is refused.

```toml
# The flags the code may read. A flag is added here by the PR that first
# reads it, and removed by the PR that removes its last use.
active = ["new-board", "bulk-export"]

# Extra regular expressions (Python syntax) that find a flag read the
# built-in calls below do not see, each with exactly one capture group: the
# flag's key.
usage = ['flagOn\(\s*"([A-Za-z0-9_-]+)"']

# Tracked paths never read: a glob (`*` also crosses `/`), or a folder
# ending in `/`. The flags file itself is never read.
exclude = ["CHANGELOG.md", "changelog/"]
```

- A key is letters, digits, `_` and `-`, starting with a letter or a digit.
- A key is listed once in `active`.
- There is no list of retired flags: removing a key from `active` is enough, and check 1 below refuses any read of it left behind.

## The two checks

The guards read the files git tracks at the PR's head (`git ls-files`; untracked, symlinked and binary files are skipped), outside the flags file and the `exclude` paths. They fail when:

1. **A flag read is not active.** A flag-reading call names a key that is not in `active`. The built-in calls, whose first string argument (positional, or named as `key: "…"` or `key = "…"`) is the key, are PostHog's: `isFeatureEnabled`, `getFeatureFlag`, `getFeatureFlagPayload`, `getFeatureFlagResult`, `useFeatureFlagEnabled`, `useFeatureFlagVariantKey`, `useFeatureFlagPayload`, `useFeatureFlag` (JavaScript, React, Node, Swift, Kotlin) and `feature_enabled`, `get_feature_flag`, `get_feature_flag_payload`, `is_feature_enabled` (Python, Rust). A key read any other way (a wrapper, a variable) is seen only through a `usage` pattern: wrap the flag reads in one helper and declare it.
2. **An active flag is no longer in the code.** A key of `active` is named in no file read, as a whole name: `old-board` is named by `"old-board"`, never by `old-board-v2` nor `old_board`. Any mention counts (a call, a constant, a comment): the check asks that a flag gone from the code is gone from the list. A path that names old flags as history (a changelog) belongs in `exclude`, or it keeps them alive.

A malformed file fails the guards too: invalid TOML, an unknown key, a value that is not a list of strings, a bad key, a key listed twice, a `usage` pattern that does not compile or does not have exactly one capture group.

Each finding is an error annotation, `Feature flags`, on the file and line of the read (check 1) or on `feature-flags.toml` (check 2 and a malformed file).

## The runner

The check runs on the guards' runner (`runs-on`) with Python 3.11 or later (`tomllib`) and git; a runner without them fails a PR of a repository that has the file, and says so. A project with no file needs neither.
