# Angle: security

Identity and privilege (who can do what, every entry point), data and privileged capability (storage, secrets, RLS or equivalent, admin surfaces), input and cost (validation, injection, quotas, abuse), ingress and response (headers, redirects, errors that leak), delivery chain (CI, dependencies, tokens, workflows). Each check ends in one of `pass | fail | not_run | not_applicable`; a `not_run` or `fail` on a required check is a `blocker`. A finding names the surface, the attacker, the impact and the fix.
