# Optional webhook relay

Use this only when faster reactions are useful. Periodic polling remains required;
`tick` and existing hosts are unchanged unless they opt in. Nothing listens on the
bridge host. This ships a Cloudflare Worker and a SQLite-backed Durable Object
queue in `relay/` (see [Cloudflare's storage model](https://developers.cloudflare.com/durable-objects/concepts/what-are-durable-objects/)).

## Deploy the relay

1. Use a Cloudflare account with Workers and Durable Objects enabled and install
   the Wrangler CLI following its [installation instructions](https://developers.cloudflare.com/workers/wrangler/install-and-update/).
2. In `relay/wrangler.toml`, set `REPOSITORIES` to a comma-separated allowlist of
   installed repositories, e.g. `example-org/acme,example-org/widget`. Use a
   separate deployment and pull token per bridge installation/trust boundary.
3. From `relay/`, run `wrangler secret put WEBHOOK_SECRET` and
   `wrangler secret put PULL_TOKEN`. Generate two distinct random secrets in your
   secret manager; enter their values at the prompts, never in a tracked file.
   Keep the first for GitHub and the second for the bridge's environment.
4. Run `wrangler deploy`. The migration creates the queue namespace. Use the
   resulting HTTPS origin as `queue_url` below, without a trailing slash.
5. In the GitHub App, enable webhooks, set the URL to `<origin>/webhook`, content
   type JSON, and the secret to `WEBHOOK_SECRET`. Subscribe to available events:
   push, pull request, pull request review, pull request review thread, check run,
   check suite, workflow run, release, create, delete, issues, issue comment and
   status. Permissions follow [github-app.md](github-app.md). Ping is ignored.
   A valid signed event for a repository outside the allowlist is ignored.

The relay verifies `X-Hub-Signature-256` using HMAC-SHA256 over the raw bytes before
parsing JSON. It limits bodies to 1 MiB, drops the payload and stores only
`{"repository":"example-org/acme","kind":"push"}`. All pending events of one
repository coalesce into one durable hint; neither history nor GitHub credentials
are stored. A missed/rejected event is recovered by the next periodic poll.

## Configure and run the bridge

In the host's instance overlay, add:

```toml
[bridge]
poll_minutes = 5
queue_url = "https://relay.example.org"
queue_token = "env:SPL_ASDLC_QUEUE_TOKEN"
```

Set `SPL_ASDLC_QUEUE_TOKEN` in the host's secret environment to the relay's
`PULL_TOKEN`. The bearer token grants access to all repositories on this relay;
rotate both ends together. HTTPS is mandatory and redirects are never followed.

Run `spl-asdlc doctor --file .spl-asdlc/config.toml`: `webhook queue: ok` verifies
an authenticated, non-consuming `/health` request and queue storage access. An
unset secret or unreachable relay reports `missing` with this setup page.

Run `spl-asdlc watch --file .spl-asdlc/config.toml` under your process supervisor,
one process per repository (normal termination stops it). It ticks at startup,
then long-polls `GET /events?repository=example-org/acme&wait=25`. An event returns
immediately and triggers a full tick. The queue pull consumes the hint. Configure
only one consumer per repository; a crash after consumption loses only a wake,
not work. Events during a tick coalesce and wake the next pass.

The periodic deadline is independent of events. Pull failures back off up to five
seconds and leave polling active; HTTP requests have bounded timeouts. A tick and
network timeout can delay a periodic pass, which runs at the next opportunity.
Without queue configuration, `watch` sleeps until the next periodic tick. Existing
`tick` invocations still run exactly once. Restart `watch` after changing settings.
Do not keep a redundant timer alongside it unless intended; the existing tick
lock still prevents concurrent decisions for one repository.

An embedded host (including a tracker's runner) must explicitly integrate
`relay::pull` and `relay::Schedule`, and call its normal `job::pass` on a wake or
periodic deadline. Merely setting queue keys does not change an existing host's
scheduler. No event content enters a tick or authorizes a write.

## Verify before enabling

Run `node --test relay/worker.test.mjs` and `cargo test --locked`. Deliver an App
webhook from GitHub's recent deliveries and observe the next watch tick without
waiting for the poll interval. Stop the relay: periodic ticks must continue.
Deployment is optional; the tests do not provision a Cloudflare account.
