# A2A-RS TODO

## Overview

This document tracks the outstanding work items for the a2a-rs project based on open pull requests and issues.

**Current Status:** The a2a-rs implementation is 100% compliant with v0.2.x but missing several v0.3.0 features.

**Overall v0.3.0 Compliance:** 40% (4/10 new features implemented)

---

## Open Pull Requests

### PR #8: Add support for default A2A agent card location and message/send
**Author:** kakilangit
**Status:** Open
**Priority:** High
**Description:** This PR introduces support for accept default location for A2A agent card and implements the "message/send" protocol for agent communication.

**Action Items:**
- [ ] Review and test the PR
- [ ] Verify compatibility with v0.3.0 spec
- [ ] Merge or provide feedback

---

## Issue #10: Implement A2A Protocol v0.3.0 Specification Features

**Author:** EmilLindfors
**Status:** Open
**Priority:** High

### Priority 1: Security Features (High Impact)

#### 1. Add AgentCard Signature Support
**Impact:** High - Essential for production security and card integrity verification

**Tasks:**
- [ ] Add `signature` field to `AgentCard` struct
- [ ] Implement `AgentCardSignature` type per RFC 7515 (JSON Web Signature)
- [ ] Add JWS verification logic
- [ ] Implement signature verification method (optional feature flag)
- [ ] Add tests for signature validation
- [ ] Update documentation

**Files to modify:**
- `a2a-rs/src/domain/core/agent.rs` (line ~219)

---

#### 2. Add mTLS Security Scheme
**Impact:** High - Enterprise requirement for zero-trust architectures

**Tasks:**
- [ ] Add `MutualTls` variant to `SecurityScheme` enum
- [ ] Support mTLS in authentication flows
- [ ] Add serialization/deserialization tests for mTLS scheme
- [ ] Add documentation on mTLS configuration
- [ ] Create example agent using mTLS authentication

**Files to modify:**
- `a2a-rs/src/domain/core/agent.rs` (line ~23)

---

### Priority 2: Compliance Features (Medium Impact)

#### 3. Add OAuth2 Metadata URL Field
**Impact:** Medium - OAuth2 discovery improvement

**Tasks:**
- [ ] Add optional `metadataUrl` field to `OAuth2` security scheme
- [ ] Support RFC 8414 OAuth2 Authorization Server Metadata
- [ ] Add tests for OAuth2 scheme with metadata URL
- [ ] Update documentation with RFC 8414 reference

**Files to modify:**
- `a2a-rs/src/domain/core/agent.rs` (line ~40)

---

#### 4. Add Per-Skill Security Requirements
**Impact:** Medium - Allow skills to specify their own auth requirements

**Tasks:**
- [ ] Add `security` field to `AgentSkill` struct
- [ ] Implement builder method `with_security()`
- [ ] Add tests for skill-level security requirements
- [ ] Create example showing skill with specific auth requirements
- [ ] Add documentation explaining security inheritance

**Files to modify:**
- `a2a-rs/src/domain/core/agent.rs` (line ~150)

---

#### 5. Update Well-Known URI Endpoint
**Impact:** Low-Medium - Proper discovery convention per IANA standards

**Current:** `/agent-card` endpoint exists
**Spec requires:** `/.well-known/agent-card.json` (changed from `agent.json`)

**Tasks:**
- [ ] Add `/.well-known/agent-card.json` route
- [ ] Keep `/agent-card` for backward compatibility
- [ ] Keep `/.well-known/agent.json` for v0.2.x compatibility
- [ ] Add tests for all three endpoints
- [ ] Update documentation with correct discovery endpoint
- [ ] Update examples to use new well-known URI

**Files to modify:**
- `a2a-rs/src/adapter/transport/http/server.rs` (line ~90)

---

### Priority 3: Advanced Features (Low Impact)

#### 6. Implement agent/getExtendedCard Method
**Impact:** Low - Advanced feature for authenticated clients

**Current state:**
- ✅ `supports_authenticated_extended_card` field exists in `AgentCard`
- ❌ No `agent/getExtendedCard` RPC method implemented

**Tasks:**
- [ ] Define `agent/getExtendedCard` JSON-RPC method
- [ ] Implement request/response types
- [ ] Create handler with authentication validation
- [ ] Ensure extended card returned only to authenticated clients
- [ ] Add tests for authenticated and unauthenticated access
- [ ] Add documentation and examples

**Files to create/modify:**
- `a2a-rs/src/application/handlers/agent.rs` (NEW FILE)
- `a2a-rs/src/application/json_rpc.rs`

---

## Issue #7: Support AP2 (Agent Payments Protocol)

**Author:** joshua-mo-143
**Status:** Open
**Priority:** Medium
**External Integration:** Rig framework considering feature-gated integration

**Description:** Add support for Agent Payments Protocol (AP2) to enable payment capabilities for A2A agents.

**Tasks:**
- [ ] Review AP2 specification: https://github.com/google-agentic-commerce/AP2/blob/main/docs/a2a-extension.md
- [ ] Design AP2 integration architecture
- [ ] Implement AP2 extension for A2A
- [ ] Add payment-related types and methods
- [ ] Create tests for AP2 functionality
- [ ] Add documentation and examples
- [ ] Consider feature-gated implementation for optional usage

**Notes:**
- This is early-stage, not urgent
- Potential collaboration with Rig framework maintainer

---

## Additional Tasks

### Documentation
- [ ] Update main README with v0.3.0 compliance status
- [ ] Add v0.3.0 migration guide
- [ ] Update spec/README.md with v0.3.0 references
- [ ] Add examples demonstrating new v0.3.0 features
- [ ] Document AP2 integration once implemented

### Testing
- [ ] Integration tests for all new v0.3.0 features
- [ ] Backward compatibility tests (v0.2.x clients)
- [ ] Test against A2A TCK if available
- [ ] Update test fixtures with v0.3.0 structures
- [ ] Add AP2 test cases

### CI/CD
- [ ] Update CI to test v0.3.0 compliance
- [ ] Add feature flags for optional v0.3.0 features
- [ ] Add feature flag for AP2 support
- [ ] Version bump to indicate v0.3.0 support

---

## Implementation Phases

### Phase 1: Security (v0.4.0)
- [ ] PR #8 - Review and merge
- [ ] Task #1 - AgentCard Signature Support
- [ ] Task #2 - mTLS Security Scheme
- [ ] Tests and documentation for Phase 1

### Phase 2: Compliance (v0.5.0)
- [ ] Task #3 - OAuth2 Metadata URL
- [ ] Task #4 - Per-Skill Security
- [ ] Task #5 - Well-Known URI Update
- [ ] Tests and documentation for Phase 2

### Phase 3: Advanced Features (v0.6.0)
- [ ] Task #6 - agent/getExtendedCard Method
- [ ] Full v0.3.0 compliance verification
- [ ] Update documentation to reflect full compliance

### Phase 4: AP2 Support (v0.7.0)
- [ ] Design and implement AP2 extension
- [ ] Tests and documentation for AP2
- [ ] Feature flag for optional AP2 usage

---

## References

- [A2A v0.3.0 Release Notes](https://github.com/a2aproject/A2A/releases/tag/v0.3.0)
- [A2A Protocol Specification](https://a2a-protocol.org/latest/specification/)
- [A2A GitHub Repository](https://github.com/a2aproject/A2A)
- [RFC 7515 - JSON Web Signature](https://tools.ietf.org/html/rfc7515)
- [RFC 8414 - OAuth 2.0 Authorization Server Metadata](https://tools.ietf.org/html/rfc8414)
- [RFC 8615 - Well-Known URIs](https://tools.ietf.org/html/rfc8615)
- [AP2 Specification](https://github.com/google-agentic-commerce/AP2/blob/main/docs/a2a-extension.md)

---

## Notes

- All v0.3.0 changes are **backward compatible** with v0.2.x
- Fields are added as `Option<T>` to maintain compatibility
- Consider feature flags for optional security and payment features
- v0.3.0 breaking changes are mainly additive (new fields/methods)
- AP2 support should be feature-gated for optional usage
