# K3TZ — A socket request of another SPL-ASDLC major is refused, never interpreted.

Status: Enforced

## Rule

Every request and response of the Livela runner's socket carries the SPL-ASDLC major of its sender. The server reads it before anything else: a request of another major, or with none, is answered with an error that names both majors, and its call is neither parsed nor run, so it writes nothing. The program refuses a response of another major the same way.

## Evidence

ASDLC-4Z4G: the runner and the program are installed apart, each at its own version; a change of major may change the protocol, and a misread request would write on a guess.

## Test

`tracker::livela::tests::over_the_socket::a_request_of_another_major_is_refused_never_interpreted`, `tracker::livela::tests::over_the_socket::the_program_refuses_a_runner_of_another_major` (unit test job).
