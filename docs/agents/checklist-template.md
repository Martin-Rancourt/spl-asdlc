# Checklist template for an angle

An angle may keep a checklist that its reviewer runs on every candidate. The checklist is the angle's memory of what it once missed; it grows by one line per escaped defect and never shrinks silently.

```markdown
# Checklist — <angle>

Version: <n> (<date>). One line per check; each ends in `pass | fail | not_run | not_applicable` in the report's `checked` list.

## Always
- [ ] <check that applies to every candidate>

## When the candidate touches <area>
- [ ] <check>

## Escapes
- <date> — <defect that escaped>, now covered by "<check>" above
```
