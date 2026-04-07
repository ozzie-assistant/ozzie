# Ozzie — Architecture

Ozzie est un Agent OS personnel. Un seul process persistant (`ozzie gateway`) orchestre tout.
Les clients se connectent via WebSocket ou connecteurs externes.

L'architecture s'articule autour de 3 organes, le cerveau étant lui-même composé de 7 sous-systèmes :

```
         ┌──────────────┐
         │  Eyes         │  Connecteurs (Discord, File, ...)
         └──────┬───────┘  connectors/
                │ events
         ┌──────▼──────────────────────────────────────────────────────────────┐
         │  Brain                                                ozzie-core/  │
         │                                                                     │
         │  ┌────────────────────────────────────────────────────────────────┐ │
         │  │ Nervous System — Event Loop (EventBus)            events/     │ │
         │  │ Transport de tous les signaux                                  │ │
         │  └────────────────────────────────────────────────────────────────┘ │
         │                                                                     │
         │  ┌────────────┐  ┌──────────┐  ┌────────┐  ┌───────────┐          │
         │  │ Identity    │  │ Cognitive│  │ Memory │  │ Skills    │          │
         │  │ Personality │  │ Actor    │  │Semantic│  │ Workflows │          │
         │  │ Self-aware  │  │ Pool     │  │Layered │  │ DAG       │          │
         │  │  prompt/    │  │ actors/  │  │memory/ │  │ skills/   │          │
         │  └────────────┘  └──────────┘  └────────┘  └───────────┘          │
         │                                                                     │
         │  ┌──────────────────┐  ┌──────────────────────┐                    │
         │  │ Conscience        │  │ Introspection         │                   │
         │  │ Approval/Sandbox  │  │ Logging (tracing)     │                   │
         │  │ conscience/       │  │                        │                   │
         │  └──────────────────┘  └──────────────────────┘                    │
         └──────────────────────────────┬──────────────────────────────────────┘
                                        │ tool calls
         ┌──────────────────────────────▼───────┐
         │  Hands                                │  Tools (registry, native, MCP)
         └──────────────────────────────────────┘  ozzie-tools/
```

---

# Eyes — Connecteurs

Comment Ozzie perçoit le monde extérieur et interagit avec les utilisateurs.

## Principe

- **Zero trust** : tout connecteur doit être paired à l'agent via `approve_pairing`
- **Multi-provider** : Discord, File (dev), extensible (Slack, Teams, Web planned)
- **Identity mapping** : chaque utilisateur externe est associé à une policy (admin, support, executor, readonly)
- **Async approval** : les demandes de pairing transitent par l'admin channel

## Flux

```
Utilisateur externe
    │
    ▼
Connector (Discord, TUI, ...)
    │
    ├─ Pairing request → admin approval → policy assigned
    │
    ▼
Event Bus ──► EventRunner ──► Session ──► Réponse
```

## Session mapping

- Chaque conversation externe est mappée à une session Ozzie
- Le mapping est persisté dans `~/.ozzie/connectors/`
- La policy détermine les droits : tools autorisés, skills accessibles, mode d'approbation

## Implémentation

- `ozzie-core/src/connector/` — Identity, IncomingMessage, OutgoingMessage types
- `ozzie-runtime/src/connector/` — `ProcessSupervisor` (spawn, monitor, restart, graceful shutdown)
- `connectors/ozzie-discord-bridge/` — Discord via serenity + OzzieClient
- `connectors/ozzie-file-bridge/` — JSONL file connector (dev/testing)

---

# Brain

Le centre de décision d'Ozzie, composé de 5 sous-systèmes.

## Nervous System — Event Loop

Le système nerveux : tous les signaux transitent par lui.

- Tout est un event : interaction utilisateur, trigger, thinking, streaming, tool call, tool result
- Le `EventBus` (trait, implémentation in-memory `Bus`) est le bus central
- Les composants communiquent exclusivement par events, jamais par appels directs
- L'`EventRunner` orchestre le cycle ReAct : prompt → LLM → tool calls → résultat → boucle
- Event persistence : chaque event est loggé dans `~/.ozzie/logs/` (JSONL)

**Code** : `ozzie-core/src/events/`, `ozzie-runtime/src/event_runner.rs`

## Identity — Personnalité & conscience de soi

Ce qui fait qu'Ozzie sait **qui il est** et **ce dont il dispose** à un instant T.

### Personnalité

- **Persona** : le caractère d'Ozzie — pragmatique, direct, dry wit, "friend in the lab"
- Overridable via `SOUL.md` dans `OZZIE_PATH`

### Conscience de soi

Ce qu'Ozzie sait de lui-même à chaque instant :

- Quels **tools** sont actifs / disponibles à activer
- Quelles **skills** il maîtrise
- Quelle **session** est en cours (working dir, language, titre)
- Quelles **instructions custom** l'utilisateur a configurées

**Code** : `ozzie-core/src/prompt/` (Registry, Composer, section builders)

## Cognitive — ReAct Loop

La partie pensante : raisonnement, prise de décision.

- `ReactLoop::run()` — itère tool calls jusqu'à réponse finale ou max iterations
- `ReactConfig` — provider + tools + instruction + timeout
- Le provider LLM est injecté via le trait `Provider`

**Code** : `ozzie-runtime/src/react.rs`, `ozzie-runtime/src/event_runner.rs`

## LLM / SLM

### Configuration

- **Providers** : Anthropic, OpenAI, Gemini, Mistral, Groq, Ollama, xAI (7 drivers)
- **Auth** : résolution en cascade : config → env var → driver default. Ollama : pas d'auth
- **Resilience** : FallbackProvider avec circuit breaker (3 fails / 60s cooldown), retry exponentiel avec jitter

**Code** : `ozzie-llm/src/providers/`

## Context Compression — Layered Context

Compression hiérarchique de l'historique des conversations longues.

| Layer | Contenu | Budget tokens |
|-------|---------|---------------|
| L0 | Abstract : 1-2 phrases | ~120 |
| L1 | Summary : bullet points structurés | ~1200 |
| L2 | Transcript : conversation complète | illimité |

Escalation progressive L0 → L1 → L2 selon la confiance BM25 + recency.

**Code** : `ozzie-core/src/layered/`

---

# Brain > Memory — Mémoire

Comment Ozzie apprend et se souvient.

## Mémoire sémantique

Stockage long-terme dans SQLite (rusqlite + bundled).

### Modèle de données

Chaque mémoire a :

- **Type** : `preference` | `fact` | `procedure` | `context`
- **Importance** : contrôle le decay — `core` (permanent) | `important` | `normal` | `ephemeral`
- **Confidence** : [0.0 - 1.0], décroît avec le temps selon l'importance
- **Tags** : labels pour la recherche
- **Source** : origine (`agent`, `task:xxx`, `consolidation`)

### Decay temporel

| Importance | Grâce | Taux / semaine | Plancher |
|------------|-------|----------------|----------|
| core | ∞ | 0 | — |
| important | 30j | 0.005 | 0.3 |
| normal | 7j | 0.01 | 0.1 |
| ephemeral | 1j | 0.05 | 0.1 |

### Recherche hybride

```
Score = 0.3 × keyword(FTS5) + 0.7 × semantic(cosine)
```

- **Keyword** : FTS5 full-text search avec prefix matching
- **Semantic** : embeddings vectoriels + brute-force cosine similarity
- Seuil minimum : 0.25

### Implicit retrieval

Les mémoires pertinentes sont injectées automatiquement dans le contexte de chaque requête
par le middleware `FtsMemoryRetriever`. Pas besoin d'appel explicite à `query_memories`.

### Consolidation LLM

- Détecte les mémoires similaires (cosine ≥ 0.85)
- Fusionne via LLM en une entrée consolidée
- Sources marquées `merged_into` → exclues des requêtes

**Code** : `ozzie-memory/src/`

---

# Brain > Skills — Compétences

Les capacités apprises d'Ozzie — workflows structurés et progressive disclosure.

## Principe

- Les skills sont des définitions déclaratives (fichiers `SKILL.md`) avec instructions, tools requis, et optionnellement un workflow DAG
- **Progressive disclosure** : les skills sont listés dans le prompt mais pas chargés. L'agent utilise `activate(name)` pour charger les instructions complètes
- Séparation connaissance (skill body) vs exécution (workflow DAG)

## Structure d'un skill

```
skills/
  my-skill/
    SKILL.md          # instructions + metadata (YAML frontmatter)
```

**Frontmatter** : name, description, tools (required), triggers (cron, on_event), workflow (steps DAG)

**Code** : `ozzie-core/src/skills/`

---

# Hands — Tools

Comment Ozzie agit sur le monde. Deux niveaux de confiance.

## Safe

Exécution directe, pas de demande d'approbation.

| Tool | Catégorie | Description |
|------|-----------|-------------|
| `web_search` | Web | Recherche web (DuckDuckGo) |
| `store_memory` | Memory | Stocke une mémoire long-terme |
| `query_memories` | Memory | Recherche keyword dans les mémoires |
| `forget_memory` | Memory | Supprime une entrée mémoire |
| `schedule_task` | Schedule | Crée une tâche récurrente (cron, interval, event) |
| `unschedule_task` | Schedule | Supprime un schedule |
| `list_schedules` | Schedule | Liste tous les schedules |
| `trigger_schedule` | Schedule | Déclenche manuellement un schedule |
| `run_subtask` | Autonomy | Délègue à une sous-boucle ReAct (profondeur max 3) |
| `activate` / `tool_search` | Control | Découvre et active des tools/skills on-demand |
| `update_session` | Control | Met à jour les metadata de session |
| `yield_control` | Control | Yield coopératif (done / waiting / checkpoint) |
| `file_read` / `file_write` | Filesystem | Lecture/écriture de fichiers |
| `list_dir` / `glob` / `grep` | Filesystem | Exploration du filesystem |
| `str_replace_editor` | Filesystem | Éditeur riche (view, create, str_replace, insert, undo) |

## Dangerous — require user approval

Le `DangerousToolWrapper` intercepte l'appel et prompt l'utilisateur (allow once / always / deny).

| Tool | Catégorie | Description |
|------|-----------|-------------|
| `execute` | Execution | Shell avec sandbox + constraints |
| `git` | Execution | Opérations git |
| `web_fetch` | Web | Fetch une page web |
| MCP tools | External | Unsafe par défaut, configurable `trusted_tools` |

**Code** : `ozzie-tools/src/native/`, `ozzie-core/src/conscience/`
