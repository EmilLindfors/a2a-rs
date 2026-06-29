# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0](https://github.com/EmilLindfors/a2a-rs/compare/a2a-agents-common-v0.3.1...a2a-agents-common-v0.4.0) - 2026-06-29

### Added

- *(agents)* Registry, runtime port, control-plane, and container runtime
- *(llm)* Add OpenRouter provider, centralized selection, and reasoning support

### Fixed

- *(common)* Use current_thread runtime in AgentCache doctest

## [0.3.1](https://github.com/EmilLindfors/a2a-rs/compare/a2a-agents-common-v0.3.0...a2a-agents-common-v0.3.1) - 2026-06-05

### Added

- *(a2a-agents)* MCP server over Streamable HTTP transport

## [0.3.0](https://github.com/EmilLindfors/a2a-rs/compare/a2a-agents-common-v0.2.0...a2a-agents-common-v0.3.0) - 2026-05-27

### Fixed

- fixed CI

### Other

- fmt,clippy
- Fix clippy warnings and failing tests
- migrate to Connect-Rust, refactor project structure, update protobuf specs, and clean up temporary scripts
- docs
