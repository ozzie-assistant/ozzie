# worm-profile

User profile management for AI agents. Store identity facts, track observations from conversations, and consolidate with LLM-driven synthesis.

Part of the [worm](https://github.com/ozzie-assistant/ozzie) family -- named after wormholes from Peter F. Hamilton's *Commonwealth Saga*.

## Features

- `UserProfile` with name, tone, language, and timestamped whoami entries
- `WhoamiEntry` with provenance tracking (`Intro`, `Conversation`, `Consolidated`)
- Protected intro entries that survive consolidation
- Automatic deduplication on `add_observation()`
- JSON file persistence with `load()` / `save()`
- `ProfileSynthesizer` trait for pluggable LLM-driven consolidation
- Zero infrastructure dependencies (serde + chrono only)

## Installation

```bash
cargo add worm-profile
```

## Usage

### Create and manage a profile

```rust
use worm_profile::{UserProfile, load, save};
use std::path::Path;

// Create a new profile with intro facts
let mut profile = UserProfile::new(
    "Alice".into(),
    vec!["Senior Rust developer".into(), "Prefers concise answers".into()],
);

// Add observations from conversation
profile.add_observation("Works on distributed systems".into());
profile.add_observation("Senior Rust developer".into()); // skipped (duplicate)

assert_eq!(profile.whoami.len(), 3); // 2 intro + 1 conversation

// Protected intro entries vs compressible conversation entries
assert_eq!(profile.intro_entries().len(), 2);
assert_eq!(profile.compressible_entries().len(), 1);

// Persist to disk
save(Path::new("./data"), &profile).expect("save");

// Load back
let loaded = load(Path::new("./data")).expect("load").unwrap();
assert_eq!(loaded.name, "Alice");
```

### LLM-driven consolidation

Implement `ProfileSynthesizer` to merge growing whoami lists:

```rust
use worm_profile::{ProfileSynthesizer, WhoamiEntry};

struct MyLlmSynthesizer;

#[async_trait::async_trait]
impl ProfileSynthesizer for MyLlmSynthesizer {
    async fn consolidate(&self, entries: &[WhoamiEntry]) -> Result<Vec<WhoamiEntry>, String> {
        // Call your LLM to deduplicate and merge entries.
        // Return consolidated entries with WhoamiSource::Consolidated.
        todo!()
    }
}
```

## License

MIT
