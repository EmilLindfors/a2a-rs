# Changelog

All notable changes to this project will be documented in this file.
## [0.5.1](https://github.com/EmilLindfors/a2a-rs/compare/a2a-llm-v0.5.0...a2a-llm-v0.5.1) - 2026-09-03

### Added

- A provider says whether the model refused its reasoning parameter

## [0.5.0](https://github.com/EmilLindfors/a2a-rs/compare/a2a-llm-v0.4.0...a2a-llm-v0.5.0) - 2026-09-03

### Fixed

- A machine with no CA bundle is an error, not a panic
- *(a2a-llm)* One Finish per stream, and a cut Gemini candidate keeps its reason

## [0.4.0](https://github.com/EmilLindfors/a2a-rs/compare/a2a-llm-v0.3.0...a2a-llm-v0.4.0) - 2026-09-01

### Added

- *(a2a-llm)* The response says why the model stopped

## [0.3.0](https://github.com/EmilLindfors/a2a-rs/compare/a2a-llm-v0.2.1...a2a-llm-v0.3.0) - 2026-08-31

### Added

- *(a2a-llm)* A context refusal carries the provider's window and token count ([#63](https://github.com/EmilLindfors/a2a-rs/pull/63))

## [0.2.1](https://github.com/EmilLindfors/a2a-rs/compare/a2a-llm-v0.2.0...a2a-llm-v0.2.1) - 2026-08-27

### Fixed

- Recognize llama.cpp's context-overflow error as one

## [0.2.0](https://github.com/EmilLindfors/a2a-rs/compare/a2a-llm-v0.1.1...a2a-llm-v0.2.0) - 2026-08-25

### Added

- *(a2a-llm)* Require a Gemini model rather than defaulting to an unlisted one
- Send `reasoning` to OpenAI and Gemini, and recover when a model refuses it

## [0.1.1](https://github.com/EmilLindfors/a2a-rs/compare/a2a-llm-v0.1.0...a2a-llm-v0.1.1) - 2026-08-18

### Documentation

- *(a2a-llm)* Give the crate a README
- The README described a workspace that no longer exists
- Repoint the last a2a-agents references at korps
