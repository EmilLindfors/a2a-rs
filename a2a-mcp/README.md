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

Both run end-to-end with no external setup — they wire up the A2A and MCP
sides in-process over an in-memory duplex transport.

- `cargo run --example a2a_as_mcp_server -p a2a-mcp` — spins up a tiny A2A
  HTTP agent, bridges it with `AgentToMcpBridge`, and demonstrates an MCP
  client listing and calling its tools.
- `cargo run --example a2a_with_mcp_tools -p a2a-mcp` — wraps an A2A handler
  with `McpToA2ABridge` so `TOOL_CALL: <name>` messages get routed to an
  in-process MCP server.

## Architecture

```mermaid
flowchart TD
    subgraph A2A[A2A Protocol]
        A2AAgent[A2A Agent]
        A2AClient[A2A Client]
    end

    subgraph Bridge[a2a-mcp Bridge]
        A2AMCPBridge[AgentToMcpBridge\n(A2A Agent as MCP Server)]
        MCPA2ABridge[McpToA2ABridge\n(MCP Server as A2A Agent)]
        
        A2AMCPBridge <--> |MessageConverter| Converters
        MCPA2ABridge <--> |MessageConverter| Converters
    end

    subgraph MCP[MCP Protocol]
        MCPClient[MCP Client]
        MCPServer[MCP Server]
    end

    A2AAgent <--> |A2A Messages| A2AMCPBridge
    A2AMCPBridge <--> |MCP JSON-RPC| MCPClient
    
    A2AClient <--> |A2A Messages| MCPA2ABridge
    MCPA2ABridge <--> |MCP JSON-RPC| MCPServer
```

## Skill schema extension v1

An A2A `AgentSkill` has no input schema, output schema, title or effect
hints, and an MCP tool has all four. Without them a skill served as a tool
takes one `message` string, and a tool served as a skill loses its type.
The gap is closed by an `AgentExtension` on the card, because
`AgentSkill` has no metadata field and extensions are the one place the spec
leaves for this.

The extension's `uri` is the address of this section:

```
https://github.com/EmilLindfors/a2a-rs/blob/master/a2a-mcp/README.md#skill-schema-extension-v1
```

`required` is `false`. A client that does not read the extension still
talks to the agent, one string at a time. `params` is keyed by skill id:

```json
{
  "uri": "https://github.com/EmilLindfors/a2a-rs/blob/master/a2a-mcp/README.md#skill-schema-extension-v1",
  "required": false,
  "params": {
    "skills": {
      "query": {
        "inputSchema": {
          "type": "object",
          "properties": {
            "view": { "type": "string", "enum": ["harvest", "feed"] },
            "year": { "type": "integer" }
          },
          "required": ["view"]
        },
        "outputSchema": { "type": "object" },
        "title": "Query a view",
        "annotations": { "readOnly": true, "openWorld": false }
      }
    }
  }
}
```

Every field of a skill's entry is optional. `annotations` uses `a2a-llm`'s
`ToolAnnotations` names: `readOnly`, `destructive`, `idempotent`,
`openWorld`. In Rust, `SkillSchemas::to_extension` builds the extension and
`SkillSchemas::from_card` reads it.

What the bridges do with it:

- `AgentToMcpBridge` serves a skill with an `inputSchema` as a tool whose
  input schema is that schema, with an optional `task_id` string property
  added unless the schema declares one. A suspended task is continued by
  calling again with the `task_id` from the result. The `outputSchema`,
  `title` and `annotations` go on the tool as they are.
- A call to a typed tool reaches the agent as one data part holding the
  arguments, without `task_id`. An untyped skill still gets a text part.
- With an `outputSchema`, a single data part in the agent's final message
  (or in its single artifact) becomes the tool result's `structuredContent`
  beside the text. The value is not validated against the schema.
- `McpToA2ABridge::agent_skills` produces skills and the extension from a
  server's tools, so a typed tool round-trips through both bridges.

Tool names are `{namespace}_{skill id}`, where the namespace is the agent's
`name` lowercased and reduced to `[a-z0-9_]`, prefixed with `agent_` when it
does not start with a letter. Gemini caps a function name at 64 characters;
a long name is shortened with `AgentToMcpBridge::with_namespace`.

## Development Status

See the workspace [TODO.md](../TODO.md) for open and deferred work.