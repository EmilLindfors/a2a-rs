# A2A Agent Multi-Turn Self-Play Examples

This directory contains working examples of **A2A (Agent-to-Agent) protocol patterns** using the `genai` crate for multi-provider LLM support.

## 🎯 What's Included

### 1. **Multi-Provider LLM Testing** (`src/main.rs`)
Tests the `genai` crate with multiple AI providers:
- ZAI (Zhipu AI)
- OpenAI
- Anthropic Claude
- Groq
- Ollama (local)

```bash
ZAI_API_KEY=your_key cargo run
```

### 2. **Simple Self-Play** (`examples/simple_self_play.rs`)
Two agents have a multi-turn philosophical conversation:
- **Socrates**: Asks probing Socratic questions
- **Student**: Responds thoughtfully and asks follow-ups
- Demonstrates: Multi-turn context, persona maintenance, natural dialogue

```bash
ZAI_API_KEY=your_key cargo run --example simple_self_play
```

**Example Output:**
```
[Turn 1] Student → Socrates
📨 What is the nature of knowledge?
💬 Before we can discuss its nature, must we not first agree on what
    knowledge itself is? How would you distinguish it from a simple opinion?

[Turn 2] Socrates → Student
📨 Before we can discuss its nature...
💬 I've been wondering if the key difference is that knowledge must be
    backed by evidence or reason...
```

### 3. **Tool-Calling Agents** (`examples/tool_calling_agents.rs`)
Agents that can call specialized tools during conversations:
- **MathAgent**: Uses calculator tool for computations
- **Researcher**: Uses search tool for information retrieval
- Demonstrates: Tool definition, execution, and result integration

```bash
ZAI_API_KEY=your_key cargo run --example tool_calling_agents
```

**Example Output:**
```
Query: What is 42 multiplied by 17?
🤖 MathAgent thinking...
   🔧 MathAgent has 1 tools available
   📞 Calling 1 tool(s)...
      • calculate({"a":42,"b":17,"operation":"multiply"})
💬 42 multiplied by 17 equals 714.
   ✅ Tools used: calculate
```

## 🏗️ Architecture

All examples follow the **A2A protocol pattern**:

```
┌─────────────┐                    ┌─────────────┐
│   Agent 1   │ ◄──── Message ───► │   Agent 2   │
│  (Persona)  │                    │  (Persona)  │
│     +       │                    │     +       │
│   LLM API   │                    │   LLM API   │
│  (genai)    │                    │  (genai)    │
└─────────────┘                    └─────────────┘
      │                                  │
      └──────── Conversation History ───┘
```

### Key Components

1. **Agent Structure**
   ```rust
   struct Agent {
       name: String,
       persona: String,      // System prompt
       model: String,        // LLM model to use
       history: Vec<ChatMessage>,  // Conversation context
   }
   ```

2. **Message Passing**
   - Agents exchange messages in turns
   - Each agent maintains full conversation history
   - Responses build on previous context

3. **Tool Integration** (optional)
   - Agents can have specialized tools
   - Tools defined with JSON schemas
   - LLM decides when to call tools
   - Results integrated into responses

## 🚀 Running the Examples

### Prerequisites
```bash
# Install Rust (if not already)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Set API key
export ZAI_API_KEY=your_key_here
```

### Build All Examples
```bash
cargo build --examples
```

### Run Specific Example
```bash
# Multi-provider test
cargo run

# Self-play conversation
cargo run --example simple_self_play

# Tool calling
cargo run --example tool_calling_agents
```

## 📚 Learned from rust-genai Examples

These implementations are based on patterns from [jeremychone/rust-genai](https://github.com/jeremychone/rust-genai):

1. **Multi-turn conversations** (`c01-conv.rs`) → `simple_self_play.rs`
2. **Tool/function calling** (`c20-tooluse.rs`) → `tool_calling_agents.rs`
3. **Streaming responses** (`c21-tooluse-streaming.rs`) → Future work
4. **Multi-provider support** (`c00-readme.rs`) → `src/main.rs`

## 🔮 Future Enhancements

- [ ] **Streaming responses** - Real-time token streaming
- [ ] **Multi-agent debates** - 3+ agents with moderator
- [ ] **RAG integration** - Agents with retrieval capabilities
- [ ] **Web search tools** - Real web search integration
- [ ] **Vision capabilities** - Image understanding agents
- [ ] **Full A2A protocol** - Integration with `a2a-rs` crate

## 🤝 A2A Protocol Concepts

These examples demonstrate core A2A concepts:

- **Autonomous Agents**: Each agent independently generates responses
- **Message Exchange**: Structured communication between agents
- **Context Preservation**: Full conversation history maintained
- **Tool Capabilities**: Agents can invoke external functions
- **Multi-turn Dialogue**: Coherent conversations over multiple exchanges

## 📖 Related Resources

- [genai crate](https://docs.rs/genai/latest/genai/)
- [rust-genai examples](https://github.com/jeremychone/rust-genai/tree/main/examples)
- [A2A Protocol Specification](https://a2a-protocol.org/)
- [a2a-rs crate](../a2a-rs/)

## ⚡ Quick Start

```bash
# Clone the repo
git clone https://github.com/seanchatmangpt/a2a-rs
cd a2a-rs/test-zai

# Set your API key
export ZAI_API_KEY=your_key

# Run self-play example
cargo run --example simple_self_play
```

You'll see two agents have a philosophical conversation about epistemology!

---

**Built with ❤️ using Rust + genai + A2A patterns**
