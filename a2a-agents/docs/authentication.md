# Authentication Configuration Guide

This guide explains how to configure authentication for A2A agents using TOML configuration files.

## Overview

The agent builder supports multiple authentication schemes that can be configured declaratively:

1. **None** - No authentication (development only)
2. **Bearer Token** - Simple token-based authentication
3. **API Key** - Key-based authentication (header/query/cookie)
4. **JWT** - JSON Web Token authentication with HMAC or RSA
5. **OAuth2** - OAuth2 authentication (authorization code or client credentials flow)

## Configuration Format

All authentication is configured in the `[server.auth]` section of your TOML file.

### 1. No Authentication (Development)

```toml
[server.auth]
type = "none"
```

**Use when:** Local development, internal testing
**Security:** ⚠️ Not suitable for production

---

### 2. Bearer Token Authentication

Simple token-based authentication where clients send a token in the Authorization header.

```toml
[server.auth]
type = "bearer"
tokens = ["${API_TOKEN}", "backup-token-123"]
format = "JWT"  # Optional, describes token format
```

**Configuration:**
- `tokens` - List of valid bearer tokens (supports environment variables)
- `format` - Optional description (e.g., "JWT", "Opaque")

**Client Usage:**
```http
Authorization: Bearer your-token-here
```

**Use when:** Simple API protection, internal services
**Security:** ✅ Suitable for production with strong tokens

---

### 3. API Key Authentication

Key-based authentication that can be sent in headers, query parameters, or cookies.

```toml
[server.auth]
type = "apikey"
keys = ["${API_KEY}", "backup-key-456"]
location = "header"  # Options: "header", "query", "cookie"
name = "X-API-Key"   # Header/param/cookie name
```

**Configuration:**
- `keys` - List of valid API keys
- `location` - Where to look for the key (default: "header")
- `name` - Name of the header/parameter/cookie (default: "X-API-Key")

**Client Usage:**
```http
# Header
X-API-Key: your-api-key-here

# Query parameter
GET /api/endpoint?X-API-Key=your-api-key-here

# Cookie
Cookie: X-API-Key=your-api-key-here
```

**Use when:** Third-party API access, webhook callbacks
**Security:** ⚠️ Currently not implemented in runtime (falls back to no auth)

---

### 4. JWT Authentication

JSON Web Token authentication with support for HMAC and RSA algorithms.

#### HMAC-based JWT (HS256/HS384/HS512)

```toml
[server.auth]
type = "jwt"
secret = "${JWT_SECRET}"  # Shared secret for HMAC
algorithm = "HS256"       # HS256, HS384, or HS512
issuer = "https://auth.example.com"    # Optional: validate iss claim
audience = "api://my-agent"             # Optional: validate aud claim
```

#### RSA-based JWT (RS256/RS384/RS512)

```toml
[server.auth]
type = "jwt"
rsa_pem_path = "/path/to/public_key.pem"  # RSA public key
algorithm = "RS256"                        # RS256, RS384, or RS512
issuer = "https://auth.example.com"       # Optional
audience = "api://my-agent"                # Optional
```

**Configuration:**
- `secret` - Shared secret for HMAC algorithms (HS256/384/512)
- `rsa_pem_path` - Path to RSA public key PEM file for RSA algorithms
- `algorithm` - JWT signing algorithm (default: "HS256")
- `issuer` - Optional: required issuer (iss claim validation)
- `audience` - Optional: required audience (aud claim validation)

**Required Claims:**
- `sub` - Subject (user ID)
- `exp` - Expiration time (Unix timestamp)
- `iat` - Issued at (Unix timestamp)

**Client Usage:**
```http
Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...
```

**Use when:** User authentication, microservices, federated identity
**Security:** ✅ Highly secure, industry standard

**Requires:** `auth` feature flag
```toml
# Cargo.toml
a2a-agents = { features = ["auth"] }
```

---

### 5. OAuth2 Authentication

OAuth2 authentication supporting both authorization code and client credentials flows.

#### Authorization Code Flow (User Authentication)

Used when human users need to authenticate via a third-party provider.

```toml
[server.auth]
type = "oauth2"
client_id = "${OAUTH_CLIENT_ID}"
client_secret = "${OAUTH_CLIENT_SECRET}"
authorization_url = "https://provider.com/oauth/authorize"
token_url = "https://provider.com/oauth/token"
redirect_url = "http://localhost:8080/oauth/callback"
flow = "authorization_code"
scopes = ["read", "write", "email"]
```

#### Client Credentials Flow (Machine-to-Machine)

Used for service-to-service authentication without user interaction.

```toml
[server.auth]
type = "oauth2"
client_id = "${OAUTH_CLIENT_ID}"
client_secret = "${OAUTH_CLIENT_SECRET}"
authorization_url = "http://localhost"  # Not used in client_credentials
token_url = "https://provider.com/oauth/token"
flow = "client_credentials"
scopes = ["api.read", "api.write"]
```

**Configuration:**
- `client_id` - OAuth2 client ID from your provider
- `client_secret` - OAuth2 client secret (use environment variables!)
- `authorization_url` - Authorization endpoint URL
- `token_url` - Token endpoint URL
- `redirect_url` - Callback URL (authorization_code flow only)
- `flow` - OAuth2 flow type: "authorization_code" or "client_credentials"
- `scopes` - Required OAuth2 scopes

**Common OAuth2 Providers:**

**GitHub:**
```toml
authorization_url = "https://github.com/login/oauth/authorize"
token_url = "https://github.com/login/oauth/access_token"
scopes = ["user", "repo"]
```

**Google:**
```toml
authorization_url = "https://accounts.google.com/o/oauth2/v2/auth"
token_url = "https://oauth2.googleapis.com/token"
scopes = ["openid", "email", "profile"]
```

**Microsoft/Azure AD:**
```toml
authorization_url = "https://login.microsoftonline.com/{tenant}/oauth2/v2.0/authorize"
token_url = "https://login.microsoftonline.com/{tenant}/oauth2/v2.0/token"
scopes = ["openid", "profile", "email"]
```

**Auth0:**
```toml
authorization_url = "https://YOUR_DOMAIN.auth0.com/authorize"
token_url = "https://YOUR_DOMAIN.auth0.com/oauth/token"
scopes = ["openid", "profile", "email"]
```

**Use when:** Third-party login, social authentication, enterprise SSO
**Security:** ✅ Highly secure, delegated authentication

**Requires:** `auth` feature flag

---

## Environment Variables

All authentication configurations support environment variable interpolation using `${VAR_NAME}` syntax:

```toml
[server.auth]
type = "jwt"
secret = "${JWT_SECRET}"  # Read from JWT_SECRET environment variable
issuer = "${JWT_ISSUER}"   # Read from JWT_ISSUER environment variable
```

**Best Practices:**
- ✅ Always use environment variables for secrets
- ✅ Never commit secrets to version control
- ✅ Use different secrets for dev/staging/production
- ✅ Rotate secrets regularly

---

## Security Best Practices

### Token Storage
- Use environment variables or secure vaults
- Never hardcode secrets in configuration files
- Use different secrets per environment

### Token Rotation
- Implement token expiration
- Support token refresh where applicable
- Revoke compromised tokens immediately

### HTTPS in Production
```toml
[server]
host = "0.0.0.0"
http_port = 443  # Use HTTPS in production
```

Use a reverse proxy (nginx, Caddy) for TLS termination.

### Minimal Scopes
Only request the OAuth2 scopes you actually need:
```toml
scopes = ["read"]  # Don't request "write" if not needed
```

---

## Testing Authentication

### Without Auth (Development)
```bash
curl http://localhost:8080/agent/card
```

### With Bearer Token
```bash
curl -H "Authorization: Bearer your-token-here" \
  http://localhost:8080/agent/card
```

### With JWT
```bash
# Generate a test JWT at https://jwt.io
curl -H "Authorization: Bearer eyJhbGc..." \
  http://localhost:8080/agent/card
```

### With API Key
```bash
curl -H "X-API-Key: your-api-key-here" \
  http://localhost:8080/agent/card
```

---

## Enabling Authentication Features

JWT and OAuth2 require the `auth` feature flag.

**In your agent's Cargo.toml:**
```toml
[dependencies]
a2a-agents = { path = "../a2a-agents", features = ["auth"] }
```

**Building with auth:**
```bash
cargo build --features auth
cargo run --features auth --example my_agent
```

---

## Example Configurations

See the `examples/` directory for complete working examples:

- `examples/jwt_auth.toml` - JWT authentication example
- `examples/oauth2_auth.toml` - OAuth2 authorization code flow
- `examples/oauth2_client_credentials.toml` - OAuth2 client credentials flow

---

## Troubleshooting

### "auth feature not enabled"
Enable the `auth` feature in your Cargo.toml and rebuild:
```bash
cargo build --features auth
```

### "JWT validation failed"
- Check that the secret matches what was used to sign the token
- Verify the algorithm is correct (HS256 vs RS256, etc.)
- Ensure required claims (iss, aud) match configuration

### "Invalid OAuth2 URL"
- Verify authorization_url and token_url are valid HTTPS URLs
- Check for typos in provider URLs
- Ensure redirect_url is correctly configured

### Environment Variable Not Found
```toml
# ❌ Wrong - will fail if JWT_SECRET not set
secret = "${JWT_SECRET}"

# ✅ Better - provide default for development
secret = "${JWT_SECRET:-dev-secret-do-not-use-in-production}"
```

---

## Migration from Code-Based Auth

**Before (manual setup):**
```rust
let authenticator = JwtAuthenticator::new_with_secret(b"my-secret")
    .with_issuer("https://auth.example.com".to_string())
    .with_audience("api://my-agent".to_string());

let server = HttpServer::with_auth(processor, agent_info, bind_address, authenticator);
```

**After (TOML configuration):**
```toml
[server.auth]
type = "jwt"
secret = "${JWT_SECRET}"
issuer = "https://auth.example.com"
audience = "api://my-agent"
```

```rust
// Just use the builder - auth is automatic!
AgentBuilder::from_file("agent.toml")?
    .with_handler(handler)
    .build_with_auto_storage()
    .await?
    .run()
    .await?;
```

---

## Summary

| Auth Type | Security | Complexity | Use Case |
|-----------|----------|------------|----------|
| None | ⚠️ Low | Minimal | Development only |
| Bearer | ✅ Good | Low | Simple APIs |
| API Key | ✅ Good | Low | Third-party access |
| JWT | ✅ High | Medium | User auth, microservices |
| OAuth2 | ✅ High | High | Social login, SSO |

Choose the authentication method that best fits your security requirements and use case.
