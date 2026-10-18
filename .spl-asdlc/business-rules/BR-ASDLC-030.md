# BR-ASDLC-030 — An angle may deliver LLM instructions, a reusable workflow, or both; a project runs an angle's workflow only by calling it from its own CI, pinned on the same version, and no switch or flag decides it.

Status: Enforced

## Rule

An angle's LLM instructions are `docs/agents/angles/<key>.md`, which the bridge copies into the angle's items. Its workflow is `.github/workflows/asdlc-angle-<key>.yml`, a reusable workflow of the method; when it has both, the instructions carry a `## Workflow results` section that tells the agent where the workflow's results are and how to read them. Nothing enters a project's CI unless the project wires it: the project calls the workflow from its own workflows, on the same ref as its other callers of the method, and no configuration key, input or flag adds it or turns it off. The library's four angles are LLM instructions only. `docs/reference/08-configuration.md`, "Angles".

## Evidence

The owner's decision in the angle brainstorm of 2026-10-18 (SID-974), implemented by SID-1192.

## Test

`method::texts::protocol::tests::every_angle_says_its_parts_and_a_workflow_angle_says_where_its_results_are`, `method::pin::tests::the_pin_is_the_one_ref_every_caller_of_the_method_names`, `method::pin::tests::no_caller_or_two_versions_tell_no_pin`
