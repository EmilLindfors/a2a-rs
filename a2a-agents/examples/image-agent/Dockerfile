# An agent that is its own image — the escape hatch from the declarative layer.
#
# Build from the workspace root so the context includes every path-dep crate:
#   docker build -t a2a-image-agent:latest -f a2a-agents/examples/image-agent/Dockerfile .
#
# Run it by hand exactly as `ContainerRuntime` would (see README.md):
#   docker run --rm -e HOST=0.0.0.0 -e A2A_CONFIG=/etc/agent.toml -p 8300:8300 \
#     -v "$PWD/a2a-agents/examples/image-agent/agent.toml:/etc/agent.toml:ro" \
#     a2a-image-agent:latest

# ---- builder ---------------------------------------------------------------
# Same floor as `a2a-agents/Dockerfile`; the dependency tree can raise it.
FROM rust:1.96-bookworm AS builder
WORKDIR /src

COPY . .

# The example *is* the agent. A real one would be `--bin` in your own crate,
# built against the published `a2a-agents` — nothing here depends on being
# inside this workspace.
RUN cargo build --release -p a2a-agents --example image-agent

# ---- runtime ---------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

# TLS roots, for an agent that calls out (this one does not; yours will).
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /src/target/release/examples/image-agent /usr/local/bin/shipping-quotes

# Unprivileged, like the base image: an agent owns nothing and writes nothing, so
# root buys only blast radius. Declared numerically as well as by name so
# orchestrators enforcing `runAsNonRoot` can see it. Consequence: the config's
# `http_port` must be above 1024.
RUN useradd --system --uid 10001 --user-group --no-create-home \
    --shell /usr/sbin/nologin agent
USER 10001:10001

# Both are set by the platform on every agent container; repeating them here is
# what makes a plain `docker run` behave the same way.
ENV HOST=0.0.0.0
ENV A2A_CONFIG=/etc/agent.toml

# No `CMD`: the platform starts an agent's own image with no command override, so
# this entrypoint is the whole story.
ENTRYPOINT ["shipping-quotes"]
