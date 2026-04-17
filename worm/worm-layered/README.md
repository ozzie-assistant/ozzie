# worm-layered

Layered context compression for LLM agents. Keep long conversations usable without blowing your token budget.

Part of the [worm](https://github.com/ozzie-assistant/ozzie) family -- named after wormholes from Peter F. Hamilton's *Commonwealth Saga*.

## Features

- **L0/L1/L2 progressive compression** -- recent messages stay verbatim, older ones get summarized, oldest become keyword-indexed archives
- **BM25 retrieval** -- Okapi BM25 scoring to pull relevant history from compressed layers
- **Confidence gating** -- retrieval escalates from L0 to L1 to L2 only when confidence is below threshold
- **Recency prior** -- recent archive nodes get a scoring bonus
- **Budget-aware selection** -- respects a token budget (45% of prompt by default)
- **Pluggable summarizer** -- bring your own LLM via `SummarizerFn`
- **Pluggable storage** -- implement `ArchiveStore` for your persistence backend
- **Token utilities** -- `estimate_tokens()`, `chunk_messages()`, `extract_keywords()`

## Installation

```bash
cargo add worm-layered
```

## Usage

### BM25 standalone

```rust
use worm_layered::BM25;

let documents = ["How to configure Nginx", "SSL certificate setup", "Docker networking"];
let bm25 = BM25::build(&documents);
let score = bm25.score("nginx SSL", 0); // score for first document
```

### Full compression pipeline

```rust
use worm_layered::{Manager, Config, Message, ArchiveStore};

let config = Config {
    max_recent_messages: 24,
    archive_chunk_size: 8,
    score_threshold_high: 0.64,
    ..Config::default()
};

// Provide a summarizer function and an archive store
let summarizer = Box::new(worm_layered::fallback_summarizer); // heuristic, no LLM
let store: Box<dyn ArchiveStore> = /* your store */;
let manager = Manager::new(store, config, summarizer);

// Apply compression to a message history
let messages: Vec<Message> = /* conversation history */;
let (compressed, stats) = manager.apply("session_123", &messages)?;
// compressed: recent messages + injected context from archives
```

### Implement ArchiveStore

```rust
use worm_layered::{ArchiveStore, ArchivePayload, Index, StoreError};

struct MyStore;

#[async_trait::async_trait]
impl ArchiveStore for MyStore {
    fn save_index(&self, index: &Index) -> Result<(), StoreError> { todo!() }
    fn load_index(&self, session_id: &str) -> Result<Option<Index>, StoreError> { todo!() }
    fn save_archive(&self, session_id: &str, node_id: &str, payload: &ArchivePayload) -> Result<(), StoreError> { todo!() }
    fn load_archive(&self, session_id: &str, node_id: &str) -> Result<Option<ArchivePayload>, StoreError> { todo!() }
}
```

## How it works

```
[Recent messages] --keep--> verbatim (last N messages)
[Older messages]  --index-> L0 abstracts (120 tokens each)
                            L1 summaries (1200 tokens each)
                            L2 full transcripts (on demand)
                               |
                 BM25 query → score L0 → escalate if low confidence → L1 → L2
                               |
                         inject context message before recent messages
```

## License

MIT
