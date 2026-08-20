# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.6.2](https://github.com/EmilLindfors/a2a-rs/compare/a2acli-v0.6.1...a2acli-v0.6.2) - 2026-08-20

### Fixed

- Accept SendMessage with no task id ([#51](https://github.com/EmilLindfors/a2a-rs/pull/51))

## [0.6.1](https://github.com/EmilLindfors/a2a-rs/compare/a2acli-v0.6.0...a2acli-v0.6.1) - 2026-08-18

### Changed

- Move a2acli into a2a-cli/, publish a2a-llm 0.1.0

## [0.6.0](https://github.com/EmilLindfors/a2a-rs/compare/a2acli-v0.5.0...a2acli-v0.6.0) - 2026-08-15

### Added

- *(a2a-rs)* Carry the authenticated caller to the message handler
- *(a2acli)* List, stdin input, honest exit codes, and a next step
- *(a2acli)* Wait on the event stream, and test the binary end to end

### Fixed

- *(a2a-rs)* Redact the token in Debug, and stop a valid URL panicking

## [0.5.0](https://github.com/EmilLindfors/a2a-rs/compare/a2acli-v0.4.0...a2acli-v0.5.0) - 2026-07-31

### Added

- *(a2a-agents)* Close out the pre-release CLI audit

### Fixed

- *(a2a-rs)* Honour `return_immediately` on `SendMessage`

### Build

- Raise the minimum supported Rust version to 1.96

## [0.4.0](https://github.com/EmilLindfors/a2a-rs/releases/tag/a2acli-v0.4.0) - 2026-06-29

### Added

- *(a2acli)* Add A2A command-line client + promote auto_connect into a2a-rs

### Documentation

- *(changelog)* Note a2acli, auto_connect, and the web-client delegation

### Added

- *(a2acli)* New command-line client driving the `a2a-rs` `Transport` port: `card`, `send`, `get`, `cancel`, `stream`. Endpoint from `A2A_URL` (`--url`/`-u` override); `--transport auto|connectrpc|jsonrpc`; `--json` for machine-readable output. Auto mode negotiates the transport from the agent card with a direct-client fallback. Doubles as a manual cross-SDK interop harness.
