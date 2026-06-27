# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.1](https://github.com/EmilLindfors/a2a-rs/compare/a2a-web-client-v0.4.0...a2a-web-client-v0.4.1) - 2026-06-27

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
