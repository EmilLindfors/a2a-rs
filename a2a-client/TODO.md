# A2A Client TODO

## High Priority

### Core Features
- [ ] **WebSocket support** - Add real-time WebSocket connection option alongside HTTP
- [ ] **Streaming responses** - Display agent responses as they stream in
- [ ] **Multiple agent support** - Allow switching between different agents in the UI
- [ ] **Session persistence** - Save chat history to local storage or database
- [ ] **Authentication** - Add proper authentication token support in the UI

### UI/UX Improvements
- [ ] **Remove auto-refresh** - Replace with WebSocket or SSE for real-time updates
- [ ] **Loading states** - Show spinner while waiting for agent responses
- [ ] **Error handling UI** - Better error messages and retry options
- [ ] **Markdown rendering** - Support markdown in agent responses
- [ ] **Code syntax highlighting** - Highlight code blocks in messages

## Medium Priority

### Features
- [ ] **File uploads** - Support sending files to agents
- [ ] **Artifact display** - Show agent artifacts (images, documents, etc.)
- [ ] **Export chat** - Download conversation as text/markdown/PDF
- [ ] **Chat history browser** - View and resume previous conversations
- [ ] **Multi-turn context** - Better handling of conversation context

### Technical Improvements
- [ ] **Configuration file** - Support config.toml for server settings
- [ ] **Health check endpoint** - Add /health for monitoring

### Developer Experience
- [ ] **API documentation** - Generate OpenAPI/Swagger docs

## Low Priority

### Advanced Features
- [ ] **Multi-agent chat** - Chat with multiple agents simultaneously
- [ ] **Agent discovery** - Auto-discover available agents on the network

### Performance
- [ ] **Response caching** - Cache agent responses where appropriate
- [ ] **Compression** - Enable gzip/brotli compression
- [ ] **Static asset optimization** - Bundle and minify CSS
- [ ] **Connection pooling** - Reuse HTTP connections to agents

### Security
- [ ] **CORS configuration** - Proper CORS setup for production
- [ ] **Rate limiting** - Prevent abuse with rate limits
- [ ] **HTTPS support** - Built-in TLS certificate handling
- [ ] **Content Security Policy** - Add CSP headers
- [ ] **Input sanitization** - Prevent XSS attacks

## Future Ideas

### Experimental
- [ ] **Plugin system** - Allow custom message handlers/transformers
- [ ] **Agent SDK** - JavaScript SDK for embedding the chat

### Integrations
- [ ] **Email gateway** - Interact with agents via email
- [ ] **Webhook support** - Send agent responses to webhooks
- [ ] **Slack integration** - Chat with agents via Slack
- [ ] **Discord bot** - Discord bot interface for agents

## Known Issues

### Bugs to Fix
- [ ] **Agent URL validation** - Currently accepts any string as agent URL
- [ ] **Message ordering** - Ensure messages always appear in correct order
- [ ] **Large message handling** - UI breaks with very long messages
- [ ] **Concurrent requests** - Handle multiple users on same task ID
- [ ] **Memory leak** - Investigate potential memory leak in long sessions

### Technical Debt
- [ ] **Error types** - Create proper error types instead of anyhow
- [ ] **State management** - Consider using a proper state store
- [ ] **Template organization** - Split large templates into partials
- [ ] **CSS architecture** - Consider using CSS modules or similar

## Notes

- The current implementation uses HTTP polling with a 5-second refresh. This should be replaced with WebSocket or Server-Sent Events for a better user experience.
- The client currently creates a new HttpClient for each request. Consider implementing connection pooling.
- The UI is intentionally simple to focus on functionality. A more sophisticated design system could be implemented.
- Consider supporting the full A2A protocol specification as it evolves.