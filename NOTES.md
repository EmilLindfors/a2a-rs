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

**Fleet file, not a directory or glob.** A directory has no name, no order, and
no place to grow per-member options, and it silently picks up whatever gets
dropped next to it — including the fleet file itself. The file redefines nothing
about an agent; `AgentConfig` stays the source of truth. Its real payoff is the
checks that only exist *between* members (shared port, colliding registry id),
run before anything binds.

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
`send` polls to a terminal *or interrupted* state (`input-required` and
`auth-required` are stops too: the agent is waiting on the caller). `--no-wait`
is the escape hatch, not the default, because "send a message to an agent" means
"and show me what it said". Polling, not `subscribe_to_task`, only because the
ConnectRPC subscription does not close on a terminal state yet — see `TODO.md`.

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

**`EnvFilter::from_default_env()` enables nothing when `RUST_LOG` is unset.** It
made `a2a run` start a server and print absolutely nothing. Any new binary needs
an explicit fallback filter.

**Docker's `.dockerignore` uses Go's `filepath.Match`, where `target/` with a
trailing slash excludes nothing.** The first version of that file made no
difference at all. Write patterns without trailing slashes.

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
