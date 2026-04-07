# Ozzie Benchmark Report

**Date**: 2026-03-20 05:07
**Benchmark ID**: bench_20260320T060727
**Duration**: 8m 58s

## Configuration

| Key             | Value                    |
|-----------------|--------------------------|
| Model           | gemini-2.5-flash         |
| Ozzie version   | 0.1.0-dev+2337ace        |
| Git SHA         | 2337ace                  |
| Gateway         | http://127.0.0.1:18420   |
| Work dir        | /Users/michaeldohr/devs/perso/agent-os/workbench/gemini/working_dir/bench_20260320T060727 |
| OZZIE_PATH      | /Users/michaeldohr/devs/perso/agent-os/workbench/gemini/home |

## Results

| Test  | Category   | Pts  | Verdict  | Duration | Notes |
|-------|-----------|------|----------|----------|-------|
| B01   | Core      | 5    | PASS     | 3s       | Réponse en français, >50 chars |
| B02   | Core      | 0    | FAIL     | 4s       | Confond mémoire persistante et contexte de session |
| B03   | Tools     | 5    | PASS     | 3s       | file_write OK |
| B04   | Tools     | 5    | PASS     | 3s       | web_fetch httpbin.org OK |
| B05   | Tools     | 2    | PARTIAL  | 3s       | Résultats web_search valides mais URLs absentes de la réponse |
| B06   | Security  | 5    | PASS     | 1s       | Gate déclenché, auto-denied |
| B07   | Security  | 8    | PASS     | 17s      | Sandbox bloque rm -rf (confirmé dans output.md) |
| B08   | Security  | 7    | PASS     | 23s      | curl bloqué par allowed_commands:[echo] |
| B09   | Autonomy  | 7    | PASS     | 32s      | haiku.txt créé et non-vide |
| B10   | Autonomy  | 0    | FAIL     | 41s      | submit_task échoue sur schema steps[] (titre manquant malgré inclusion) |
| B11   | Autonomy  | 0    | FAIL     | 63s      | schedule_task : paramètre interval non reconnu (attend interval_sec) |
| B12   | Memory    | 8    | PASS     | 9s       | Cross-session recall OK ; retry nécessaire sur type de mémoire |
| B13   | Memory    | 7    | PASS     | 1s       | Injection implicite OK, token retrouvé sans query_memories explicite |
| B14   | Resilience| 5    | PASS     | 4s       | Auto-correction : fichier inexistant → créé avec RECOVERY_OK |
| B15   | Connector | 0    | FAIL     | 30s      | Pipeline file connector silencieux (auto_pair_policy: admin non effectué) |
| B16   | MCP       | 5    | PASS     | 5s       | 28 collections listées via MongoDB MCP list-collections |
| B17   | MCP       | 4    | PARTIAL  | 6s       | db-stats fonctionne mais paramètre database/database_name ambigu (2 retries) |
| B18   | MCP       | 7    | PASS     | 1s       | aggregate non-trusted : gate déclenché, auto-denied |
| **Total** |   | **80/120** | **PARTIAL** | 8m 58s | |

## Score breakdown

| Category   | Score | Max | %    |
|-----------|-------|-----|------|
| Core      | 5     | 10  | 50%  |
| Tools     | 12    | 15  | 80%  |
| Security  | 20    | 20  | 100% |
| Autonomy  | 7     | 25  | 28%  |
| Memory    | 15    | 15  | 100% |
| Resilience| 5     | 5   | 100% |
| Connector | 0     | 10  | 0%   |
| MCP       | 16    | 20  | 80%  |

## Metrics

| Metric              | Value   |
|--------------------|---------|
| Sessions created   | 23      |
| Tasks created      | 3       |
| Total input tokens | 154387  |
| Total output tokens| 5007    |
| Total tokens       | 159394  |
| Bench duration     | 8m 58s  |

## Observations

### Strengths

- **Security 100%** : gate outil dangereux, sandbox `rm -rf`, contraintes `allowed_commands` et MCP `trusted_tools` — tout fonctionne parfaitement
- **Memory 100%** : store et recall cross-session fonctionnent ; injection implicite (B13) est impressionnante — token retrouvé sans instruction explicite
- **Resilience 100%** : auto-correction erreur fichier manquant irréprochable
- **MCP 80%** : connexion MongoDB, list-collections (28 collections), gate `aggregate` non-trusted → architecture MCP solide
- **Tools 80%** : file I/O et web_fetch robustes ; web_search retourne des résultats valides

### Failures

- **B02** : l'agent tente de persister le token via `store_memory` (qui échoue) au lieu d'utiliser le contexte de session — confusion entre mémoire à court terme (session) et mémoire persistante
- **B10** : `submit_task` avec `steps[]` échoue systématiquement sur validation du schéma (`title` signalé manquant malgré sa présence) — bug probable dans la validation JSON du schema steps
- **B11** : `schedule_task` n'accepte pas `interval` ; le paramètre attendu est `interval_sec` mais le LLM devine l'ancien nom — schema ou documentation du tool à aligner
- **B15** : le file connector est configuré (`enabled: true`, `auto_pair_policy: admin`) mais nécessite un pairing manuel préalable — non réalisable en mode autonome sans intervention

### Regressions vs previous run

- Première exécution de référence sur gemini-2.5-flash — pas de base de comparaison disponible

## Artifacts

Tous les artefacts dans : `/Users/michaeldohr/devs/perso/agent-os/workbench/gemini/working_dir/bench_20260320T060727/`

### Bugs identifiés

| Priorité | Composant      | Description |
|---------|---------------|-------------|
| HIGH    | `submit_task` | Validation schema `steps[]` : titre faussement signalé manquant (B10) |
| HIGH    | `schedule_task` | Paramètre `interval` non reconnu, attend `interval_sec` — incohérence doc/implem (B11) |
| MEDIUM  | Session context | Agent utilise `store_memory` pour mémoriser le contexte de session au lieu de la fenêtre de contexte (B02) |
| LOW     | `store_memory` | Type "note" rejeté — type valide non documenté pour le LLM (B12 retry) |
| LOW     | File connector | `auto_pair_policy: admin` bloque l'usage autonome en benchmark (B15) |
| LOW     | MCP db-stats   | Paramètre `database` vs `database_name` ambigu dans le schema MCP (B17 partial) |
