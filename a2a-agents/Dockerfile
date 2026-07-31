# Base image for `ContainerRuntime` — one image, agent config injected per run.
#
# Build from the workspace root so the build context includes every path-dep crate:
#   docker build -t a2a-agents:latest -f a2a-agents/Dockerfile .
#
# Run an agent by mounting its TOML at /etc/agent.toml (what ContainerRuntime does):
#   docker run --rm -e HOST=0.0.0.0 -p 8080:8080 \
#     -v "$PWD/agent.toml:/etc/agent.toml:ro" a2a-agents:latest

# ---- builder ---------------------------------------------------------------
# Keep in step with `rust-version` in the workspace `[workspace.package]`, and
# note it must also satisfy what the *dependency tree* requires, which can be
# higher: transitive deps raise their floor over time, and cargo refuses the
# whole build when one wants more than the toolchain offers. This is the line to
# bump when that happens — the symptom is a wall of "requires rustc 1.x" during
# `docker build`.
FROM rust:1.96-bookworm AS builder
WORKDIR /src

# Copy the whole workspace (the a2a binary depends on several in-workspace crates).
COPY . .

# Build just the `a2a` binary. Its `required-features` are the crate defaults,
# so no `--features` is needed — and keeping it that way means this image builds
# exactly what `cargo install a2a-agents` produces, rather than a variant only
# this Dockerfile knows how to make.
#
# The reimbursement *sample* agent is deliberately absent: this image runs
# whatever configs get deployed to it, and a demo handler is not one of them.
RUN cargo build --release -p a2a-agents --bin a2a

# ---- runtime ---------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

# TLS roots for the outbound HTTP/LLM clients.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /src/target/release/a2a /usr/local/bin/a2a

# Run as an unprivileged user. An agent is an HTTP server reading a mounted
# config: it needs to own nothing and write nothing, so root buys only blast
# radius. Declared numerically as well as by name so orchestrators that enforce
# `runAsNonRoot` can see it without resolving /etc/passwd.
RUN useradd --system --uid 10001 --user-group --no-create-home \
    --shell /usr/sbin/nologin agent
USER 10001:10001

# Consequence worth knowing: a non-root process cannot bind a port below 1024,
# so an agent config must use a high `http_port`. Publish it on whatever host
# port you like (`-p 80:8080`) — the restriction is inside the container only.

# Bind all interfaces so the published port is reachable (configs that omit
# `host` pick this up via `default_host`).
ENV HOST=0.0.0.0

# Run the agent whose config is mounted at /etc/agent.toml.
ENTRYPOINT ["a2a"]
CMD ["run", "--config", "/etc/agent.toml"]
