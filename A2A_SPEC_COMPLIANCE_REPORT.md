# A2A Protocol Specification Compliance Report

**Date**: 2025-11-23
**Implementation**: a2a-rs (Rust)
**Local Spec Version**: 0.3.0
**Official Spec Version**: 0.3.0
**Repository**: https://github.com/EmilLindfors/a2a-rs

---

## Executive Summary

The `a2a-rs` implementation is **largely compliant** with the official A2A Protocol v0.3.0 specification. The implementation demonstrates strong architectural design with hexagonal architecture, comprehensive feature coverage, and production-ready code quality. However, there are **several notable gaps** and **areas for optimization** that should be addressed.

### Quick Assessment

✅ **Strengths**:
- Complete v0.3.0 data model implementation
- All 11 core methods defined (9 fully working)
- Strong authentication framework (5 schemes)
- Dual storage backends (in-memory + SQLx)
- WebSocket streaming support
- Comprehensive error handling
- Good test coverage

⚠️ **Gaps**:
- New message API methods not implemented (message/send, message/stream)
- gRPC transport not implemented (only HTTP/WebSocket)
- Context ID hardcoded in several places
- AP2 payments extension placeholder only
- Mutual TLS defined but not implemented

---

## Detailed Compliance Analysis

### 1. Core Protocol Methods (11 total)

| Method | Status | Implementation Quality | Notes |
|--------|--------|----------------------|-------|
| **message/send** | ❌ **Not Implemented** | N/A | Returns UnsupportedOperation error. Legacy tasks/send works as alternative. |
| **message/stream** | ❌ **Not Implemented** | N/A | Returns UnsupportedOperation error. Legacy tasks/sendSubscribe works. |
| **tasks/get** | ✅ Fully Working | Excellent | Supports history length, proper error handling |
| **tasks/list** | ✅ Fully Working | Excellent | v0.3.0 feature with pagination, filtering, metadata |
| **tasks/cancel** | ✅ Fully Working | Good | Proper state validation |
| **tasks/resubscribe** | ✅ Fully Working | Good | WebSocket reconnection support |
| **tasks/pushNotificationConfig/set** | ✅ Fully Working | Excellent | Full implementation with authentication |
| **tasks/pushNotificationConfig/get** | ✅ Fully Working | Excellent | v0.3.0 enhanced with optional config ID |
| **tasks/pushNotificationConfig/list** | ✅ Fully Working | Excellent | v0.3.0 new method |
| **tasks/pushNotificationConfig/delete** | ✅ Fully Working | Excellent | v0.3.0 new method |
| **agent/getAuthenticatedExtendedCard** | ✅ Fully Working | Good | v0.3.0 new method |

**Legacy Methods** (Still Working):
- `tasks/send` - Works, preferred over unimplemented message/send
- `tasks/sendSubscribe` - Works, preferred over unimplemented message/stream

### 2. Transport Protocol Support

| Transport | Official Spec | Implementation | Notes |
|-----------|---------------|----------------|-------|
| **JSON-RPC over HTTP** | Required | ✅ Fully Implemented | Axum-based, production ready |
| **JSON-RPC over WebSocket** | Optional | ✅ Fully Implemented | tokio-tungstenite, streaming support |
| **gRPC** | Optional | ❌ **Not Implemented** | Types defined, no actual implementation |
| **HTTP+JSON (REST)** | Optional | ❌ **Not Implemented** | Only JSON-RPC binding exists |

**Issue**: The spec defines `TransportProtocol` enum with GRPC and HTTP+JSON, but only JSON-RPC transport is implemented.

**Recommendation**: Either implement gRPC/REST transports or document that only JSON-RPC is supported.

### 3. Data Model Compliance (v0.3.0)

#### AgentCard
| Field | Required | Status | Notes |
|-------|----------|--------|-------|
| name | ✅ Required | ✅ Implemented | |
| description | ✅ Required | ✅ Implemented | |
| version | ✅ Required | ✅ Implemented | |
| url | ✅ Required | ✅ Implemented | |
| protocolVersion | ✅ Required | ✅ Implemented | Defaults to "0.3.0" |
| preferredTransport | Optional | ✅ Implemented | Defaults to "JSONRPC" |
| additionalInterfaces | Optional | ✅ Implemented | Array of AgentInterface |
| iconUrl | Optional | ✅ Implemented | |
| capabilities | ✅ Required | ✅ Implemented | |
| skills | ✅ Required | ✅ Implemented | |
| security | Optional | ✅ Implemented | |
| securitySchemes | Optional | ✅ Implemented | |
| signatures | Optional | ✅ Implemented | Not validated |
| supportsAuthenticatedExtendedCard | Optional | ✅ Implemented | |
| defaultInputModes | ✅ Required | ✅ Implemented | |
| defaultOutputModes | ✅ Required | ✅ Implemented | |

**Issue**: AgentCardSignature is stored but not validated anywhere in the codebase.

**Recommendation**: Implement JWS signature validation for enhanced security.

#### Task
| Field | Required | Status | Notes |
|-------|----------|--------|-------|
| id | ✅ Required | ✅ Implemented | UUID-based |
| contextId | ✅ Required | ✅ Implemented | Server-generated |
| kind | ✅ Required | ✅ Implemented | Always "task" |
| status | ✅ Required | ✅ Implemented | TaskStatus with state |
| history | Optional | ✅ Implemented | Message array |
| artifacts | Optional | ✅ Implemented | Artifact array |
| metadata | Optional | ✅ Implemented | Extension support |

#### Message
| Field | Required | Status | Notes |
|-------|----------|--------|-------|
| messageId | ✅ Required | ✅ Implemented | UUID-based |
| role | ✅ Required | ✅ Implemented | "user" or "agent" |
| parts | ✅ Required | ✅ Implemented | Text/File/Data parts |
| kind | ✅ Required | ✅ Implemented | Always "message" |
| taskId | Optional | ✅ Implemented | |
| contextId | Optional | ✅ Implemented | |
| referenceTaskIds | Optional | ✅ Implemented | |
| extensions | Optional | ✅ Implemented | v0.3.0 feature |
| metadata | Optional | ✅ Implemented | Extension support |

### 4. Authentication & Security

| Security Scheme | Spec Status | Implementation | Notes |
|-----------------|-------------|----------------|-------|
| **API Key** | Supported | ✅ Fully Implemented | Header, query, cookie locations |
| **HTTP (Bearer)** | Supported | ✅ Fully Implemented | Bearer token authentication |
| **OAuth2** | Supported | ✅ Fully Implemented | All 4 flows (Auth Code, Client Credentials, Implicit, Password) |
| **OpenID Connect** | Supported | ✅ Fully Implemented | Discovery URL support |
| **Mutual TLS** | Supported | ⚠️ **Partial** | Type defined, no implementation |

**Issue**: MutualTLSSecurityScheme is defined in the domain model but has no actual authenticator implementation.

**Recommendation**: Implement mTLS authenticator or remove from supported schemes.

### 5. Storage & Persistence

| Feature | InMemoryTaskStorage | SqlxStorage |
|---------|---------------------|-------------|
| Task CRUD | ✅ Complete | ✅ Complete |
| Push notification configs | ✅ Complete | ✅ Complete |
| Task filtering (v0.3.0) | ✅ Complete | ✅ Complete |
| Pagination | ✅ Complete | ✅ Complete |
| Metadata support | ✅ Complete | ✅ Complete |
| Timestamp filtering | ✅ Complete | ✅ Complete |
| Context ID management | ⚠️ Hardcoded to "default" | ⚠️ Hardcoded to "default" |

**Critical Issue**: Context IDs are hardcoded to "default" in multiple locations:
- `adapter/storage/task_storage.rs:125, 214`
- `adapter/storage/sqlx_storage.rs:362, 406`

**Impact**: Multi-session context tracking doesn't work properly. All tasks share the same context.

**Recommendation**: Implement proper context ID extraction from messages and tasks.

### 6. Error Handling

All A2A-specific error codes are properly implemented:

| Error Code | Error Type | Status |
|------------|------------|--------|
| -32700 | JSONParseError | ✅ |
| -32600 | InvalidRequestError | ✅ |
| -32601 | MethodNotFoundError | ✅ |
| -32602 | InvalidParamsError | ✅ |
| -32603 | InternalError | ✅ |
| -32001 | TaskNotFoundError | ✅ |
| -32002 | TaskNotCancelableError | ✅ |
| -32003 | PushNotificationNotSupportedError | ✅ |
| -32004 | UnsupportedOperationError | ✅ |
| -32005 | ContentTypeNotSupportedError | ✅ |
| -32006 | InvalidAgentResponseError | ✅ |
| -32007 | AuthenticatedExtendedCardNotConfiguredError | ✅ |

### 7. Extensions & Features

#### v0.3.0 New Features
| Feature | Status | Notes |
|---------|--------|-------|
| Protocol versioning | ✅ Implemented | Defaults to "0.3.0" |
| Multiple transports | ⚠️ Partial | Types defined, only JSON-RPC works |
| Agent extensions | ✅ Implemented | URI-based extension support |
| Agent card signatures | ⚠️ Partial | Stored but not validated |
| Push notification IDs | ✅ Implemented | Multi-config support |
| Task filtering | ✅ Implemented | Status, context, timestamp filters |
| Pagination | ✅ Implemented | Page tokens, size limits |
| List push configs | ✅ Implemented | New v0.3.0 method |
| Delete push configs | ✅ Implemented | New v0.3.0 method |
| Authenticated extended card | ✅ Implemented | New v0.3.0 method |

#### AP2 (Agent Payments Protocol)
**Status**: ❌ **Not Implemented**

The codebase has an `a2a-ap2` crate but it only contains:
- Empty `main.rs`
- `AP2_IMPLEMENTATION_PLAN.md` documentation

**Recommendation**: Either implement AP2 or remove the placeholder crate.

---

## Critical Issues & Recommendations

### 🔴 Critical Issues

1. **Context ID Hardcoding** (Priority: HIGH)
   - **Files**:
     - `a2a-rs/src/adapter/storage/task_storage.rs:125, 214`
     - `a2a-rs/src/adapter/storage/sqlx_storage.rs:362, 406`
   - **Impact**: Multi-session context tracking is broken
   - **Fix**: Extract context ID from messages and tasks, implement proper context management

2. **Message API Not Implemented** (Priority: HIGH)
   - **Files**: `a2a-rs/src/adapter/business/request_processor.rs:387-409`
   - **Impact**: v0.3.0 message/send and message/stream methods return errors
   - **Workaround**: Legacy tasks/send and tasks/sendSubscribe still work
   - **Fix**: Implement proper message handling or deprecate these methods

### 🟡 Medium Priority Issues

3. **gRPC Transport Missing** (Priority: MEDIUM)
   - **Impact**: Spec advertises gRPC support but it doesn't work
   - **Recommendation**: Either implement gRPC or document HTTP/WS-only support

4. **Mutual TLS Not Implemented** (Priority: MEDIUM)
   - **Files**: `a2a-rs/src/domain/core/agent.rs` (type defined)
   - **Impact**: Security scheme advertised but not functional
   - **Recommendation**: Implement or remove from SecurityScheme enum

5. **Security Scheme Integration Incomplete** (Priority: MEDIUM)
   - **Files**: `a2a-rs/src/adapter/business/agent_info.rs:96`
   - **TODO**: "Implement SecurityScheme integration"
   - **Impact**: Security configuration not fully wired

6. **Signature Validation Missing** (Priority: MEDIUM)
   - **Impact**: AgentCard signatures stored but never validated
   - **Security Risk**: Signatures provide no actual protection
   - **Recommendation**: Implement JWS (RFC 7515) validation

### 🟢 Low Priority Issues

7. **Legacy Re-exports** (Priority: LOW)
   - **Files**: `a2a-rs/src/adapter/mod.rs:19`
   - **TODO**: "Remove these in a future version"
   - **Impact**: Technical debt, backward compatibility cruft

8. **AP2 Extension** (Priority: LOW)
   - **Status**: Placeholder only
   - **Recommendation**: Complete implementation or remove crate

---

## Performance & Optimization Opportunities

### 1. Database Query Optimization
The SQLx implementation could benefit from:
- Connection pooling configuration guidance
- Index recommendations for common queries
- Prepared statement caching

### 2. Streaming Performance
WebSocket implementation is solid but could improve:
- Backpressure handling in high-throughput scenarios
- Configurable buffer sizes
- Connection pool management for scaling

### 3. Memory Management
InMemoryTaskStorage could use:
- Configurable eviction policies
- Memory limits
- LRU cache for task history

---

## Comparison with Official Specification

### What's Missing from Official v0.3.0 Spec

Based on the official specification at https://a2a-protocol.org/latest/specification/:

1. **Proto File Authority**
   - Official spec uses `spec/a2a.proto` as normative reference
   - This implementation uses JSON Schema in `spec/*.json`
   - **Recommendation**: Consider generating types from official .proto file

2. **Version Negotiation**
   - Official spec requires `A2A-Version` HTTP header
   - **Status**: Not clearly implemented in HTTP transport
   - **Recommendation**: Add version negotiation in HTTP server/client

3. **REST/HTTP+JSON Binding**
   - Official spec defines HTTP+JSON as alternative to JSON-RPC
   - **Status**: Not implemented
   - **Impact**: Reduced interoperability with REST-based agents

4. **Error Response Format**
   - Official spec has specific guidance on error responses
   - **Status**: Need to verify JSON-RPC error format compliance
   - **Recommendation**: Add integration tests with official spec examples

### What's Better in This Implementation

1. **Storage Abstraction**: Excellent hexagonal architecture with multiple backends
2. **Authentication Framework**: More comprehensive than minimal spec requirements
3. **Builder Patterns**: Ergonomic Rust API with bon builders
4. **Feature Flags**: Modular compilation for different use cases
5. **WebSocket Support**: Full bidirectional streaming
6. **Examples**: Comprehensive examples including reimbursement agent

---

## Testing Recommendations

### Required Test Coverage

1. **Spec Compliance Tests**
   - Add official spec test vectors
   - Verify JSON serialization matches spec exactly
   - Test all error code scenarios

2. **Integration Tests**
   - Test against official reference implementations
   - Cross-language compatibility tests
   - WebSocket reconnection scenarios

3. **Performance Tests**
   - Benchmark storage backends
   - Streaming throughput tests
   - Concurrent task handling

---

## Migration Path for Fixes

### Phase 1: Critical Fixes (1-2 weeks)
1. Fix context ID hardcoding issue
2. Implement message/send and message/stream OR deprecate them
3. Add version negotiation HTTP headers

### Phase 2: Medium Priority (2-4 weeks)
4. Implement mTLS or remove from supported schemes
5. Add signature validation
6. Complete security scheme integration
7. Document gRPC limitation or implement it

### Phase 3: Enhancement (4-8 weeks)
8. Add REST/HTTP+JSON transport binding
9. Implement AP2 extension
10. Add proto file generation
11. Performance optimizations

---

## Conclusion

The `a2a-rs` implementation is a **high-quality, production-ready A2A v0.3.0 implementation** with excellent architecture and comprehensive feature coverage. The main gaps are:

1. **Context ID management** needs fixing (critical)
2. **New message API** needs implementation or deprecation (critical)
3. **gRPC transport** is advertised but missing (medium)
4. **mTLS authentication** is defined but not implemented (medium)

These issues are addressable and don't fundamentally undermine the implementation's quality. With the recommended fixes, this would be a reference-quality A2A implementation.

### Overall Grade: B+ (85/100)

**Strengths**: Architecture, feature coverage, code quality, documentation
**Weaknesses**: Context management, incomplete new APIs, missing transports

---

## References

- **Official Specification**: https://a2a-protocol.org/latest/specification/
- **GitHub Repository**: https://github.com/a2aproject/A2A
- **Google Announcement**: https://developers.googleblog.com/en/a2a-a-new-era-of-agent-interoperability/
- **Microsoft Blog**: https://www.microsoft.com/en-us/microsoft-cloud/blog/2025/05/07/empowering-multi-agent-apps-with-the-open-agent2agent-a2a-protocol/
- **InfoQ Article**: https://www.infoq.com/news/2025/04/google-agentic-a2a/

---

**Report Generated**: 2025-11-23
**Reviewer**: Claude (Anthropic AI)
**Implementation Version**: Based on commit b2d8dbf
