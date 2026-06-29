# terraform-provider-a2aagent

A Terraform provider for declaring A2A agents as code. It is a
**config-as-artifact** provider: it is the source of truth for agent
definitions and renders the TOML config files that the [`a2a` binary]
(../a2a-agents/bin/a2a.rs) consumes. It does **not** provision runtime
infrastructure — container/k8s/deployment stays in your existing tooling.

## Resources

- `a2aagent_agent` — maps 1:1 to one `AgentConfig`. On create/update it
  renders `<name>.toml` into the configured `output_dir` and records the path
  in state.
- `a2aagent_agent_set` — groups multiple `a2aagent_agent`s and emits a
  manifest file listing config paths. The `a2a` binary accepts repeated
  `--config` args to run many agents in one process.

## Validation source

The single source of truth for "what is a valid agent config" is the
`AgentConfig` type in `a2a-agents/src/core/config.rs`, exported as a JSON
Schema via the `schema` feature:

```sh
# Regenerate the schema fixture bundled into the provider:
cargo run -p a2a-agents --example print_schema --features schema -- > internal/schema/agent_config.json
```

When an `a2a` binary is configured (`a2a_bin`), the provider validates configs
by shelling out to `a2a validate`. Otherwise it falls back to the bundled JSON
Schema. This keeps Rust the single validator.

## Build

```sh
# 1. Generate the JSON Schema fixture from the Rust config types.
cargo run -p a2a-agents --example print_schema --features schema -- \
  > internal/schema/agent_config.json

# 2. Build the provider.
go generate ./...    # (if codegen is wired)
go build .
go test ./...
```

## Run the generated agents

After `terraform apply`, run the rendered configs:

```sh
terraform output -raw manifest | xargs a2a run --config
# or, pointing at a single agent:
a2a run --config generated/llm-agent.toml
```
