# 10 — The run journal

Every release cycle keeps a journal, written by whoever drives the cycle (the bridge writes the facts, a human or an agent writes the reading). It is the memory of what the method did and did not do; the record of the cycle (the merged angle reports) is its evidence.

## Template

```markdown
---
product: <product>
version: <version>
opened: <date time, local>
closed: <date time, local>
rc_passes: <n>
hands: <number of human gestures outside the method's list>
incidents: <number of defects of the chain found during the cycle>
---

# Run — <product> v<version> (<date>)

Driven by <who>. Gestures outside the method: <n>, listed below.

## Timeline
- <time> — cut (dispatch), RC1 tagged, class <routine|sensible>
- <time> — release issue created by the bridge
- <time> — gate green on RC1
- <time> — angle sub-items, consolidation item (blocked by them) and brief created by the bridge (once the gate is green)
- <time> — angle PRs: <angle> (go), <angle> (hold: 1 blocker fixed in branch), …, each reviewed by the coordinator as it came
- <time> — angle PRs merged; overall review by the coordinator; RC2 tagged, gate green
- <time> — GO computed; release tag and release published by the bridge
- <time> — published; merge-back merged
- <time> — closure: <n> tickets Done, <n> Post Deploy, <n> back In Progress; release note sent (<channels>)

## What held
<what went as the method says>

## Findings about the chain
1. <a defect of the method or the tooling, with the requirement it violates and the ticket opened for it>

## Gestures outside the method
1. <what, why, and whether it becomes a requirement, a configuration or an exception>
```

## Rules

1. `hands` and `incidents` are counted honestly; a cycle with three explained gestures is worth more than a cycle at zero that hides one.
2. Every gesture outside the method ends as a requirement of the tooling, a configuration of the project, or a documented exception. Never as a habit.
3. Times are local to the driver, with the zone named once in the front matter.
