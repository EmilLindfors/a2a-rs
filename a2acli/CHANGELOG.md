# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- *(a2acli)* New command-line client driving the `a2a-rs` `Transport` port: `card`, `send`, `get`, `cancel`, `stream`. Endpoint from `A2A_URL` (`--url`/`-u` override); `--transport auto|connectrpc|jsonrpc`; `--json` for machine-readable output. Auto mode negotiates the transport from the agent card with a direct-client fallback. Doubles as a manual cross-SDK interop harness.
