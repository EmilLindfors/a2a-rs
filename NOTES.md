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

**The stream log is a port, not a field on the streaming handler.** (2026-08-23)

`InMemoryStreamingHandler` owned both halves of streaming: who is listening, and
what has been said. The first is this process's business and a restart may
forget it. The second a resuming client still needs, and forgetting it is what
made ids restart at 1 — handing a reconnecting client ids it had already seen,
for different events.

The alternative was a second streaming-handler adapter that talked to a
database. That would have duplicated the fan-out, because live readers still
need an in-process broadcast channel whichever store the events are in. Making
the log a port instead leaves one fan-out (`StreamingFanout<L>`) and makes
durability a matter of which log it was given.

The id is assigned by the log, inside the insert. A counter in the process
cannot be right across a restart, and reading `MAX(id)` first and binding it
leaves a window where two appends pick the same number; computing it in the
statement means the primary key settles a collision rather than one event
overwriting another.

`task_events` has no foreign key to `tasks`. The two are separate ports and need
not be the same database, and an append rejected because the task row had not
landed yet would drop an event on the one path whose job is not to drop events.
The cost is that nothing reclaims the rows implicitly, so `delete_context` names
the table like every other one it sweeps.

**A replay that cannot cover the gap is dropped, not sent.** (2026-08-23)

A bounded log discards its oldest events, so a client disconnected long enough
asks for a tail that is no longer whole. What is left starts partway through the
gap. Sending it looks like a successful resume and is not: those events are
older than the task snapshot the service sends ahead of the stream, so a status
update re-applies stale state and an artifact update with `append` duplicates
content. `Replay::complete` is what lets the fan-out tell the two cases apart,
and the incomplete one streams live from the snapshot — which is what a client
that never sent `Last-Event-ID` gets, and is coherent.

**A ConnectRPC event id rides in the payload's metadata, and only when asked
for.** (2026-08-23)

The SSE transports carry the per-task event id in the W3C `id:` field, which is
protocol-level and invisible to a client that does not read it. ConnectRPC has
no equivalent: `StreamResponse` is a payload oneof with no room for anything
else, and a per-message value cannot go in a header or a trailer. So the id goes
into `TaskStatusUpdateEvent.metadata` / `TaskArtifactUpdateEvent.metadata` under
`a2a_rs_event_id`.

That is a change to the agent's own metadata bag, not an inert field, so it is
gated on the `a2a-rs-event-ids` request header: a client that does not send it
gets the bytes the spec describes, and our client strips the key before the
event reaches the caller. The alternative — stamp it for everyone — would put a
key nobody wrote into every third-party SDK's view of the payload, and the id is
only useful to a client that knows to look.

The gate cannot be "the client is resuming". Resuming needs the id of the last
event received *before* the disconnect, so ids have to flow from the first
event; a header sent only on reconnect would arrive one connection too late.

The id is a string. `google.protobuf.Struct` numbers are doubles and an event id
is a `u64`, so a number would round past 2^53.

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
with, not the bare minimum, so it is a claim we exercise. Stable moved to 1.98 on
2026-08-18 and stopped exercising it, so `rust.yml` gained a job pinned to 1.96
— library targets only, since the dev-dependencies behind `--all-targets` have
floors of their own. The number now lives in two places that have to move
together: `[workspace.package] rust-version` and that job's toolchain.

**An empty text part is indistinguishable from a file part on the wire.** proto3
omits a default value, so `Part::text("")` serializes as `{}` and a client
renders it as `[non-text content]` — a line about binary data the agent never
produced. Anything that builds a part from a computed string has to decide what
an empty one *means* before sending it; `LlmHandler` decided it means failure,
because a task that finishes with nothing to show is a failed task whatever the
transport thinks. The class is wider than that one call site: the same silence
awaits any artifact, status message, or tool result assembled from a string that
can come back empty.

**Ask the endpoint which reasoning parameter a model takes; do not keep a
table.** Support turns on the *model*, not the provider: `reasoning_effort` is a
400 on `gpt-4o-mini` and mandatory on `gpt-5-pro`, and Google's docs list
`thinkingLevel` for the 2.5 generation while the field reports say those models
take only `thinkingBudget`. A model-name table is explicit and reportable and
goes stale with every release — the `TODO.md` entry describing the providers had
itself gone stale before it was implemented. So the parameter is sent, a refusal
is recognized from the 400 that names the field, and the request is retried once
without it. A 400 generated nothing, so the recovery costs a round trip and no
tokens.

Two details make it safe rather than clever. The refusal is remembered only after
the retry *succeeded*: a 400 naming the field can be about something else, and
remembering that would disable reasoning for the rest of the process over an
unrelated failure. And only a 400 counts — a 5xx that happens to name the field
is an outage, and treating it as a refusal would leave the model thinking at its
default long after the outage ended.

What this costs is that the plan is no longer known before the run.
`ReasoningPlan::Attempted` is that state, and a report has to be able to say
"sent, and the model has the last word" — which is why `unsupported()` now
answers only for the drops decided by selection. There is one of those left: a
token budget on OpenAI, whose Chat Completions API has no field for one at all.

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

**Subscribing to a finished task is an error, not an empty subscription.**
`a2a.proto` says `SubscribeToTask` returns `UnsupportedOperationError` on a
terminal task, and we answered with the snapshot plus an empty stream instead —
defensible in isolation (nothing further can ever be broadcast for it) and
wrong on the wire, because a subscription that opens and closes without an
event is indistinguishable from an agent that has not spoken yet. A caller that
only reads events, which is what subscribing is for, saw nothing at all.
Changed 2026-08-21. Two things fell out of it. The check stays conditional on
`from_event_id` being unset — resuming after a disconnect on a task that has
since finished is exactly when the replay buffer matters, so refusing there
would break the call that exists to recover. And `a2acli`'s wait now takes the
polling fallback whenever the task settled before the subscription opened,
which is why `poll_until_settled` reads before it sleeps: sleeping first
charged every fast agent an interval it did not need.

**An error answering a streaming call is not a stream, and status codes will
not tell you.** A JSON-RPC error is HTTP 200 with a JSON body, so a client that
gates only on `status().is_success()` hands it to its SSE reader, which finds
no frames and reports a subscription that ended. The caller sees silence and
exits 0 — the one response the server took care to explain is the one that gets
thrown away. Both CLIs did this: ours against the upstream agent, and the
official `a2acli` against a server with no streaming backend. Wherever a call
can answer either with a stream or with an error over the same 200, the content
type is the discriminator; and a response carrying none has to keep being read
as a stream, because reading a body to inspect it hangs on a stream that has
nothing to say yet.

**ProtoJSON omits an empty REQUIRED field, so the default value is off the
wire.** `AgentSkill.tags` is `REQUIRED` in `a2a.proto` and `SimpleAgentInfo`
built every skill with an empty one, so the served card had no `tags` key —
and the official `a2acli`, which models it as non-optional, refused the card.
It fetches the card before every call, so *every* subcommand failed with
"error decoding response body", which reads like a broken server rather than
one missing field. The class is wider than that field: any REQUIRED string,
list or message left at its default disappears the same way, and the failure
always lands on the consumer. Where a field is REQUIRED, make the API demand a
value rather than defaulting one — a builder that accepts no tags is a builder
that produces uncallable agents. `agent_card_required_fields_test` pins the
card's list.

**matchit cannot route the spec's `{id}:verb` paths, and the workaround has to
live in the handler.** `POST /tasks/{id=*}:cancel` and `GET
/tasks/{id=*}:subscribe` put the verb on the same segment as the parameter;
`{id}` already claims that segment, so the literal form conflicts with
`/tasks/{id}` and axum rejects it. Serving only the slash aliases
(`/tasks/{id}/cancel`) is not a substitute — official clients send the
canonical form, and it *matches* `/tasks/{id}`, so the failures are `405` on
cancel and, worse, a task returned as JSON to a subscriber waiting on an event
stream. The verb therefore arrives inside the captured id and is split off
there. A rewriting middleware would be tidier and does not work: axum layers
run after routing, and wrapping the router in an outer service stops it being a
`Router` that `merge` accepts.

**A stream that can only be closed by an event nobody will send stays open.**
`send_streaming_message` ended its stream on the settling update — correct for
an agent that works in the background, and a hang for one that finishes inside
`process_message`, which returns a `COMPLETED` task having broadcast nothing.
The subscription is a broadcast receiver: with the sender alive it parks
forever rather than ending. Whenever a stream's termination depends on an event
from elsewhere, ask what closes it when that elsewhere has already finished —
`subscribe` had the answer (short-circuit a terminal task) and the send path
did not.

**A `oneshot` test that lets the router own the adapter cannot prove a stream
ends.** The first version of the regression test for the hang above passed
without the fix. `rest_router(a)` moves the only `Arc`, so the router — and the
streaming handler, and its broadcast sender — drops when the response is
produced, which ends the receiver by itself. A served agent outlives its
responses. Hold the adapter across the body read (`rest_router(a.clone())`), or
the assertion is about the test's own drop order.

**A test that sleeps for a server to start has two bugs, and polling fixes one.**
`authenticated_principal_test` bound `127.0.0.1:8199` and slept 200ms; it failed
once under a full workspace run and passed alone. Either cause explains that —
the sleep being short under load, or another test binary holding the port — and
polling the socket only answers the first. Binding the listener in the test and
handing it to `HttpServer::serve_on` answers both: port 0 picks whatever is
free, `local_addr()` says which, and the kernel queues connections from the bind
onward, so a client can connect before the serve future is polled. There is no
readiness window left to wait out. A `free_port()` helper that binds and
releases is the same trap with a smaller window — the port is free when it is
read and can be taken before it is claimed.

**Retention measures idleness from writes, and a read does not count.**
`SqlxTaskStorage` gets its timestamps from `updated_at` columns, which a `SELECT`
does not touch; `InMemoryTaskStorage` had no timestamps at all and needed a map
of its own. Making that map bump on reads was tempting and would have made the
two stores disagree about what "idle" means — two retention policies wearing one
name. So both expire a context that is only ever read, and the rule is on
`RetentionPolicy` rather than in either adapter. The cost is real: a long-lived
`user:` fact that is read on every turn and rewritten never will expire. That is
the price of not recording reads, which is the thing worth not paying for.

**SQLite's `foreign_keys` is a per-connection pragma, and the cascades depend on
it.** SQLite defaults it *off*, so `ON DELETE CASCADE` in `migrations/sqlite/` is
decoration until each connection says otherwise. It has been on all along —
sqlx's `SqliteConnectOptions` defaults it true and the `Any` driver inherits
that — but nothing in this repo said so, which made it a guarantee borrowed from
a dependency's default for a schema that cannot work without it. Set explicitly
in `after_connect` as of 2026-08-21, with a test that reads the pragma back off a
pooled connection and one that deletes a task and checks its history went too.
There is no other place to put it: the `Any` driver parses URLs itself and
rejects SQLite's own parameters, so `?foreign_keys=…` does not reach the driver
in either direction.

**A sweep counts what both stores can count.** `Swept::messages` first came off
`rows_affected` on the `task_history` delete, and the SQLite store reported three
where the in-memory store reported two — `task_history` holds status transitions
as well as messages, and the in-memory conversation log holds only messages. The
number was accurate about rows and useless for comparing the adapters. It is now
a `COUNT(*) … WHERE message IS NOT NULL` taken inside the sweep transaction,
which is the same predicate `load` reads the conversation by. Where two adapters
report a number under one name, the name has to be a quantity they both have.

**A sweep is global, so a test that sweeps needs a database to itself.** The
in-memory and SQLite cases get one for free — a fresh store is a fresh database.
PostgreSQL is a server, and eight cases sweeping one database delete each other's
contexts and then disagree about the counts. `retention_test.rs` creates a
database per case, named after the case, so a run reuses the same eight rather
than accumulating them and no case can drop one another is using. The
alternative — unique ids per case and assertions that count only their own rows —
was rejected because `Swept` counts what the sweep deleted, and the sweep does
not know whose rows they were.

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
