# Memory Consolidation — The Sleep Analogy

## The human parallel

When humans sleep, the brain doesn't rest — it **consolidates**. The hippocampus replays the day's experiences, and a triage process occurs:

- **Important patterns** get transferred to long-term memory (neocortex)
- **Emotional/identity markers** get reinforced — who you are, what matters to you
- **Noise** gets discarded — the color of the coffee cup, the exact wording of a trivial email
- **Related memories** get linked and compressed — three separate conversations about the same project become one coherent understanding

Without sleep, short-term memory overflows, recall degrades, and the signal-to-noise ratio collapses. The brain doesn't remember more by sleeping less — it remembers *better* by forgetting strategically.

## Ozzie's sleep cycle

Ozzie follows the same principle. After conversations accumulate, a **consolidation job** runs — Ozzie's equivalent of sleep. This is not retrieval, not search, not summarization. It's an active process of **digestion and qualification**.

### The process

```
Raw conversations (short-term)
    │
    ▼
Consolidation job (recurring, principal actor only)
    │
    ├── Extract: what did we learn?
    │
    ├── Qualify: what kind of knowledge is this?
    │   │
    │   ├── Identity    → profile.jsonc  (who is the user)
    │   ├── Contextual  → semantic memory (what are we working on)
    │   └── Noise       → discard        (not worth retaining)
    │
    ├── Compress: are there redundant memories?
    │   │
    │   ├── Profile entries consolidated  (3 facts → 1 concise statement)
    │   └── Memory entries deduplicated   (overlapping context merged)
    │
    └── Mark conversations as "digested"
```

### Two memory systems

Just like humans have different memory systems, Ozzie maintains two:

| | Profile (`profile.jsonc`) | Semantic Memory (`memory/`) |
|---|---|---|
| Human analogy | **Autobiographical self** — who am I, who are my people, what do I value | **Episodic + semantic memory** — what happened, what I know about the world |
| Content | User identity, communication preferences, stable facts | Project context, technical decisions, task history |
| Lifespan | Long — changes slowly, like personality | Variable — decays, gets updated, context-dependent |
| Access | Always loaded (system prompt) | Retrieved on demand (similarity search) |
| Size | Compact (few hundred tokens) | Unbounded (database) |
| Compression | Consolidate non-intro entries when list grows | Decay + merge overlapping entries |

### Why the principal actor only

In humans, memory consolidation happens in a specific brain state — you can't outsource it. Similarly, Ozzie's consolidation runs **only on the principal actor** (the default, trusted LLM provider). Reasons:

1. **Confidentiality** — the user's identity and personal information should not transit through sub-agents or untrusted models
2. **Consistency** — the same "mind" that converses with the user should be the one that decides what to remember
3. **Quality** — classification requires nuance that smaller models may lack

### The "intro" protection

During onboarding ("getting to know each other"), the user provides foundational information. These entries are tagged `source: "intro"` and are **never compressed or discarded** — they're the equivalent of core autobiographical memories that define the relationship. Everything else is fair game for consolidation.

### When does it run?

The consolidation job is triggered by the scheduler/heartbeat system, not by conversation flow. Possible triggers:

- **Time-based**: every 24 hours of gateway uptime
- **Volume-based**: after N new conversations since last consolidation
- **Idle-based**: when the agent has been idle for a period (closest to actual sleep)

The "last consolidated" marker per session prevents reprocessing.

## What this is NOT

- **Not real-time** — consolidation doesn't happen during conversation. That would slow down interaction and create race conditions.
- **Not summarization** — we're not creating summaries of conversations. We're extracting and classifying discrete facts.
- **Not the same as the layered context system** — L0/L1/L2 compression operates within a session to manage context window. Consolidation operates across sessions to build long-term knowledge.
- **Not backup** — the raw conversations remain in session storage. Consolidation creates a *distilled* layer on top.
