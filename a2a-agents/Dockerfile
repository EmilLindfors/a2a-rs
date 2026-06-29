# Base image for `ContainerRuntime` — one image, agent config injected per run.
#
# Build from the workspace root so the build context includes every path-dep crate:
#   docker build -t a2a-agents:latest -f a2a-agents/Dockerfile .
#
# Run an agent by mounting its TOML at /etc/agent.toml (what ContainerRuntime does):
#   docker run --rm -e HOST=0.0.0.0 -p 8080:8080 \
#     -v "$PWD/agent.toml:/etc/agent.toml:ro" a2a-agents:latest

# ---- builder ---------------------------------------------------------------
FROM rust:1.85-bookworm AS builder
WORKDIR /src

# Copy the whole workspace (the a2a binary depends on several in-workspace crates).
COPY . .

# Build just the `a2a` binary with the features its subcommands need.
RUN cargo build --release -p a2a-agents --bin a2a --features mcp-server,schema

# ---- runtime ---------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

# TLS roots for the outbound HTTP/LLM clients.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /src/target/release/a2a /usr/local/bin/a2a

# Bind all interfaces so the published port is reachable (configs that omit
# `host` pick this up via `default_host`).
ENV HOST=0.0.0.0

# Run the agent whose config is mounted at /etc/agent.toml.
ENTRYPOINT ["a2a"]
CMD ["run", "--config", "/etc/agent.toml"]
