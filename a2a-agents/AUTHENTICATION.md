# Authentication Configuration

The reimbursement agent supports multiple authentication methods to secure API endpoints.

## Authentication Types

### 1. No Authentication (Default)

For development and testing, the agent runs without authentication by default:

```json
{
  "auth": {
    "type": "None"
  }
}
```

```bash
# No authentication required
curl -H "Content-Type: application/json" \
     -d '{"jsonrpc":"2.0","method":"tasks/sendMessage","params":{...},"id":1}' \
     http://localhost:8080/
```

### 2. Bearer Token Authentication

Secure API access using bearer tokens:

```json
{
  "auth": {
    "type": "BearerToken",
    "tokens": ["secret-token-123", "another-token-456"],
    "format": "JWT"
  }
}
```

**Client Usage:**
```bash
curl -H "Authorization: Bearer secret-token-123" \
     -H "Content-Type: application/json" \
     -d '{"jsonrpc":"2.0","method":"tasks/sendMessage","params":{...},"id":1}' \
     http://localhost:8080/
```

### 3. API Key Authentication

Secure API access using API keys in headers, query parameters, or cookies:

#### Header-based API Keys
```json
{
  "auth": {
    "type": "ApiKey",
    "keys": ["api-key-123", "api-key-456"],
    "location": "header",
    "name": "X-API-Key"
  }
}
```

**Client Usage:**
```bash
curl -H "X-API-Key: api-key-123" \
     -H "Content-Type: application/json" \
     -d '{"jsonrpc":"2.0","method":"tasks/sendMessage","params":{...},"id":1}' \
     http://localhost:8080/
```

#### Query Parameter API Keys
```json
{
  "auth": {
    "type": "ApiKey",
    "keys": ["api-key-123"],
    "location": "query",
    "name": "api_key"
  }
}
```

**Client Usage:**
```bash
curl -H "Content-Type: application/json" \
     -d '{"jsonrpc":"2.0","method":"tasks/sendMessage","params":{...},"id":1}' \
     "http://localhost:8080/?api_key=api-key-123"
```

#### Cookie-based API Keys
```json
{
  "auth": {
    "type": "ApiKey",
    "keys": ["session-token-123"],
    "location": "cookie",
    "name": "session_token"
  }
}
```

**Client Usage:**
```bash
curl -H "Cookie: session_token=session-token-123" \
     -H "Content-Type: application/json" \
     -d '{"jsonrpc":"2.0","method":"tasks/sendMessage","params":{...},"id":1}' \
     http://localhost:8080/
```

## Environment Variables

Configure authentication using environment variables:

### Bearer Token
```bash
export AUTH_BEARER_TOKENS="token1,token2,token3"
export AUTH_BEARER_FORMAT="JWT"  # Optional
```

### API Key
```bash
export AUTH_API_KEYS="key1,key2,key3"
export AUTH_API_KEY_LOCATION="header"  # or "query" or "cookie"
export AUTH_API_KEY_NAME="X-API-Key"   # Header/query/cookie name
```

## Configuration Examples

### Development (No Auth)
```bash
cargo run --bin reimbursement_server
```

### Production with Bearer Token
```bash
AUTH_BEARER_TOKENS="prod-token-123" \
cargo run --bin reimbursement_server --features auth
```

### Production with API Key Header
```bash
AUTH_API_KEYS="prod-api-key-123" \
AUTH_API_KEY_LOCATION="header" \
AUTH_API_KEY_NAME="X-API-Key" \
cargo run --bin reimbursement_server --features auth
```

### Using Configuration File
```bash
cargo run --bin reimbursement_server --features auth -- --config config.auth.example.json
```

## Security Considerations

### Token Management
- **Rotate tokens regularly** in production environments
- **Store tokens securely** (environment variables, secret management systems)
- **Use HTTPS** in production to protect tokens in transit
- **Validate token format** if using structured tokens like JWT

### API Key Security
- **Generate strong, random API keys** (recommended: 32+ characters)
- **Implement rate limiting** to prevent brute force attacks
- **Log authentication attempts** for security monitoring
- **Use least privilege principle** - only grant necessary permissions

### Best Practices
- **Enable authentication** for all production deployments
- **Use bearer tokens** for stateless authentication
- **Use API keys** for service-to-service communication
- **Monitor authentication logs** for suspicious activity
- **Implement token expiration** for enhanced security

## Error Responses

### Missing Authentication
```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32001,
    "message": "Authentication required"
  },
  "id": 1
}
```

### Invalid Token/Key
```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32001,
    "message": "Invalid authentication credentials"
  },
  "id": 1
}
```

## WebSocket Authentication

WebSocket connections support the same authentication methods:

- **Bearer tokens**: Include in `Authorization` header during WebSocket handshake
- **API keys**: Include in appropriate header/query parameter during connection

```javascript
// WebSocket with bearer token
const ws = new WebSocket('ws://localhost:8081', [], {
  headers: {
    'Authorization': 'Bearer secret-token-123'
  }
});

// WebSocket with API key in query
const ws = new WebSocket('ws://localhost:8081?api_key=api-key-123');
```

## Future Authentication Methods

The framework supports additional authentication methods that can be enabled:

- **JWT with signature verification**
- **OAuth2 flows** (Authorization Code, Client Credentials)
- **OpenID Connect** integration
- **mTLS** (mutual TLS) authentication

Contact the development team for assistance implementing these advanced authentication methods.