# Notes

Decisions and hazards worth not re-deriving. This is the *why* behind work that
is already done — kept out of `CHANGELOG.md` (which records *what* changed, per
release) and out of `TODO.md` (which is open work only).

Add to this when a choice was contested, when a default is load-bearing, or when
something bit us in a way that will bite again.

---

## Direction

**The `a2a` CLI is the front door; Terraform is deferred.** (2026-07-25)

The end goal was framed as *write HCL, get running agents*, which put the
Terraform provider on the critical path. It is now *install `a2a`, get running
agents* — a good standalone CLI, no Go and no Terraform in the loop.

This is the better order regardless of preference: HCL is a front-end onto the
same control plane and the same config schema, so anything the standalone path
settles — config strictness, fleet composition, lifecycle commands — is work the
provider would otherwise have had to invent and then re-litigate. The provider
becomes a thin passthrough client over what the CLI already proves out.

`terraform-provider-a2aagent/` is parked WIP and is not part of the supported
path. Its README still describes it as the source of truth for agent
definitions; that is stale.

**Pre-1.0 with only in-workspace consumers.** Break cleanly and fix call sites in
one PR. No deprecation shims — they are over-engineering at this stage. See
`.claude/skills/api_stability_posture`.

---

## Design rules being applied

These recur; each was arrived at from a concrete failure.

**No defaulted trait method whose default would be a silent lie.** `AgentRuntime`
has no default `recover` and no default `logs`. A default `recover` returning
"nothing running" would have been exactly the silent-wrong the method exists to
remove, and a default `logs` returning an empty list would tell an operator "the
agent printed nothing" when the truth is "this backend does not record output" —
opposite places to go looking. Hence `Recovered::{Adopted, Ephemeral}` and
`RuntimeError::Unsupported` (→ HTTP 501): every adapter must *state* which
answer it is giving. Call this the honesty rule; apply it to the next port.

**Gate policy with a type, not a convention.** `ControlPlane::prepare(raw_toml)`
returns a `PreparedDeploy` that is the only thing `deploy` accepts, so the
secrets-allowlist check cannot be skipped by a caller who forgets. Likewise
`ControlPlaneAuth` is a *required* parameter of `control_plane_router` —
`ControlPlaneAuth::Disabled` has to be written out, so an unauthenticated
control plane cannot happen by omission. (`PreparedDeploy`'s `Debug` is
hand-written to print only the id: the prepared config holds *expanded* secret
values.)

**Check raw text before parsing, when parsing would leak.** The env allowlist runs
against the raw TOML, because parsing expands `${VAR}` and reports set vs. unset
differently — so a config naming a forbidden secret would otherwise be a probe
for which secrets the control plane holds.

**Ports are capabilities, not technologies**, and infrastructure lives in the
platform layer (`a2a-agents`), never in `a2a-rs`. That is why sandboxing,
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

**Hide, never discard.** `a2a ps` omits stopped agents, but the runtime keeps
their entries and `a2a logs` still answers for them — the log of an agent that
died is the one that matters most. `ListFilter::{Live, All}` is the split, and
`docker ps` / `docker ps -a` is the precedent. The rule generalizes: when a
listing is noisy, filter the *view*; pruning the record throws away the evidence.

**The same invariant has to hold wherever it can be violated.** `a2a up` checked
its fleet file for port clashes; the control plane did not check a deploy against
the live fleet, so the second agent on a port reported `ok … healthy` — its
process lost the bind race while the card probe, which knows only the endpoint,
answered from the agent that won. A pre-flight check that runs in one entry point
and not the other is not a check. `ControlPlane::deploy` now runs it too.

**Reports go to stdout, events go to `tracing` (on stderr).** `validate`,
`doctor`, `deploy`, `ps`, `logs`, and the `run` banner produce output a person
reads and a script greps — no timestamps, levels, or module paths. Long-running
commands log, because they emit events over time. Two bugs came from getting
this wrong: `tracing` defaulted to stdout and mixed into the reports
(`a2a validate > report.txt`), and ANSI colour was emitted unconditionally, so
every captured agent log had escape codes baked in.

---

## Choices that had a real alternative

**Conversation memory lives in the protocol, not in the handler.** (2026-08-14)

The alternative was a `HashMap<ContextId, Vec<ChatMessage>>` on `LlmHandler`:
faster, and it keeps the tool-call rounds. It loses on the platform this repo
actually ships. `a2a control-plane` recovers its fleet on startup, so agents get
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

**The load window has to be wider than the window compaction may not touch.**
`keep_recent_turns` means two things that pull against each other: how much of a
conversation is read back for a prompt, and how much of it compaction is
forbidden to summarize. Using the one number for both is a deadlock — the load
returns exactly the protected window, so there is never anything older to fold
and compaction can never fire. Hence `LOAD_WINDOWS`: a load reaches back several
windows, and only what falls outside the last `keep_recent_turns` is summarized.
In steady state the digest watermark bounds the read long before the limit does;
the limit is the backstop for a conversation that has not compacted yet.

The related trap is the watermark. A digest has to cover exactly what was
summarized, not everything that was loaded — otherwise the recent turns sit
behind a summary that does not describe them, and they vanish from the next
prompt while appearing to have been preserved.

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

**An agent's own image is started with no command; the base image is not.** Both
get the same mount (`/etc/agent.toml`), the same `A2A_CONFIG` naming it, `HOST`,
the published port and the allow-listed variables. The difference is that
`ContainerRuntime` appends `run --config /etc/agent.toml` only for the base
image, because that argv belongs to *this* project's binary. The alternative —
one argv for every image — makes "accepts `a2a run --config`" part of the
contract, which is a constraint on someone else's `ENTRYPOINT` that buys nothing
and fails at run time, inside a container, rather than at deploy time.
`A2A_CONFIG` exists so an image need not hard-code the mount path.

**A runtime that cannot honour an image refuses the deploy.** `LocalProcessRuntime`
returns `RuntimeError::Unsupported` for a spec carrying an image instead of
spawning `a2a run` on the config. The fallback is worse than it looks: for an
agent whose handler happens to be built in, it provisions something that starts,
answers, and passes its health probe while being a different agent than the one
deployed — the failure the whole `Recovered::Ephemeral` / `logs`-`Unsupported`
line of reasoning exists to avoid. Same rule as those: say "I cannot", never
answer with something else.

**An unknown handler is a hard error now that images exist.** `a2a run` used to
fall back to echo. That was defensible only while there was nothing else to
offer; with `[runtime] image` there is a real answer, so the error names it. Note
what this fixed: the fallback made a config with a typo'd handler name behave
exactly like a config that asked for echo, and the only difference was one
`warn!` on a stream supervisors do not read.

**`doctor` stops at the image.** A config naming an image gets its port checks
and nothing else — no handler verdict, no model provider, no MCP command probe.
Those questions are all about a binary this machine does not have and cannot
look inside; answering them from *this* build produces confident statements
about something that will not run. This is the same rule as running the code
rather than re-deriving it: when the code is in an image, all doctor can report
is which image.

**A configured LLM provider that cannot be built stops the run.** The
alternative was what it did before: warn, fall back to the non-LLM handler, keep
serving. That is worse for the case it actually covers — a typo.
`OPENROUTER_REASONING=hgih` or `provider = "opnrouter"` is not a request to run
without a model, it is the same request with a mistake in it, and the agent that
came up answered every message with a stub while reporting healthy. Two things
follow from the split:

- Nothing configured at all still runs the fallback. That is a setup someone
  chose (CI, a demo with no keys), not a mistake, so it stays `Ok(None)`.
- A failing provider does not fall through to the next in the cascade. A broken
  `OPENROUTER_API_KEY` promoting whatever else is exported changes which model
  answers and what it costs.

`a2a doctor` calls `provider_from_settings` / `provider_from_env` rather than
checking which variables are set, so its verdict is the run's verdict. The same
applies to anything else `doctor` checks: run the code, don't re-derive it.

**Reasoning is configured in `[llm]`, beside the model — not in
`[handler.llm]`.** The obvious place was the handler's own options, next to
`system_prompt` and `max_tool_rounds`, and it is the wrong one: how hard to think
is priced like the model and only makes sense against a particular model, while
the handler serves whichever model it is handed. Putting it beside `model` also
makes it reach *every* caller of the provider — the generic handler, a custom
Rust handler, the MCP bridge — instead of only the one handler that remembered
to read the field. A request may still override it (`LlmRequest::reasoning`), so
a caller with a reason (`complex_agent` streams its thinking on purpose) is not
overruled by config.

The related deletion: `LlmProvider::supports_reasoning` is gone. It answered
"does this *endpoint* speak the reasoning parameter", and every caller used it
as "should this *model* think", which is how `LlmHandler` came to bill high
effort on a flash model and `complex_agent` came to sniff `OPENROUTER_API_KEY`
from the environment. A capability that only the adapter can answer belongs
inside the adapter: callers now ask for what they want, and a provider whose
endpoint has no such field sends the request without it. Asking is therefore
always safe, which is the property that removes the sniffing.

**A dropped `reasoning` is a value, not a log line.** `SelectedLlm.reasoning` is
`ReasoningPlan { Unset, Sent(_), Unsupported(_) }` rather than
`Option<Reasoning>`, because a provider that discards the setting and a config
that never set it used to arrive as the same `None` — leaving `a2a doctor` with
nothing to report and the difference showing up on the bill. `a2a run` still
warns; a report reads the field.

Sending `reasoning` on OpenAI and Gemini stayed open for a reason worth keeping:
both mappings are questions about the **model**, not the provider. OpenAI's
`reasoning_effort` is rejected outright by models that do not reason, and the
default here is `gpt-4o-mini`, so dispatching on provider kind breaks the common
case; Gemini's thinking budget is spelled differently across model generations
and its minimum is model-dependent, so `Off` is expressible on some models and
not others. Either way the adapter needs a model-name list, which goes stale
with every release — that is the cost to weigh, not the request field.

**Giving up is `failed`, not `input-required`.** The alternative was real: the
model gathered tool results before running out of rounds, so "I got partway, a
human could unblock me" describes what happened. It is still the wrong state.
`input-required` is a promise that the caller has something to supply, and here
the agent asked no question — a conformant client would prompt its user, and
that reply restarts the same run against the same budget, which is a loop rather
than a recovery. The knob that actually unblocks it (`max_tool_rounds`, or
`[llm] reasoning` for the empty-answer sibling) belongs to whoever configured the
agent, not to whoever is talking to it. Same test as `EchoResponder`'s `Working`:
would a caller acting on this state be acting correctly?

The partial work rides along in the message rather than being replaced by an
apology — a failed task's most useful content is whatever the model did produce,
and for the empty-answer case the reasoning is the *only* thing produced, billed
and otherwise seen by nobody who was not streaming.

**Fleet file, not a directory or glob.** A directory has no name, no order, and
no place to grow per-member options, and it silently picks up whatever gets
dropped next to it — including the fleet file itself. The file redefines nothing
about an agent; `AgentConfig` stays the source of truth. Its real payoff is the
checks that only exist *between* members (shared port, colliding registry id),
run before anything binds.

**A delegation tool stays advertised through its peer being absent.** The
alternative was what it did before: resolve every peer at startup and drop the
entry when the lookup or the dial failed. That makes the model's toolset a
snapshot of one instant, and under a control plane the instant is the wrong one
— an orchestrator deployed first sees no peers at all and never looks again. So
the reference is resolved per call, the tool is advertised regardless, and being
unable to reach the peer is a tool *result* the model can route around rather
than an error that fails the orchestrator's task.

`PeerUnavailable` exists to keep that line drawn: never reached it (nothing
matches, or the dial failed) is news, while an error *during* a delegation means
the peer took the work and we lost it, which is a broken run. Two different
types, so a call site cannot blur them by accident.

**A dead peer is ranked, not removed.** The refresh loop could have
deregistered an agent that stopped answering, and that reads fine until the
lookup happens: `find_by_skill` would return nothing, and the orchestrator would
report "no agent advertises this skill" — untrue, and it sends whoever is
debugging to the wrong place. Ordering the unreachable last gives a caller
taking the first match a live peer whenever one exists, and when none does it
still gets an entry, so what comes back is "could not connect to X". Same shape
as `a2a ps`: filter the view, keep the record.

`Liveness` has three states because two would have to pick a default, and both
defaults are wrong — a freshly registered agent reading `Live` is a claim nobody
checked, and reading `Unreachable` demotes an agent for not having been probed
yet. Same reasoning as `Recovered::{Adopted, Ephemeral}`.

**Registry recovery derives from the runtime; it is not a database.**
`ControlPlane::recover` rebuilds discovery at startup from what the runtime is
still running plus a card fetch. This was the cheaper of the two options and the
better one: no schema, no migrations, and it cannot go stale against reality,
because reality — what the engine is running — *is* the source. A persistent
registry adapter is still the answer for what derivation cannot cover (agents
registered by something other than this runtime; discovery shared across
control-plane processes), but both are speculative, which is why there is a port
and not a database.

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

**Configs deploy raw; the control plane expands `${VAR}`.** Expanding client-side
would put the operator's secrets on the wire, force every deploying machine to
hold them, and quietly bypass the `--allow-env` allowlist, which is the whole
secrets model.

**`deploy` checks before it sends, then does not abort mid-flight.** Shape
(leniently — the deploying machine may hold none of the secrets) and the
cross-agent conflict rules are checked up front, so `--fleet` cannot leave half a
fleet deployed over a port clash that was knowable. Past that gate each agent is
independent and failures are *reported* rather than aborting: the remaining
agents are no more likely to fail than the first, and stopping early only makes
the partial state less predictable.

**Isolation is deliberately deferred.** Supervision (provision/start/stop/health)
and isolation (microVMs, gVisor) are different jobs. Supervision shipped;
isolation is a response to a threat model that does not exist yet — there is no
untrusted third party in the picture — and the `AgentRuntime` port already bought
the option to add it later. Don't exercise the option before there is a use case.
Two caveats stand: the cheap 80% was taken (see container hardening below), and
today's real exposure is the **control-plane API**, not the agent sandbox.
Bind-to-localhost is not a security model.

If the driver ever becomes "run this on users' infra", the next adapter is
**Kubernetes** (Deployment + Service per agent), not a microVM. Firecracker and
gVisor only matter if we host other people's agent code — at which point the
config-delivery model (host bind-mounts, published host ports, secrets from the
control-plane process env) needs rethinking anyway.

**Container agents are *contained*, not *isolated*.** Hardening drops all
capabilities, sets `no-new-privileges`, caps processes, and mounts the root
filesystem read-only where storage allows. That removes what an HTTP server never
needed and bounds what a misbehaving one consumes; it is not a defence against
code written to escape. Keep describing it that way.

**Read-only rootfs is derived, not asked.** `needs_writable_rootfs` decides it
from the config, because being wrong is invisible in both directions: a read-only
`sqlx` agent crash-loops on a disk error that names nothing useful, and a
writable in-memory one gives up the protection for free.

**Resource ceilings are opt-in.** There is no memory limit that is right for
every agent, and a guessed default surfaces as an agent dying under load for no
visible reason. `--memory` / `--cpus` are explicit; `--no-hardening` is the
escape hatch and warns, like `--no-auth`.

**`--runtime local` is dev-only, and says so.** Its children die with the
supervisor (`Recovered::Ephemeral`), and they inherit the control plane's entire
environment — the allowlist bounds what a *config* may name, not what a spawned
child can read, and an agent config declaring an `mcp_client` with an arbitrary
`command` can read all of it. `--runtime container` is the supported
control-plane backend. Sealing the local case needs `Command::env_clear()` plus
an explicit carry-over set, which is platform-fiddly (`PATH`, `SystemRoot`, temp
dirs) — see `TODO.md`.

---

## Hazards that will recur

**A test gated on Docker skips *green* when the image is absent — and an image
that cannot be built is absent.** The container backend silently had no image at
all: the `llm` feature was added to the binary's `required-features` and never to
the Dockerfile, the pinned `rust:1.85` builder was below what the dependency tree
required, and a missing `.dockerignore` shipped a multi-GB context. None of it
was caught. The fix for that whole class is a structural test that needs no
Docker and parses both sides — see
`container_runtime_test.rs::every_feature_the_a2a_binary_requires_is_a_default`.
Any "skips when unavailable" test needs a no-infrastructure sibling asserting the
thing *could* run.

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

**A settled-looking task can still be reporting failure in prose.** The same
handler ended a tool-budget overrun with `Ok("I could not converge on an
answer…")` — `completed`, with a sentence saying it did not work. Every caller
that branches on state (`a2acli` exits 2 on `failed`, delegation relays whatever
it gets) read that as success. When a handler gives up, the giving up belongs in
the state, not only in the text.

**A tool result is prose, and the model is the only thing that can branch on
it.** Delegation returned the peer's status message whatever state its task
ended in, so a `failed` peer ("I could not find that invoice") was relayed as an
answer. There is no state field to fix this in — `ToolSource::invoke` returns a
string that goes straight into the conversation — so the outcome has to be *in*
the prose: a completed task's reply goes back bare, everything else is
introduced by what happened to it. Anywhere a caller's only channel is text,
the text has to carry what a state field would have.

The general form is a return type that cannot say it: `Result<String, _>` has
one slot for success and one for a *broken run*, and "the model finished but
delivered nothing" is neither. Both cases here — the empty completion and the
budget overrun — were the same missing third outcome, which is why they are now
one `Answer::{Given, GaveUp}` rather than two ad-hoc patches. When a handler's
outcome type has fewer variants than its real outcomes, the surplus one gets
encoded in prose, and prose is what no caller can branch on.

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
made `a2a run` start a server and print absolutely nothing. Any new binary needs
an explicit fallback filter.

**Docker's `.dockerignore` uses Go's `filepath.Match`, where `target/` with a
trailing slash excludes nothing.** The first version of that file made no
difference at all. Write patterns without trailing slashes.

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
- The binary build (`release-binaries.yml`) needs manual dispatch —
  `GITHUB_TOKEN` will not auto-trigger it from a release-plz tag.
