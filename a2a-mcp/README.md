# A2A-RMCP Integration

A bridge between Agent-to-Agent (A2A) protocol and Rusty Model Context Protocol (RMCP)

## Overview

This crate provides integration between the A2A protocol and RMCP, enabling bidirectional communication between these protocols. It follows a bridge pattern with adapter layers for message conversion and protocol translation.

## Key Features

- Use A2A agents as RMCP tools
- Expose RMCP tools as A2A agents
- Bidirectional message conversion
- State management across protocols

## Examples

See the `examples` directory for working demonstrations:

```rust
cargo run --example minimal_example
```

## Architecture

```text
┌─────────────────────────────────────────────┐
│               a2a-mcp Crate                 │
├─────────────┬─────────────┬─────────────────┤
│ RMCP Client │ Translation │    A2A Client   │
│ Interface   │    Layer    │    Interface    │
├─────────────┼─────────────┼─────────────────┤
│ RMCP Server │ Conversion  │    A2A Server   │
│ Interface   │    Layer    │    Interface    │
└─────────────┴─────────────┴─────────────────┘
```

## Development Status

See [TODO.md](TODO.md) for current implementation status and next steps.