# A2A Agents Common

Common utilities for building A2A Protocol agents. This crate provides reusable components and patterns that make agent development faster and easier.

## Features

- **NLP Utilities** - Intent classification and entity extraction
- **Formatting** - Markdown and table formatting helpers
- **Caching** - Async caching for performance optimization (requires `async` feature)
- **Testing** - Test fixtures and helpers for agent testing

## Installation

```toml
[dependencies]
a2a-agents-common = "0.1"
```

For async features (caching):
```toml
[dependencies]
a2a-agents-common = { version = "0.1", features = ["async"] }
```

## Quick Start

### Intent Classification

```rust
use a2a_agents_common::nlp::IntentClassifier;

let classifier = IntentClassifier::new()
    .add_intent("get_quote", &["price", "quote", "what's"])
    .add_intent("analyze", &["analyze", "analysis", "should i buy"]);

let intent = classifier.classify("What's the price of AAPL?");
assert_eq!(intent, Some("get_quote"));
```

### Entity Extraction

```rust
use a2a_agents_common::nlp::EntityExtractor;

let extractor = EntityExtractor::new()
    .with_pattern("stock_symbol", r"\b[A-Z]{2,5}\b");

let entities = extractor.extract("Check AAPL and MSFT prices");
let symbols = entities.get("stock_symbol").unwrap();
// symbols contains ["AAPL", "MSFT"]
```

### Markdown Formatting

```rust
use a2a_agents_common::formatting::MarkdownFormatter;

let response = MarkdownFormatter::new()
    .heading(1, "Stock Analysis")
    .paragraph("Current market data:")
    .bullet_list(&["AAPL: $178.42 (+1.22%)", "TSLA: $242.84 (+2.15%)"])
    .build();
```

### Table Formatting

```rust
use a2a_agents_common::formatting::{TableFormatter, Alignment};

let table = TableFormatter::new()
    .header(&["Symbol", "Price", "Change"])
    .align(1, Alignment::Right)
    .align(2, Alignment::Right)
    .row(&["AAPL", "$178.42", "+1.22%"])
    .row(&["TSLA", "$242.84", "+2.15%"])
    .build();
```

### Async Caching (requires `async` feature)

```rust
use a2a_agents_common::caching::AgentCache;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let cache = AgentCache::<String, String>::new()
        .with_max_capacity(1000)
        .with_ttl(Duration::from_secs(300));

    // Cache API responses
    cache.insert("quote:AAPL", quote_data).await;

    // Get or compute
    let quote = cache.get_or_insert_with("quote:AAPL", async {
        fetch_quote("AAPL").await
    }).await;
}
```

### Testing Utilities

```rust
use a2a_agents_common::testing::*;

#[test]
fn test_my_handler() {
    let message = test_message("Hello, agent!");
    let task = test_task("task-123", "working");
    let quote = sample_stock_quote("AAPL", 178.42, 1.22);

    // Use these in your tests
}
```

## Module Documentation

### `nlp` - Natural Language Processing

- **`IntentClassifier`** - Keyword-based intent classification
  - Simple pattern matching for routing queries to skills
  - Returns best matching intent or all matches with scores

- **`EntityExtractor`** - Extract structured data from text
  - Built-in patterns: emails, URLs, numbers, currency, dates
  - Support for custom regex patterns
  - Returns all entities grouped by type

### `formatting` - Output Formatting

- **`MarkdownFormatter`** - Build markdown responses
  - Headings, paragraphs, lists, code blocks
  - Links, images, blockquotes
  - Fluent builder API

- **`TableFormatter`** - Create markdown tables
  - Column alignment (left, center, right)
  - Automatic width calculation
  - Clean markdown output

### `caching` - Performance Optimization

- **`AgentCache`** - Async caching with TTL
  - Built on moka for high performance
  - Configurable capacity and TTL
  - `get_or_insert_with` for lazy computation
  - Requires `async` feature

### `testing` - Test Utilities

- **Fixtures** - Pre-built test data
  - `test_message()` - Create test messages
  - `test_task()` - Create test tasks
  - `sample_stock_quote()` - Sample financial data
  - `sample_expense()` - Sample expense data

## Examples

See the [examples](examples/) directory for complete examples:

- `intent_classifier.rs` - Intent classification patterns
- `entity_extraction.rs` - Extracting structured data
- `markdown_responses.rs` - Building formatted responses
- `caching_demo.rs` - Using async caching

## Feature Flags

- `async` - Enable async utilities (caching)
- `full` - Enable all features

## License

MIT

## Contributing

Contributions welcome! This crate is designed to be a collection of proven patterns from real agent implementations.
