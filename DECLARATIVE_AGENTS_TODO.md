# Declarative Agents — Next Steps

Roadmap for turning the A2A crates into a usable multi-agent **platform**. The
guiding principle: don't build infrastructure into `a2a-rs` (the pure hexagonal
protocol crate) — define **capabilities as ports** and add **adapters** in the
platform layer. Four pillars; 1–3 are substantially done, 4 is the gap.

Status legend: ✅ done · 🔜 next · ⬜ planned

---

## Direction change (2026-07-25): standalone first, Terraform later

The end goal was framed as *write HCL, get running agents*, which made the
Terraform provider the critical path. **That is now deferred.** The goal is
*install `a2a`, get running agents* — a great standalone CLI, no Go and no
Terraform in the loop. The provider becomes a thin client over whatever the CLI
already proves out, rather than the primary interface being designed through it.

This is the better order regardless: HCL is a front-end onto the same control
plane and config schema, so anything the standalone path settles (config
strictness, fleet composition, lifecycle commands) is work the provider would
otherwise have had to invent and then re-litigate.

The **Standalone UX** section below is now the active track. The "close the
spine" items keep their numbering for continuity; 1, 2, and 4 are done, and 3
(the provider rewrite) is parked behind the UX work and the extraction.

---

## Standalone UX — the active track

Making `a2a` a tool that is pleasant on its own. Ordered by value.

Everything here has landed. The last item — the CLI half of the control plane —
went in on top of restart-recovery ("close the spine" item 4), which is what
kept an `a2a ps` from reporting a fleet the control plane had already forgotten.

- ✅ **Typo'd config keys are errors, not silent defaults.** Every config struct
  is `#[serde(deny_unknown_fields)]`. Previously `http_prot = 9999` validated
  clean and then served on 8080 — the worst failure mode a declarative tool can
  have, because the config *lies* and the check *agrees*. Now it points at the
  line, names the key, and lists the valid ones. (Nothing in the crate uses
  `serde(flatten)`, so this composes cleanly; all 17 example configs still pass.)
- ✅ **`a2a validate` is usable without production secrets.** It used to hard-fail
  on any unset `${VAR}`, so checking a config's *shape* required holding the
  secrets it would run with — unusable in CI and on a fresh checkout, which is
  exactly where validation earns its keep. Unset refs now expand to a placeholder
  and are *reported*; `--strict-env` restores hard failure for a pre-deploy gate.
  Leniency is scoped to missing values only — structural checking is untouched
  (`AgentConfig::check_toml`).
- ✅ **Validation output is CLI output, not log output.** Results go to stdout
  without timestamps, levels, or module paths, one indented block per file with a
  summary line (agent, handler, port, skills). Long-running commands (`run`,
  `control-plane`) still use `tracing` — they emit events over time, which is
  what it is for.
- ✅ **`a2a new <name> [--template echo|llm|mcp|orchestrator]`** — scaffolds a
  commented, immediately-runnable config, then prints the next commands. The
  templates are written to be *read*: every non-obvious key carries a comment, so
  a generated file answers "what else can go here?" without `print-schema`.
  Rendering is pure (`core::template`), so it is unit-tested against the real
  parser; file I/O stays in the binary. `--output`, `--port`, `--force`;
  clobbering an existing file is refused by default. Two invariants are pinned by
  tests because they are what make the scaffold trustworthy: every template
  parses *and validates*, and no template references `${VAR}` — a freshly
  scaffolded agent must run without the user first setting something.
- ✅ **`a2a run` is no longer silent.** `EnvFilter::from_default_env()` enables
  nothing when `RUST_LOG` is unset, so `run` started a server and printed
  *absolutely nothing* — no confirmation, no URL, no errors. Since `a2a new`
  ends by telling you to run exactly that, the scaffold walked users into a
  blank terminal. Default is now `warn` globally with our crates at `info`;
  `RUST_LOG` still overrides.
- ✅ **`a2a run` prints a banner** — agent name, URL, card URL, and a
  copy-pasteable `a2acli send` / `curl`. On stdout, not through `tracing`: it is
  the one thing a person needs off the screen, not a timestamped event. Printed
  before binding (since `start_http` never returns), so the URLs are an intent
  and a bind failure is still reported by the agent task.
- ✅ **Fleet in one file** — `a2a up -f fleet.toml` (`core::fleet`). Multi-agent
  used to mean repeating `--config` per agent on every invocation, which is not
  a reviewable artifact. A fleet file lists member configs by path (resolved
  relative to itself, so a fleet runs from any directory) and redefines nothing
  about an agent — `AgentConfig` stays the source of truth. Chose the file over
  the directory/glob alternative: a directory has no name, no order, and no
  place to grow per-member options, and it silently picks up whatever is dropped
  next to it — including the fleet file itself.
  Its real payoff is the checks that *only* exist between members, run before
  anything binds: two agents claiming the same **port**, and two names that
  slugify to the same **registry id**. Both are silent-wrong at runtime — a port
  clash surfaces as one bind error buried in a process that otherwise came up,
  and an id clash as registration upsert, so delegation by skill/`agent_id`
  quietly reaches only the last one registered. `a2a validate --fleet` runs the
  same check without starting anything, and reports every conflict rather than
  the first. Pure conflict rules unit-tested in `core::fleet`; the binary is
  driven end-to-end by `tests/fleet_test.rs`, which also pins
  `examples/fleet.toml` against drift.
- ✅ **Drive the control plane from the CLI** — `a2a deploy/ps/logs/stop`. The
  server half existed and nothing but `curl` could work it, which made the whole
  daemon story reachable only through Terraform — the interface that was just
  deferred. The same binary is now both halves, over a first-class
  `ControlPlaneClient` adapter (the Terraform provider will target the same
  routes, so this is not throwaway CLI plumbing). `--url`/`--token` default to
  `A2A_CONTROL_PLANE_URL`/`_TOKEN` and to where `control-plane` binds, so a
  control plane in one terminal and these in another find each other with no
  configuration.
  Three decisions worth keeping:
  - **Configs deploy raw.** `${VAR}` is expanded by the control plane, against
    *its* environment and *its* `--allow-env` allowlist. Expanding client-side
    would put the operator's secrets on the wire and force every deploying
    machine to hold them — and it would quietly bypass the allowlist, which is
    the whole secrets model.
  - **`deploy` checks before it sends.** Shape (leniently — this machine may
    hold none of the secrets) and the cross-agent conflict rules, so
    `--fleet` cannot leave half a fleet deployed over a port clash that was
    knowable up front. Past that gate each agent is independent and failures are
    reported rather than aborting, since stopping early only makes the partial
    state less predictable.
  - **`logs` is a new port method, not a special case.** `AgentRuntime::logs`
    (no default impl, like `recover`): `ContainerRuntime` replays via the
    engine — merging *both* streams, since an agent's `tracing` output is on
    stderr and reading stdout alone shows an empty log for a crashing agent —
    and `LocalProcessRuntime` captures per-agent files under a `--log-dir`
    (defaulting to `<config-dir>/logs`). A backend that keeps no logs returns
    `RuntimeError::Unsupported` → 501 → a distinct client error, because
    "printed nothing" and "not recorded" send an operator to opposite places.
    This is the same honesty rule as `Recovered::Ephemeral`.
  Two papercuts fixed on the way, both of which made captured logs worse than
  useless: `a2a`'s `tracing` output went to **stdout**, so it mixed into the
  reports `validate`/`doctor` exist to produce (now stderr), and it emitted ANSI
  colour unconditionally, so every captured log had escape codes baked in (now
  gated on `stderr().is_terminal()`).
  Tests: client↔router round-trip over the in-memory runtime (including a
  delegating no-logs runtime for the 501 path) in `tests/control_plane_test.rs`,
  and `tests/control_plane_cli_test.rs`, which drives the real binaries — a live
  `a2a control-plane` supervising a real `a2a run` child — through
  deploy → ps → logs → stop, plus the two failures a user hits first (no control
  plane there; a fleet that conflicts).
- ✅ **`a2a doctor`** — is an LLM key set, is a container engine present, is the
  port free, does the config's MCP command exist, is every `${VAR}` actually
  resolvable *here*. One report instead of a class of unrelated-looking runtime
  symptoms (a bind error buried in a log, a tool that silently is not there, an
  agent answering from a canned fallback). Split so the rules are testable:
  `core::doctor::requirements` derives what a config *needs* purely from the
  config, and the binary probes the machine (bind the port, search `PATH` with
  `PATHEXT`, read the environment) — `doctor` is the two halves joined.
  It also checks the whole named set with `fleet_conflicts`, since each config
  can be fine alone and still not run alongside the others. Deliberately
  stricter than `validate` on one point: an unset `${VAR}` is *reported* by
  validate (shape without secrets) and a **problem** here (`a2a run` refuses to
  start), which is the difference between "is this well-formed" and "will this
  work on this machine". Only problems set the exit code; warnings (no model
  key, no container engine) do not. Tests: `core::doctor` units +
  `tests/doctor_test.rs`, which takes a real port and names a real missing
  command rather than asserting on the report's wording alone.

---

## Close the spine (ordered)

The critical path as originally scoped. Items 1, 2, and 4 landed; the rest is
now sequenced behind the standalone UX track above. What is left splits cleanly:
3 and 7 wait on Terraform resuming, while 5 and 6 (container hardening,
per-agent images) are independent and can be picked up whenever.

1. ✅ **Agent card `protocol_binding` now matches the mounted transport** (also
   `TODO.md` §2). It advertised `JSONRPC` while `HttpServer` mounts ConnectRPC,
   so card-driven clients negotiated to an endpoint that was never mounted —
   and pillar 1 (`A2aAgentToolSource`) and pillar 2 (skill discovery) *both*
   rest on that negotiation. Fixed at the root rather than per-caller:
   `HttpServer` stamps `CONNECTRPC` onto the primary interface of the card it
   serves (it mounts exactly one protocol, so it can state the truth about
   itself), and `a2a-agents`' `agent_info_from_config` sets it at the source
   too, because the card is read off the HTTP path as well — registry
   self-registration and MCP mode both take it from there. The binding strings
   are now `PROTOCOL_BINDING_{JSONRPC,CONNECTRPC,HTTP_JSON}` consts in `domain`,
   shared by the card builder and the client negotiation factories so the two
   sides cannot drift apart again. Regression test builds the agent info
   *plainly* — no `with_preferred_transport` — and asserts both the served card
   and a live `default_registry().negotiate()`
   (`a2a-rs/tests/agent_card_transport_test.rs`).
2. ✅ **Control plane authenticated + secrets allowlisted.** It had no auth at
   all, `POST /agents` accepted arbitrary TOML, and `ContainerRuntime` injected
   *any* var the config referenced from the control-plane process's env — i.e.
   "start a container of my choosing, with your secrets in it", held back only
   by the default `127.0.0.1` bind. Now:
   - `ControlPlaneAuth` (bearer token, constant-time compare) is a **required
     parameter** of `control_plane_router`, so an open endpoint cannot happen by
     omission — `ControlPlaneAuth::Disabled` has to be written out. The CLI
     refuses to start without `--token` / `A2A_CONTROL_PLANE_TOKEN` unless
     `--no-auth` is passed, which warns loudly.
   - `EnvAllowlist` (deny-by-default) replaces "inject whatever is referenced".
     Configured once at the composition edge (`--allow-env VAR`, repeatable) and
     injected into both the `ControlPlane` (vets what the API accepts) and the
     runtime adapter (decides what actually crosses into the agent), so the
     runtime stays safe even when something else drives it.
   - The check runs against the **raw** TOML *before* parsing. Parsing expands
     `${VAR}` and reports set vs. unset differently, so a config naming a
     forbidden secret would otherwise be a probe for which secrets the control
     plane holds. `ControlPlane::prepare` returns a `PreparedDeploy` that is the
     only thing `deploy` accepts, making the gate unskippable rather than
     merely conventional. Its `Debug` is hand-written to print only the id — the
     prepared config holds *expanded* secret values.
   - `LocalProcessRuntime` enforces the same allowlist, but its children inherit
     the whole process environment and an `mcp_client` config can exec arbitrary
     commands, so it is documented as dev-only; untrusted configs belong on
     `ContainerRuntime`. (Sealing that would need a scrubbed child env — see
     follow-ups.)
3. ⏸ **JSON body on `POST /agents`; rewrite the provider as a passthrough**
   (pillar 4). Kills `renderTOML` drift at the root instead of patching it.
   **Parked** behind the standalone UX track and the platform extraction — see
   the direction change above. The passthrough decision still stands; it just
   isn't the next thing built.
4. ✅ **Restart-recovery for control-plane state** (pillar 2/3). Both halves of
   what the control plane knew were process state — the runtime's instance table
   and the registry's cards — so a bounce produced the worst answer an API can
   give: `GET /agents` → `[]` and `DELETE` → 404 *while the agents were still
   serving*, which reads as "they were destroyed" to an operator and to a
   Terraform `Read` (which would then recreate them on top of the running ones).
   Now `AgentRuntime::recover` adopts what the backend is still running and
   `ControlPlane::recover` re-registers the adopted agents' cards, rebuilding
   discovery from the runtime rather than from a database — the option pillar 2
   named. `a2a control-plane` calls it before binding, so the API's first answer
   is true; a failure there is fatal, since a backend that cannot be queried
   cannot be deployed to either.
   The shape that matters is `Recovered::{Adopted, Ephemeral}`: "nothing was
   running" and "I cannot tell what is running" are different answers, and only
   the first is safe to report as an empty fleet. Every adapter must say which
   (no defaulted trait method) — `ContainerRuntime` reads `docker ps -a --filter
   label=a2a-agent` (provisioning now also stamps the published port as a label,
   so the whole map is reconstructed rather than half of it), while
   `LocalProcessRuntime` answers `Ephemeral` because its children die with it.
   That asymmetry is the concrete argument for container being the supported
   control-plane backend. Card fetching became a port (`CardSource`) so the
   service does not reach for an HTTP client and recovery is testable without a
   network. Tests: label round-trip + `ps` parsing units, four service-level
   recovery tests over the in-memory adapters, an HTTP-level restart test
   (`tests/control_plane_test.rs`), an engine-accepts-the-query test that needs
   docker but no image, and the docker-gated e2e where a fresh runtime adopts a
   live container and stops it.
5. ✅ **Container hardening flags + non-root image** (pillar 3) — the cheap 80%
   of isolation, taken at the `create_args` chokepoint as planned.
   `ContainerHardening` drops all capabilities, sets `no-new-privileges`, caps
   processes at 512, and mounts the root filesystem read-only (with a `/tmp`
   tmpfs) — the last **only where the agent's storage allows it**, derived from
   the config rather than left to the operator, because getting it wrong either
   way is invisible: a read-only `sqlx` agent crash-loops on a disk error that
   names nothing, and a writable in-memory one gives up the protection for
   free. The image runs as uid 10001.
   Resource *ceilings* are deliberately not defaulted. There is no memory limit
   that is right for every agent, and a guessed one surfaces as an agent dying
   under load for no visible reason — so `--memory` / `--cpus` are opt-in, with
   `--no-hardening` as the escape hatch (it warns, like `--no-auth`).
   **Three latent breakages surfaced doing this, all in the container path:**
   the base image could not build *at all* — the `llm` feature was added to the
   binary's `required-features` and never to the Dockerfile, and cargo refuses
   a named bin whose features are not all enabled; the pinned `rust:1.85`
   builder was below what the dependency tree now requires (1.87+); and there
   was no `.dockerignore`, so `COPY . .` shipped a 2 GB context, most of it
   `target/`. None of it was caught because the docker-gated e2e *skips green
   when the image is absent* — and an image that cannot be built is absent. The
   fix for that class is a test that needs no Docker:
   `the_dockerfile_enables_every_feature_the_a2a_binary_requires` parses both
   sides and fails in CI. Verified against a real engine besides: a fully
   hardened agent comes up healthy and `docker inspect` confirms the engine
   applied each flag (a flag it ignored would pass the argv tests and protect
   nothing).
6. ⬜ **Per-agent images** (pillar 3) — the escape hatch that makes custom Rust
   handlers expressible declaratively.
7. ⬜ **End-to-end `terraform apply` smoke test** — run a real `a2a
   control-plane`, apply real HCL, assert an agent answers. Every layer is
   currently tested in isolation, and the seams between them are exactly where
   items 1, 3, and 4 live.

---

## Pillar 1 — Agent-as-tool delegation ✅

Shipped in `feat/declarative-agents` (commit `9decb30`).

- ✅ `ToolSource` trait unifying MCP servers and remote A2A agents
  (`a2a-agents/src/handlers/tools.rs`).
- ✅ `McpToolSource` (all configured MCP servers) + `A2aAgentToolSource`
  (a remote agent as one `ask_<slug>` tool via the `Transport` port).
- ✅ `[[handler.llm.agents]]` config + `a2a` binary wiring.
- ✅ Live integration test over a real socket + `orchestrator_agent.toml`.

**Follow-ups (small, do opportunistically):**
- ⬜ Stream the delegated agent's tokens through instead of poll-to-terminal:
  prefer `subscribe_to_task`, fall back to the current bounded `get_task`
  poll. (`A2aAgentToolSource::invoke`.)
- ✅ Decoupled the LLM handler from the `mcp-server` feature so agent-as-tool
  works without pulling in `rmcp`: new `llm` feature gates `LlmHandler` (the
  `tools` module / `A2aAgentToolSource` were already MCP-free; only the `mcp`
  submodule / `McpToolSource` stay `mcp-server`-gated). `cargo check -p
  a2a-agents --features llm` builds with zero MCP.
- ⬜ Resolve the axum 0.7 (frontend) vs 0.8 (`a2a-rs`) split — the test uses an
  `axum8` dev-dep alias as a stopgap. Bump the frontend to 0.8 when
  `askama_axum` allows.

---

## Pillar 2 — Agent registry / discovery ✅ (first cut)

So an orchestrator finds peers by **skill** instead of hard-coded URLs.

- ✅ `AgentRegistry` port (capability, not technology):
  `register(card, endpoint)`, `deregister(id)`, `get(id)`, `find_by_skill(skill)`,
  `list()`, in the platform layer (`a2a-agents/src/registry/mod.rs`).
- ✅ `InMemoryAgentRegistry` adapter (first-class type, per hex rule 6) —
  `RwLock<HashMap>`, runs without external infra. Unit-tested.
- ✅ `A2aAgentToolSource` resolvable from the registry: a `[[handler.llm.agents]]`
  entry names exactly one of `url` / `skill` / `agent_id`
  (`RemoteAgentConfig::target()`, parse-don't-validate). The runner two-phase
  starts: phase 1 self-registers every agent's card from config (race-free),
  phase 2 resolves skill/agent-id refs at startup (`bin/a2a.rs`).
- ✅ Live discovery-by-skill integration test
  (`tests/registry_discovery_test.rs`) + `examples/registry_{worker,orchestrator}.toml`.

**Follow-ups:**
- ✅ **Registry survives a control-plane restart — by derivation, not by a
  database.** `InMemoryAgentRegistry` is still process-lifetime, but
  `ControlPlane::recover` rebuilds it at startup from the runtime + a card
  fetch (`CardSource`), which was the second of the two options named here and
  the cheaper one: it needs no schema, no migrations, and cannot go stale
  against reality, because reality (what the engine is running) *is* the source.
  The failure it closes is the one described: `Read` 404ing after a bounce and
  Terraform recreating agents on top of running ones.
  ⬜ A persistent adapter is still the answer for what derivation cannot cover:
  agents registered by something *other* than this runtime, and discovery shared
  across control-plane processes. Both are speculative today — hence a port, not
  a database.
- ⬜ Resolve at **call time** (dynamic registry-backed `ToolSource`) for late
  joiners, not just at startup. Also upgraded from opportunistic: TF-managed
  agents come and go, so a startup-only resolution pass goes stale by design.
- ⬜ Card-fetch refresh loop (re-poll `/.well-known/agent-card.json` for
  liveness). Blocked in practice on the `protocol_binding` fix above.

---

## Pillar 3 — Runtime supervision ✅ / isolation ⬜ (deliberately deferred)

A place to run an agent. Chosen first substrate: **OCI containers**.

These are two different jobs and they belong in different phases:

- **Supervision** — provision/start/stop/health, the container backend, secret
  pass-through. Done, and rightly done before pillar 4: a Terraform provider
  needs a real Create/Delete to drive. This is what has landed.
- **Isolation** — microVMs, gVisor, defending against agent code you did not
  write. **Not now.** It is a response to a threat model that does not exist yet
  (there is no untrusted third party in the picture), and the port already
  bought the option to add it later without disturbing anything. Don't exercise
  the option before there's a use case. Two caveats in the bullets below: take
  the cheap 80% now, and note that today's real exposure is the *control-plane
  API*, not the agent sandbox.

- ✅ `AgentRuntime` port: `provision(spec)`, `start(id)`, `stop(id)`,
  `health(id)`, `list()`, `recover()`, `logs(id, tail)` — capability port in the platform layer
  (`a2a-agents/src/runtime/mod.rs`), sharing `registry::AgentId` so runtime and
  registry compose. `AgentSpec`, `RuntimeHealth`, `RuntimeStatus`, `RuntimeError`.
  (The existing per-agent `core::AgentRuntime<H,S>` was renamed to `AgentServer`
  to free the name — that type *serves one agent*; the port *supervises many*.)
- ✅ `LocalProcessRuntime` adapter (`a2a-agents/src/runtime/local.rs`) —
  supervises each agent as a child `a2a run --config <path>` OS process; health =
  process alive **+** agent-card probe (`a2a_rs::fetch_agent_card`). First-class
  type (hex rule 6). Live end-to-end test spawns a real `a2a` child via
  `CARGO_BIN_EXE_a2a` and drives provision→start→Healthy→stop→Stopped
  (`tests/local_runtime_test.rs`).
- ✅ `ContainerRuntime` adapter (`a2a-agents/src/runtime/container.rs`) — CLI
  shell-out to a configurable `docker`/`podman` (no new dep). One base image
  (`a2a-agents/Dockerfile`) with the TOML bind-mounted at `/etc/agent.toml` and
  `a2a run` as the command; one container per agent (`a2a-agent-<id>`). Health =
  `inspect` status + agent-card probe through the published port. Pure
  `create_args`/`container_name` are unit-tested without Docker; a Docker-gated
  e2e (`tests/container_runtime_test.rs`) skips when the engine/image is absent.
  `a2a control-plane --runtime container [--engine --image]` runs the control
  plane over it (the payoff of the shared port). Adds `RuntimeError::Backend`.
  Constraint: container agent configs must omit `host` (so `HOST=0.0.0.0`
  applies) to be reachable. *Per-agent images for custom Rust handlers still ⬜.*
- ✅ Thin **control-plane service** owning `AgentRuntime` + `AgentRegistry` +
  `CardSource` (`a2a-agents/src/control_plane/`):
  `ControlPlane::{deploy,undeploy,status,list,recover}`
  (service, hex rule 9a) — deploy provisions+starts via the runtime **and**
  registers the card so peers discover it (runtime/registry ids coincide), and
  `recover` rebuilds both halves at startup after a restart. HTTP
  adapter `control_plane_router` (`POST/GET/DELETE /agents`, axum 0.7) is the
  surface the Terraform provider will target. `a2a control-plane --bind
  --config-dir` subcommand serves it over `LocalProcessRuntime`. Added
  `InMemoryAgentRuntime` (process-free fake, hex rule 6) for fast service tests;
  HTTP round-trip test drives it with `reqwest` (`tests/control_plane_test.rs`).
- ✅ Secrets stay out of on-disk TOML: configs use `${ENV}` refs, and
  `ContainerRuntime::provision` injects every referenced var into the container
  as a value-less `-e VAR` pass-through (new `core::config::referenced_env_vars`;
  values resolved from the deploying process's env by the engine CLI, so they
  never hit disk or argv; in-container `a2a run` expands at startup; missing
  secrets fail provisioning on the host, fast). Local runtime children inherit
  the env already. E2e-verified via a `${VAR}`-templated card description
  (`tests/container_runtime_test.rs`).
- ✅ **Control-plane API authenticated + injectable secrets allowlisted.** See
  "close the spine" item 2 — the pillar's real security work, worth more than a
  sandbox. Bind-to-localhost is not a security model.
- ⬜ **Scrub the child environment in `LocalProcessRuntime`.** The allowlist
  bounds what a *config* may name, not what a spawned child can read: children
  inherit the control plane's entire environment, and an agent config declaring
  an `mcp_client` with an arbitrary `command` can read all of it. Sealing it
  means `Command::env_clear()` plus an explicit carry-over set, which is
  platform-fiddly (`PATH`, `SystemRoot`, temp dirs) — hence deferred, with the
  adapter documented as dev-only in the meantime.
- ✅ **Restart-recovery.** `AgentRuntime::recover` (see "close the spine" item 4
  for the full write-up). `ContainerRuntime` rebuilds its `AgentId → port` map
  from `docker ps -a --filter label=a2a-agent`, which is what makes the engine —
  not this process's memory — the source of truth about which agents exist;
  provisioning stamps the published port as a second label so the map is
  recovered whole. `LocalProcessRuntime` returns `Recovered::Ephemeral`: nothing
  durable ties a stray `a2a run` to an `AgentId`, so the honest answer is "I
  cannot tell you", not an empty list. That is the concrete reason container is
  the supported control-plane backend and local is dev-only.
- ✅ **Container hardening — the cheap 80% of isolation.** `ContainerHardening`
  at the `create_args` chokepoint: `--cap-drop=ALL`,
  `--security-opt=no-new-privileges`, `--pids-limit 512`, and `--read-only` +
  `--tmpfs /tmp` where storage allows, plus opt-in `--memory` / `--cpus` and a
  non-root `USER 10001` in the Dockerfile. Still describe this as *contained*,
  not *isolated* — it removes what an HTTP server never needed and bounds what a
  misbehaving one consumes; it is not a defence against code written to escape.
  The `--read-only` caveat was handled by deriving it (`needs_writable_rootfs`)
  rather than asking, and the labels restart-recovery reads back are called out
  in `create_args` so a future edit does not drop them
  (`recovery_reads_back_what_create_stamped` now runs against the *default*
  hardening, i.e. what production actually stamps).
- ⬜ **Per-agent images** (`image` on `AgentSpec` / a `[runtime]` config block).
  This is the escape hatch that keeps the declarative layer from being a toy: a
  custom Rust handler is just a different image and the platform stops caring.
  TOML-only covers the common case; image + config covers 100%. Also retires the
  `HandlerType::Custom(_) → echo` fallback — with images available, an unknown
  handler type should be a hard error. (`a2a doctor` reports it as a problem
  today, so it is at least no longer *silent*; it is still wrong at run time.)
- ⬜ *Deferred, gated on a real use case:* further adapters behind the same port.
  If the driver is "users' infra", the next adapter is **Kubernetes**
  (Deployment + Service per agent), not a microVM. Firecracker/gVisor only
  become relevant if we host other people's agent code — at which point the
  current config-delivery model (host bind-mounts, published host ports, secrets
  from the control-plane process env) needs rethinking anyway. Sister-project
  material, not a next step.

---

## Pillar 4 — Terraform provider rework ⏸ (deferred — standalone first)

> **Parked as of 2026-07-25.** The provider is no longer the primary interface;
> the `a2a` CLI is (see "Direction change" above). Everything below still holds
> as the plan for *when* it resumes — the passthrough design in particular is
> what stops the drift described here from coming back. Resume after the
> standalone UX track and the platform extraction.


Make `terraform-provider-a2aagent` a real provider, not a file writer. This is
where the end goal lives, and it is currently the least-built pillar.

**Present state, precisely.** `renderTOML`
(`internal/provider/agent_resource.go:223`) emits `implementation = "llm"` — a
key `HandlerConfig` no longer reads — so the provider's output is *silently
wrong*: `terraform apply` succeeds and the agent falls back to echo. Both
validators (`:274`, `:286`) `return nil`, so the "Rust is the single source of
truth for validation" claim in `DECLARATIVE_AGENTS.md` is false today; nothing
validates anything. And the control-plane HTTP API built for the provider to
target (pillar 3) is not targeted — the provider still writes files to a dir.

**The fix is structural, not a patch.** Hand-maintaining a TOML serializer in Go
against a Rust struct is a permanent drift source; the typed HCL attributes
cover ~8 of ~40 config fields, so the provider cannot express most agents even
when correct. Make the provider carry config it does not understand:

- ⬜ **Passthrough config.** `POST /agents` already takes `config_toml: String`
  — lean into that rather than fighting it. `AgentConfig` derives `Deserialize`,
  so accepting a JSON body variant is nearly free and lets HCL do
  `jsonencode(...)`. The resource takes a `config` object (or raw TOML) and
  passes it through; Go never learns the schema, so it can never drift, and Rust
  becomes the sole validator *for real* because the provider has nothing to
  validate against.
- ⬜ **Delete `validateWithJSONSchema` rather than implementing it.** One
  working validation path beats two stubs. Either shell to `a2a validate` (needs
  a stdin / `--stdin` mode — it is paths-only today) or let the control plane
  reject on deploy and surface the error as a TF diagnostic. With passthrough
  config, the bundled `internal/schema/agent_config.json` fixture and the
  `a2a print-schema` regeneration loop become unnecessary, not unimplemented.
- ⬜ **Real lifecycle against the control-plane API:** Create = provision + start
  + register card; Read = health/inspect; Update = re-provision; Delete = stop +
  deregister. Its blocker is gone: restart-recovery landed, so `Read` no longer
  lies after a control-plane bounce (on `--runtime container`, which is the only
  backend a TF-driven control plane should use).
- ⬜ **End-to-end acceptance test:** live `a2a control-plane`, real
  `terraform apply`, assert the agent answers. See "close the spine" item 7.
- ⬜ Provider moves to the extracted platform repo — see below; it should move
  *before* this work, not after.

---

## Platform extraction ⬜ — do it *before* pillar 4

Per `DECLARATIVE_AGENTS.md`: move `a2a-agents`, `a2a-agents-common`, and the
Terraform provider into `a2a-agents-platform`, depending only on **published**
`a2a-rs` / `a2a-mcp` / `a2a-ap2` (no path deps back). Keeps the protocol crates
clean. Runtime + registry land in the new repo.

**Ordering:** extract before the pillar-4 rework rather than after. A Terraform
provider with its own Go toolchain, Go CI, and TF acceptance tests does not
belong in the protocol repo, and pillar 4 is where the Go work gets serious —
extracting afterwards means moving a much larger, freshly-churning surface. The
extraction only needs published `a2a-rs`, and nothing on the critical path
requires protocol changes; use a local `[patch.crates-io]` path override if
co-development against `a2a-rs` is needed during the transition.

---

## Doc drift ⬜

- ✅ `DECLARATIVE_AGENTS.md` rewritten for the standalone direction: the
  Terraform deferral is stated up front, the provider-centric diagram is
  replaced with a config-is-the-source-of-truth one, and there is a real
  getting-started (`a2a new` → `validate` → `doctor` → `run`) plus fleet and
  control-plane sections. `a2a-agents/README.md` and `CLAUDE.md` track the same
  surface (subcommand table, fleets, pre-flight, restart-recovery); `CLAUDE.md`
  also gained the missing `a2acli` row and dropped the stale "all crates are
  0.3.0" claim — release-plz versions each crate independently and they have
  diverged.
- ⬜ `terraform-provider-a2aagent/README.md` still describes the provider as the
  source of truth for agent definitions. Fix when the provider resumes — or
  sooner, with a parked-WIP banner, if it starts misleading anyone.

---

## Finish-work bugs (from review)

- ✅ `expand_env_vars` now honours `${VAR:-default}` (widened regex; missing var
  uses the default, else a hard error) (`a2a-agents/src/core/config.rs`).
- ✅ `handler.type` is a typed `HandlerType { Echo, Llm, Reimbursement,
  Custom(String) }` enum (parse, don't validate), replacing the `!= "echo"`
  check; `bin/a2a.rs` matches on it with a `Custom`/unsupported→echo fallback.
- ✅ `reasoning_enabled` is driven by `LlmProvider::supports_reasoning()` (true
  for the OpenRouter provider via `OpenAiConfig`), not env-sniffing
  (`handlers/llm.rs`).
- ✅ `AgentId` `From`/`FromStr` route through `slugify` so non-canonical lookup
  keys (HTTP path param, config `agent_id` ref) resolve instead of silently
  missing the slugified keyspace (`registry/mod.rs`).
- ✅ `slugify` collapses separator runs and `expand_env_vars` matches
  lower/mixed-case var names (was `[A-Z_]`-only, so `${database_url}` passed
  through literally instead of erroring) (`utils/mod.rs`, `core/config.rs`).
- ✅ `ControlPlane::deploy` takes an already-parsed `AgentBuilder` instead of
  re-reading the file: the TOML is parsed once, the HTTP adapter writes it with
  `tokio::fs` (no blocking I/O on the executor), and the service no longer
  touches the filesystem (hex rule 9a) (`control_plane/{mod,http}.rs`).
