# A2A Client

A simple web-based chat client for interacting with A2A (Agent-to-Agent) protocol agents. This client provides a clean, server-side rendered interface for communicating with agents like the reimbursement agent.

## Features

- 🌐 **Web-based interface** - No client installation required
- 💬 **Real-time chat** - Send and receive messages with A2A agents
- 🔄 **Auto-refresh** - Automatically updates chat history every 5 seconds
- 🎨 **Clean UI** - Simple, responsive design with styled messages
- 🚀 **Fast** - Server-side rendered with Askama templates
- 🔧 **Easy setup** - Single binary, minimal configuration

## Quick Start

### Prerequisites

- Rust 1.70 or later
- An A2A agent running (e.g., reimbursement agent on `http://localhost:8080`)

### Building

```bash
cd a2a-client
cargo build --release --bin server
```

### Running

```bash
# Run with default settings (connects to http://localhost:8080)
cargo run --bin server

# Or specify a custom agent URL
AGENT_URL=http://my-agent:8080 cargo run --bin server

# With logging enabled
RUST_LOG=info cargo run --bin server
```

The server will start on `http://localhost:3000`.

## Usage

1. **Start the server** using one of the commands above
2. **Open your browser** and navigate to `http://localhost:3000`
3. **Enter the agent URL** (or use the default `http://localhost:8080`)
4. **Click "Start New Chat"** to begin a conversation
5. **Type your message** and click "Send" to interact with the agent

## Architecture

The client is built with:

- **[Axum](https://github.com/tokio-rs/axum)** - Web framework
- **[Askama](https://github.com/djc/askama)** - Type-safe templating
- **[a2a-rs](../a2a-rs)** - A2A protocol implementation
- **Server-side rendering** - No JavaScript required

### Project Structure

```
a2a-client/
├── src/
│   ├── bin/
│   │   └── server.rs      # Main server binary
│   └── styles.css         # CSS styles
├── templates/
│   ├── index.html         # Home page template
│   └── chat.html          # Chat interface template
├── Cargo.toml
└── README.md
```

## Configuration

### Environment Variables

- `AGENT_URL` - Default agent URL (default: `http://localhost:8080`)
- `RUST_LOG` - Log level (e.g., `info`, `debug`, `trace`)

### Server Settings

The server runs on port 3000 by default. To change this, modify the address in `src/bin/server.rs`:

```rust
let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
```

## Message Format

The client uses the A2A protocol message format:

- **User messages** are sent with `Role::User`
- **Text content** is wrapped in message parts
- **Message IDs** are automatically generated UUIDs
- **Task IDs** are used to group conversations

## Styling

The UI uses a clean, modern design with:

- Distinct styling for user and agent messages
- Responsive layout that works on mobile
- Auto-scrolling to the latest message
- Clear visual hierarchy

To customize the appearance, edit `src/styles.css`.

## Development

### Adding Features

To extend the client:

1. **New routes** - Add handlers in `src/bin/server.rs`
2. **New templates** - Create `.html` files in `templates/`
3. **API changes** - Update the message handling logic
4. **Styling** - Modify `src/styles.css`

### Testing

Run the client with a mock agent or the reimbursement agent:

```bash
# Terminal 1: Start the reimbursement agent
cd ../a2a-agents
cargo run --bin reimbursement_server

# Terminal 2: Start the client
cd ../a2a-client
RUST_LOG=info cargo run --bin server

# Terminal 3: Test with curl
curl -X POST http://localhost:3000/chat/new \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -d "agent_url=http://localhost:8080"
```

## Troubleshooting

### Common Issues

1. **"Failed to connect to agent"**
   - Ensure the agent is running on the specified URL
   - Check firewall settings
   - Verify the agent URL is correct

2. **"No messages appearing"**
   - Check the browser console for errors
   - Ensure the task ID is valid
   - Verify the agent is responding

3. **"Build errors"**
   - Run `cargo clean` and rebuild
   - Ensure you're using Rust 1.70+
   - Check all dependencies are available

### Debug Mode

Enable detailed logging:

```bash
RUST_LOG=debug cargo run --bin server
```

## License

This project is part of the a2a-rs workspace. See the main project for license information.