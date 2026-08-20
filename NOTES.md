# Notes

Decisions and hazards worth not re-deriving. This is the *why* behind work that
is already done — kept out of `CHANGELOG.md` (which records *what* changed, per
release) and out of `TODO.md` (which is open work only).

Add to this when a choice was contested, when a default is load-bearing, or when
something bit us in a way that will bite again.

The declarative-agent platform moved to
[korps](https://github.com/EmilLindfors/korps) on 2026-08-17, and its notes went
with it — the CLI, fleets, the control plane, runtimes, the registry, container
hardening, and the LLM handler's context and tool-calling behaviour are all in
that repo's `NOTES.md` now. A few entries that both repos need are in both.

---

## Direction

**Pre-1.0 with only in-workspace consumers.** Break cleanly and fix call sites in
one PR. No deprecation shims — they are over-engineering at this stage. See
`.claude/skills/api_stability_posture`.

---

## Design rules being applied

**Ports are capabilities, not technologies**, and infrastructure lives in the
platform layer ([korps](https://github.com/EmilLindfors/korps)), never in `a2a-rs`. That is why sandboxing,
registry, and runtime are all in the platform crate. See
`.claude/rules/hexagonal_architecture.md`.

**A rule two adapters both need lives in the domain, not in each of them.**
`cancel`'s eligibility check was copy-pasted into the in-memory and sqlx stores,
and both were wrong the same way (`Working` only). One `TaskState::is_cancelable`
next to `is_terminal` fixes it once and makes the next adapter correct by
default. The general form: when two adapters implement the same port, anything
they must agree on is domain knowledge that leaked, and the duplicate is where
the drift will start.

**A task state is a promise to the client, not a label.** `EchoResponder`
attached its reply and reported `Working`, which reads as "still thinking" to
every conformant A2A client — so they waited forever on an agent that was
finished. Reference implementations get copied and get pointed at by other SDKs'
clients, so a state that is merely *approximately* right there is a bug in every
implementation downstream. The test is whether a caller acting on the state would
be acting correctly; if not, the state is wrong no matter how it is documented.

---

## Choices that had a real alternative

**Conversation memory lives in the protocol, not in the handler.** (2026-08-14)

The alternative was a `HashMap<ContextId, Vec<ChatMessage>>` on `LlmHandler`:
faster, and it keeps the tool-call rounds. It loses on the platform this repo
actually ships. `korps-fleet control-plane` recovers its fleet on startup, so agents get
restarted on purpose — in-process conversation state would reset silently on
every bounce, and would be per-replica the moment anyone runs two. Reconstructing
from `task_history` on each turn keeps the handler stateless, makes sqlx storage
give durable conversations for free, and makes restart and replica behaviour
correct by construction.

The cost is real and accepted: only A2A `Message`s round-trip, so tool-call
rounds are lost between turns. Tool results are the bulk of the tokens and are
re-derivable; the assistant's conclusion from them is in the history.

The design follows Google ADK rather than being invented here. ADK is A2A's
reference companion, and its `Session` *is* `context_id` by construction, so the
mapping is protocol-native: ADK's event log is `task_history`, `Session.state`
is a per-context scratchpad, `MemoryService` is the deferred retrieval tier.
Four invariants are common to ADK, LangGraph checkpointers, and the OpenAI
Responses API, and none of them is worth re-deriving:

1. The log is append-only and never rewritten. Compaction appends a summary and
   advances a watermark.
2. The prompt is a projection, recomputed every turn, never the source of truth.
3. Tool results are evicted before turns are. Anthropic shipped this as
   first-class server-side context editing.
4. Compaction triggers on tokens, not turn count.

**PostgreSQL is one store over two schemas, not two stores.** (2026-08-16)

The alternative was a second adapter, and the evidence against it was already in
the repo: `migrations/` carried `_postgres.sql` files for the first two
migrations and none of the last three. Two copies of a store drift, and the
drifted copy is the one nobody runs.

So the queries are written once and executed through sqlx's `Any` driver, which
picks the backend from the URL at runtime. Three constraints follow, and all of
them are worth knowing before touching this again:

- **The driver decodes text, integers, floats, booleans and bytes, and converts
  a row whole.** A `jsonb` or `timestamptz` column in a result set fails the
  read even if nothing asks for it, which is why the PostgreSQL schema stores
  JSON as `TEXT` and why every query names its columns instead of `SELECT *`.
  Nothing queries into the JSON, so `jsonb` would buy operators nothing.
- **Nothing may execute on a borrowed connection.** sqlx implements `Executor`
  for `&'c mut AnyConnection` at a single lifetime, so a future holding that
  borrow across an await can only be proved `Send` at a concrete lifetime — and
  a caller that spawns asks for every lifetime. `korps-fleet up` puts each agent on a
  `JoinSet`, so the whole construction path stops compiling, reported as
  "implementation of `Executor` is not general enough" at the spawn site, a long
  way from the cause. Execute through `&AnyPool`, always. (Checked against sqlx
  0.9: the impl has the same shape there, and 0.9 costs an `AssertSqlSafe`
  wrapper at every dynamic call site, so it buys nothing here.)
- **`sqlite::memory:` has to be given a name.** An anonymous in-memory database
  gets a fresh one invented per URL *parse*, and this driver parses per
  connection — so a ten-connection pool would be ten empty databases and the
  second query would not see the first one's table. `pooled_url` pins one.

**A database belongs to one agent.** (2026-08-16)

The schema has no agent column and no tenant column — tasks are keyed by a
caller-supplied id, contexts by a caller-supplied `contextId`, and `contexts`
records an *owner* (the authenticated principal), not an agent. So two agents on
one database share both namespaces: `ListTasks` returns the other's work, and a
`contextId` used against both reads back one mixed transcript. Delegation
produces that `contextId` by itself, which makes it the likely case rather than
the perverse one.

Hence one SQLite file per agent, or one PostgreSQL *database* per agent — a
fleet may share a server, not a database — and `fleet_conflicts` reports two
members pointing at the same URL alongside the port and id clashes. What
PostgreSQL buys today is durability across a restart onto another host, and
several instances of *one* agent behind one address. Sharing a database between
different agents is the multi-tenancy item in korps' backlog, not a configuration.

**Migrations take a lock because a fleet starts at once.** (2026-08-16)

A shared database is the reason to run PostgreSQL, so several agents migrating
concurrently is the normal case, not an edge one. Concurrent `CREATE TABLE IF
NOT EXISTS` on related tables does not no-op: it deadlocks, or the loser fails on
the catalog's unique index. This was observed, not predicted — nine concurrent
test processes made four tests flaky before the lock existed.

The lock is `pg_advisory_lock`, and it needs a session to live in. Since nothing
may hold a borrowed connection (above), the session is a *pool of one*, opened
just for the migrations and closed after — closing it releases the lock, so
there is no unlock statement to get wrong on the error path. Behind that, each
migration file is retried once on the duplicate-object and deadlock SQLSTATEs,
which covers the database being migrated by a process that holds no lock.

**OAuth2 identity comes from introspection, and there is no fallback.** (2026-08-16)

`OAuth2Authenticator` had no way to validate an opaque token, so it matched
against a list korps never filled — an agent that rejected every request —
and named the caller `oauth2:{access_token}`, which changes on refresh. RFC 7662
introspection fixes both at once: the server says whether the token is live and
returns `sub`.

A response that names no subject is an **error**. Falling back to the token there
would put the credential back in the principal id, silently, on exactly the
server that gave us nothing better — and everything an agent keys on the caller
(a conversation, a quota) would start over at the next refresh without anything
saying so. `client_id` is accepted as the subject for a client-credentials token,
because that token genuinely has no end user and the client is a stable identity.

The static token list stays for tests and is ignored once introspection is
configured: a list cannot know about a revocation, which is what introspection
is for. And `AgentConfig::validate` refuses an OAuth2 block without an
`introspection_url` rather than warning: an agent that binds its port, serves a
card and refuses every request is the same silent-wrong as the echo fallback for
an unknown handler, and the same answer applies — fail the config, name the key.

**Two authenticators for one OIDC provider, because the credential differs.**
(2026-08-17)

`OpenIdConnectAuthenticator` verifies an **ID token**: signature against the
keys discovery published, `iss`, `exp`, and `aud` naming the configured
`client_id`. `OAuth2Authenticator::with_introspection` handles an **opaque
access token**, which cannot be verified locally at all. Keycloak (or any other
provider) serves both, so the choice is made by what the caller presents, not by
who issued it. That is why `[server.auth]` in korps has `oauth2` and no
`oidc`: an agent is a resource server, and what reaches it is an access token.
The OIDC authenticator is a library surface for an embedder whose callers
forward ID tokens.

`aud` is the check doing the work in that path. An ID token is issued to one
client, and an agent that accepted any well-signed one would let every
application the user has signed into speak for them. The nonce is *not* checked,
and cannot be: it binds a token to an authentication request, and the agent
never made one.

The key set is refetched only on a token naming a key we do not have, and at
most once a minute. That failure is what a rotation looks like — and also what a
flood of junk tokens looks like, which is why the floor is there rather than a
refetch per failure.

**`load` returns the digest and the tail together.** Splitting the conversation
into an `AsyncContextHistory` port and an `AsyncContextDigest` port reads
tidier and is wrong: a digest written between the two reads leaves either a gap
or duplicated turns in the prompt, and nothing in the type system says the two
reads have to agree. One method makes the transaction boundary expressible, so
the split stays collapsed even though it puts two nouns behind one port.

**Digests append and carry a watermark rather than updating in place.** Two
concurrent turns in one context can both decide to compact. Both digests land,
`load` takes the highest `covers_through_seq`, and the loser is duplicated work
rather than corruption. Update-in-place would need a lock across an LLM call to
get the same property.

**Ownership is read before it is claimed.** (2026-08-17)

`SqlxTaskStorage` inserted the `contexts` row and then selected its owner, so
every conversation read and every state-bag read cost two statements. Ownership
is first-write and is never reassigned, which makes an existing row the whole
answer for every turn but the one that opens a context — so the read goes
first, and only its absence writes.

The one-statement version, `INSERT … ON CONFLICT DO UPDATE … RETURNING owner`,
looks better and is worse where it counts: it turns every read into a row
update, leaving a dead tuple per turn on PostgreSQL and firing the `contexts`
update trigger on SQLite.

The claiming path reads back rather than trusting `rows_affected` to say whether
the insert was ours. Two callers can open one context in the same instant, and a
driver that counted an ignored insert as a row would admit the loser to a
conversation it does not own. One statement is not worth resting an
authorization decision on how each backend reports a no-op.

**Wiring conversation memory makes `context_id` an authorization boundary.**
Worth stating because it changes what an existing field means. Nothing reads by
context today, so a guessed `context_id` gets you nothing; once a handler
projects a conversation from it, presenting someone else's id reads their
conversation into your prompt. Hence `contexts.owner`, taken from the
`Authenticator` principal and checked on load. This is what pulled the deferred
multi-tenancy theme forward, and it is not optional to defer again.

**The caller travels in one value, not one more parameter.** Getting the
principal from the auth middleware to the message handler meant changing
`AsyncMessageHandler::process_message`, which already took `session_id:
Option<&str>` that almost nobody read. A fourth positional argument was the
smaller diff and the worse signature; `port::RequestContext` replaces the
`session_id` parameter and carries both facts. Two reasons it is a struct: the
things a handler wants to know about *who is asking* arrive together and grow
together — `tenant` is the next one, and it costs no signature change now — and
a bare `Option<&str>` next to another `Option<&str>` is the shape where an
argument silently lands in the wrong slot.

The principal itself rides in the HTTP request extensions between the middleware
and the transport adapter, because that is the only channel `connectrpc` gives a
tower layer (it moves `parts.extensions` onto its `Context` verbatim). Note the
name collides with `rmcp::service::RequestContext`; `a2a-mcp` aliases one of the
two wherever both are in scope.

**The state bag is a row per key, and its scope lives in the key.** (2026-08-17)

Three decisions, each with a real alternative that was tried on paper first.

*A table, not the `contexts.state` column 005 created.* A JSON document per
context needs read-modify-write to add one key, and two turns of one context can
run at once — the same fact that makes `context_digests` append-only. The loser
of that race loses its write, silently. A row per key upserts and has nothing to
lose. The column was dropped in 006 rather than left: nothing had ever written
it, and a column named `state` next to a state table is a schema that lies.

*The scope is a key prefix, not a column the caller passes.* Taken from Google
ADK, which is A2A's reference companion, so the spelling is one a model has
likely seen. It also puts the scope where the model reads it back — in the
prompt, in a `forget` call, in the store — rather than in a parameter it has to
be told about separately. The cost is that `app:tone` and `user:tone` differ by
five characters, which is why an unrecognized prefix is a refusal naming the
three that exist rather than an ordinary key.

*No `app:` scope, and `temp:` stores nothing.* ADK has four; two of them do not
survive the trip. `app:` is agent-wide, so it has no owner to check a caller
against — which makes it a config value the operator writes, not something one
caller's model can set for every other caller. `temp:` is kept precisely because
it stores nothing: without the prefix being parsed, `temp:draft` would be an
ordinary key outliving the turn under a name promising the opposite.

The `user:` scope is what earns the feature. A per-context bag is close to
redundant with the transcript — everything in it was said in this context — and
its value is surviving compaction. A `user:` key is filed under the principal, so
it reaches a conversation that has never seen it, which nothing else in the
memory design does. That is also why a `user:` write with no principal is an
error rather than a fallback to context scope: the fallback would keep the value
and break the promise its name makes.

**`a2acli send` waits by default.** An agent may answer synchronously (the
scaffolded `echo` handler completes in the same call) or asynchronously (the
`llm` handler returns `working` and delivers the reply on a later `get`). A
client that printed only what `send` returned showed nothing at all for the
second kind — which is what the `llm` and `orchestrator` templates scaffold. So
`send` waits for a terminal *or interrupted* state (`input-required` and
`auth-required` are stops too: the agent is waiting on the caller). `--no-wait`
is the escape hatch, not the default, because "send a message to an agent" means
"and show me what it said".

Since blocking `SendMessage` landed the wait is mostly the *server's*: `send`
leaves `return_immediately` at its spec default, so a conformant agent hands
back a settled task and the client-side poll loop never runs. The loop stays as
the fallback for an agent that ignores the flag. `--no-wait` has to switch off
both halves — declining to poll while the server blocks anyway just relocates
the wait somewhere the flag cannot reach. Making that fallback a subscriber
rather than a poll is open work, not a constraint (the reason it polled —
subscriptions never closed — is gone); see `TODO.md`.

---

### Where a task id comes from

Both `task_id` and `context_id` are optional on a client message, and proto3
gives an omitted string field the value `""` — so "absent" and "empty" are the
same thing on the wire, and neither transport can tell them apart. The rule
lives in `TaskService`, not in the adapters and not in the handlers:

- No task id: generate one, and a context id with it.
- A task id naming a task we hold: that task's context wins. A caller that also
  supplied a different one gets a `ValidationError` rather than having its
  message re-homed — the spec requires the two to match, and silently moving a
  message between conversations is the failure that is hardest to notice.
- A task id we have not seen: the client picked it; treat the task as new.

The resolved ids are stamped onto the message before it reaches the handler, so
task history carries them and a handler still receives a resolved `&str`. Two
consequences worth keeping in mind. The lookup costs one storage read per send
that names a task, which is what buys the context inference. And a client cannot
be given back a task id it did not send unless it reads the response — which is
why `Transport::send_task_message` takes `Option<&str>` and callers use
`task.id` afterwards, rather than the id they passed in.

---

## Hazards that will recur

**Cargo silently skips a binary whose `required-features` are not all enabled.**
It does not warn on `cargo install`; you simply do not get the binary. This is
why `required-features` for `a2a` must stay a subset of `default`, pinned by a
test. It had already broken three ways: `cargo install a2a-agents` produced only
the reimbursement demo, the release-binaries workflow built without `llm` and
`schema`, and the Dockerfile drifted from the list.

**`rust-version` is never checked against your dependency graph.** Cargo errors
when the *toolchain* is below a dependency's floor, never when the declared
`rust-version` is — so a false MSRV claim surfaces only somewhere exotic (for us,
a container build on a pinned builder image). The number now lives once in
`[workspace.package]` and is set to the toolchain we actually build and test
with, not the bare minimum, so it is a claim we exercise. It stops being proven
the day stable moves past it; that is when an MSRV CI job earns its place.

**An empty text part is indistinguishable from a file part on the wire.** proto3
omits a default value, so `Part::text("")` serializes as `{}` and a client
renders it as `[non-text content]` — a line about binary data the agent never
produced. Anything that builds a part from a computed string has to decide what
an empty one *means* before sending it; `LlmHandler` decided it means failure,
because a task that finishes with nothing to show is a failed task whatever the
transport thinks. The class is wider than that one call site: the same silence
awaits any artifact, status message, or tool result assembled from a string that
can come back empty.

**Asking a model to think is a request, never a guarantee.** `Reasoning::Off`
sends OpenRouter's `enabled: false`; a model with no way to turn reasoning off
may ignore it, and reasoning tokens are billed even when the text is not
returned. Verified accepted on `deepseek-v4-flash`, `minimax-m3`, and `glm-5.2`
on 2026-08-13 — which is evidence about those three models on that day, not a
property of the setting. Treat the config as "what we asked for" and the bill as
the source of truth.

**A proto3 `bool` makes the spec's default the one you have to write code for.**
`SendMessageConfiguration.return_immediately` defaults to `false`, and `false`
is the *demanding* branch — it obliges the server to block until the task
settles. So the request that exercises it is the empty one: a client that sends
no configuration at all, which is what every official SDK does. The field read
as an opt-*in* to blocking and was in fact an opt-*out*, which is why it sat
unimplemented while looking harmless. Whenever a proto3 scalar carries policy,
check which way the zero value points before deciding a field is optional —
and model it in Rust as an enum (`SendCompletion::{WhenSettled, WhenCreated}`),
never the bare `bool`, so the polarity cannot be misread at a call site.

**A "MUST wait" needs a bound, and the bound must return, not error.**
`send_message` gives up after 25s and returns the task *unsettled*. Returning an
error instead would deny the caller the one thing that makes the situation
recoverable — the task id — and `WORKING` is not a lie: the agent genuinely has
not finished.

**Making a call block is a change to everyone who already had their own
deadline.** Turning on the spec's blocking `SendMessage` quietly broke the two
callers that ran their own follow-up: agent-as-tool delegation polls to a
400ms deadline, and `a2acli send --no-wait` skips its poll loop — both now sat
behind the peer's 25s wait, because two nested waits do not compose, the longer
one just wins. The tell was a test going from 0.5s to 25.5s while still
passing. So a caller that manages its own completion must be able to say
`WhenCreated`, which is why the flag had to reach the client `Transport` port
and not just the server. Whenever you add blocking to a call, go find every
caller that already had a timeout and ask which one is supposed to be in charge.

**A server-side wait must expire before the client's request timeout.** The
first cut used 30s, which is exactly what `JsonRpcClient`, `HttpClient` and
`a2acli --timeout` all default to — a dead heat, in which a slow agent yields a
*transport error* rather than the unsettled task the bound exists to hand back.
The blocking wait had converted "agent is slow" into "connection failed", which
is strictly worse than the behaviour it replaced. Hence 25s: the server must
lose that race on purpose. Any pair of nested timeouts needs the inner one
strictly shorter, and the two here live in different crates, so the constant
carries the invariant in its doc comment.

**Test a timeout's *default* on tokio's virtual clock, not the wall clock.** The
one test that has to exercise the real 25s budget — rather than a short one
injected via `with_send_wait` — spent 25s of every CI run watching a timer
expire. `#[tokio::test(start_paused = true)]` advances the clock whenever
nothing is runnable, so the same assertion costs 0.06s; measure with
`tokio::time::Instant`, since the paused clock does not move `std`'s. Two
catches. It needs tokio's `test-util` feature, and *workspace feature
unification will hide a missing one*: `cargo test --workspace --all-features`
compiled it via another crate's dev-dep while `cargo test -p a2a-rs` did not, so
the crate that uses a dev-dep feature must name it itself. And a virtual clock
removes the lower bound for free — a wait that was skipped entirely reports 0s
and passes an upper-bound-only assertion — so assert both ends.

**`take_while`/`scan` cannot end a stream on its own last item.** Both decide to
stop only when the *next* item arrives — so terminating a subscription "after
the terminal event" with either one hangs on exactly the event it is supposed to
close on, because after a terminal state no next item ever arrives and the
broadcast receiver simply parks. The shape that works is `unfold` carrying the
inner stream in an `Option` and dropping it on the settling item: the next poll
ends without touching the inner stream, and the drop is also what releases the
subscription. The general form: "inclusive take" is not a combinator futures
gives you, and reaching for the exclusive one quietly inverts the bug.

**Distinguish "last piece of a thing" from "the thing is over".** The predicate
that ends a subscription cannot be the one that answers "is this final", because
an artifact's `last_chunk` marks the end of *that artifact* while the task keeps
working and may emit several more. The old `UpdateEvent::is_final` merged both
and would have truncated a streaming response mid-task had anything used it.
Name such predicates after the question the caller is actually asking
(`settles_task`), not after the field they happen to read.

**`EnvFilter::from_default_env()` enables nothing when `RUST_LOG` is unset.** It
made `korps run` start a server and print absolutely nothing. Any new binary needs
an explicit fallback filter.

**rustls ignores `SSL_CERT_FILE`, so a TLS-intercepting proxy breaks every
outbound call and nothing outside the binary can fix it.** With reqwest's
`rustls-tls` alone the trust anchors are the compiled-in webpki roots; the
environment variables every other tool honours (`SSL_CERT_FILE`,
`REQUESTS_CA_BUNDLE`) do nothing. Found in a Docker Sandbox that re-signed all
egress with a per-machine "Docker Sandboxes Proxy CA": every LLM call died as
`error sending request for url (...)` with no cause attached, indistinguishable
from the network being down, while `curl` in the same shell worked — which is the
tell, since curl reads the OS store. The workspace `reqwest` dependency now
enables `rustls-tls-native-roots` alongside `rustls-tls`; reqwest's two root
stores are **additive**, so webpki stays the floor and the OS store adds whatever
CA the operator installed.

Worth knowing how narrow the reproduction was: interception is a *policy*, not a
property of sandboxing. `sbx` v0.38 under the `balanced` policy tunnels allowed
hosts, so agents see real certificates and even the unfixed binary works; the
old bundled v0.12 plugin intercepted everything. The fix is not really about
Docker Sandboxes at all — a corporate MITM gateway is the common case, and it
fails the same way.

**A `Network error` variant that drops the source is a debugging dead end.**
The proxy-CA failure above reported only `error sending request for url (...)`.
`reqwest::Error`'s `Display` deliberately omits the source chain, so the
certificate error underneath was invisible; diagnosing it needed the *sandbox's*
network log to prove the request had reached the proxy at all. Anywhere a
`reqwest::Error` is wrapped, walk `std::error::Error::source()` into the message.

Fixed 2026-08-16 with a `describe_transport_error` in each crate that flattens
one into a string (`a2a_llm`, `a2a_rs::adapter::error`). Two copies rather than
one shared helper on purpose: `a2a-llm` does not depend on `a2a-rs` and should
not start doing so for eight lines. It takes
`&dyn Error` rather than `&reqwest::Error` so the SSE stream's
`EventStreamError` wrapper is covered by the same rule — a wrapper is exactly
where a cause chain gets lost.

---

## Release pipeline

release-plz is PR-driven on push to master. Merging the "chore: release" PR is
what ships; tags are an *output*, never pushed by hand.

- Each crate versions and tags **independently** (`{package}-v{version}`) and
  they have diverged — check the crate's own `Cargo.toml`, never assume a shared
  number. There is no umbrella `v*` tag; carrying both broke the changelog
  compare-links at 0.3.0, which is why `release-plz.toml` pins the tag template
  and uses the `release_link` variable.
- **Per-crate `CHANGELOG.md` files are generated** by release-plz from
  conventional commits. Do not hand-edit them.
- **The root `CHANGELOG.md` is hand-written.** It is the workspace-level record
  and the place to write prose about a change; that is where the `[Unreleased]`
  section lives.
- **The publish job runs on every push to master, with no commit-subject gate.**
  `release-plz release` is a no-op unless a crate's version is ahead of
  crates.io, so ordinary merges publish nothing and the merge button used for
  the release PR no longer matters. The gate this replaced tried to recognize
  the merged release PR by subject, and had to match both a squash merge
  (`chore: release`) and a merge commit (`Merge pull request #N from
  OWNER/release-plz-…`). On 2026-08-15 it skipped the publish twice: the bumps
  landed on master, crates.io stayed on the previous release, no tags were
  written, and the workflow still reported success. It also meant one transient
  network failure stranded the release until someone dispatched the job by hand.
  What actually prevents an uncontrolled publish is `guard-version-bump.yml`,
  which fails any PR editing a package `version = "…"` outside the release PR,
  plus branch protection requiring every change to arrive through a PR.
- **A skipped publish looks like release-plz re-proposing the same bump.**
  Version numbers come from the last *published* release, not from the
  `Cargo.toml` on master, so an unpublished bump makes every later push open a
  release PR proposing that identical bump again. If a release PR keeps
  reappearing with versions master already carries, check crates.io and the tags
  before merging it again — merging changes nothing. Now that the release job is
  ungated the next push to master retries the publish by itself; a manual
  `workflow_dispatch` of `Release-plz` forces it immediately.
- **The binary build is a dependent job, not a tag trigger.** `release-plz.yml`
  reads the release command's `releases` output (one object per published crate,
  each with `package_name`/`version`/`tag`), pulls the `a2acli` tag out of it,
  and calls `release-binaries.yml` as a reusable workflow in the same run. The
  old `on: push: tags: a2acli-v*` never fired — release-plz pushes that tag with
  `GITHUB_TOKEN`, and a token-pushed tag does not start a workflow — which is why
  every release used to need a manual dispatch. `workflow_dispatch` is still
  there for rebuilding assets on an existing tag.
- **CI on the release PR needs a real credential; nothing else does.** A PR
  opened with the default `GITHUB_TOKEN` starts no workflows, so release PRs ran
  with zero checks — the bumped versions and the regenerated `Cargo.lock` went
  in unverified. Both release-plz jobs now authenticate with the `RELEASE_TOKEN`
  secret, a PAT with Contents + Pull requests write, via
  `${{ secrets.RELEASE_TOKEN || secrets.GITHUB_TOKEN }}`. The fallback means
  removing the secret degrades the pipeline to bot-authored PRs rather than
  breaking it. If `release-pr` starts failing with a 403 on `POST /pulls`, the
  PAT lost Pull requests write or expired — that is the first thing to check.
