# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.2](https://github.com/EmilLindfors/a2a-rs/compare/a2a-web-client-v0.7.1...a2a-web-client-v0.7.2) - 2026-09-03

### Other

- No code changes; republished to correct a release-plz changelog entry

## [0.7.1](https://github.com/EmilLindfors/a2a-rs/compare/a2a-web-client-v0.7.0...a2a-web-client-v0.7.1) - 2026-09-03

### Other

- Updated the following local packages: a2a-rs (0.8 -> 0.9)

## [0.7.0](https://github.com/EmilLindfors/a2a-rs/compare/a2a-web-client-v0.6.1...a2a-web-client-v0.7.0) - 2026-08-25

### Fixed

- *(a2a-web-client)* Gate axum-components and move to axum 0.8
- Accept SendMessage with no task id ([#51](https://github.com/EmilLindfors/a2a-rs/pull/51))

## [0.6.1](https://github.com/EmilLindfors/a2a-rs/compare/a2a-web-client-v0.6.0...a2a-web-client-v0.6.1) - 2026-08-18

### Changed

- Split the workspace along protocol vs platform

### Documentation

- Repoint the last a2a-agents references at korps

## [0.6.0](https://github.com/EmilLindfors/a2a-rs/compare/a2a-web-client-v0.5.0...a2a-web-client-v0.6.0) - 2026-08-15

### Added

- *(a2a-rs)* Carry the authenticated caller to the message handler

### Fixed

- *(a2a-rs)* Redact the token in Debug, and stop a valid URL panicking

## [0.5.0](https://github.com/EmilLindfors/a2a-rs/compare/a2a-web-client-v0.4.1...a2a-web-client-v0.5.0) - 2026-07-31

### Added

- *(a2a-agents)* Close out the pre-release CLI audit

### Fixed

- *(a2a-rs)* Honour `return_immediately` on `SendMessage`

### Build

- Raise the minimum supported Rust version to 1.96

## [0.4.1](https://github.com/EmilLindfors/a2a-rs/compare/a2a-web-client-v0.4.0...a2a-web-client-v0.4.1) - 2026-06-29

### Added

- *(a2acli)* Add A2A command-line client + promote auto_connect into a2a-rs

### Documentation

- *(changelog)* Note a2acli, auto_connect, and the web-client delegation

### Fixed

- *(client)* Render task-status/artifacts and stream tokens in sse example

### Changed

- *(a2a-web-client)* `WebA2AClient::auto_connect` now delegates to `a2a_rs::auto_connect` (shared entry point); a malformed URL surfaces as `A2AError::InvalidParams`. Dropped the now-unused `reqwest` dependency.

## [0.4.0](https://github.com/EmilLindfors/a2a-rs/compare/a2a-web-client-v0.3.0...a2a-web-client-v0.4.0) - 2026-06-05

### Added

- *(a2a-agents)* MCP server over Streamable HTTP transport

### Documentation

- Doc-comment audit, add ROADMAP, retire stale planning docs

### Feat

- *(a2a-rs)* Client Transport port + JSON-RPC 2.0 client + card negotiation

### Refactor

- *(a2a-rs)* Split streaming & push out of storage adapters (Phase 4 final)

## [0.3.0](https://github.com/EmilLindfors/a2a-rs/compare/a2a-web-client-v0.2.0...a2a-web-client-v0.3.0) - 2026-05-27

### Other

- fmt,clippy
- Fix clippy warnings and failing tests
- migrate to Connect-Rust, refactor project structure, update protobuf specs, and clean up temporary scripts
- docs
