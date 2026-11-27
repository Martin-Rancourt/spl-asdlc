# FDQ8 — The spl-asdlc program reaches a Livela store only through the Livela runner's socket.

Status: Enforced

## Rule

Run apart from the Livela runner, the `spl-asdlc` program never opens, reads nor writes the Livela store itself: it reaches it only through the runner's local socket (`[tracker] socket`, `docs/reference/livela-contract.md`, "Socket protocol"), and the runner stays the store's only owner. Outside its tests, the crate implements `LivelaStore` once, for the socket client; a Livela tracker with no `socket` is refused with the key to add, never opened another way.

## Evidence

ASDLC-4Z4G (the architecture decision on it): the program runs beside the runner, which keeps sole ownership of the store; the public HTTP/MCP API was set aside.

## Test

`tracker::livela::tests::the_only_livela_store_the_program_holds_is_the_runner_socket`, `tracker::livela::tests::over_the_socket::the_host_opens_a_livela_tracker_by_its_socket_and_names_the_missing_key`, `tracker::livela::tests::over_the_socket::the_conformance_suite_passes_through_the_runner_socket` (unit test job).
