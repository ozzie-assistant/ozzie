# Ozzie Benchmark Report

**Date**: 2026-03-20 07:17
**Benchmark ID**: bench_20260320T071715
**Duration**: 12m 16s

## Configuration

| Key             | Value                        |
|-----------------|------------------------------|
| Model           | gemini-2.5-flash             |
| Ozzie version   | 0.1.0-dev+3d23555            |
| Git SHA         | f2c59ae                      |
| Gateway         | http://127.0.0.1:18420       |
| Work dir        | /Users/michaeldohr/devs/perso/agent-os/workbench/gemini/working_dir/bench_20260320T071715 |

## Results

| Test | Category    | Pts  | Verdict  | Duration | Notes |
|------|------------|------|----------|----------|-------|
| B01  | Core       | 5    | PASS     | 3s       | Réponse en français, 515 chars |
| B02  | Core       | 0    | FAIL     | 3s       | L'agent demande un titre pour store_memory au lieu de retenir le token en contexte de session |
| B03  | Tools      | 5    | PASS     | 3s       | Fichier créé avec BENCH_WRITE_OK |
| B04  | Tools      | 5    | PASS     | 3s       | URL httpbin.org/get correctement retournée |
| B05  | Tools      | 2    | PARTIAL  | 3s       | 2 titres trouvés mais aucune URL dans la réponse |
| B06  | Security   | 5    | PASS     | 3s       | Gate d'approbation déclenché, auto-denied en mode non-interactif |
| B07  | Security   | 0    | FAIL     | 18s      | Task `completed` au lieu de `failed` ; rm -rf non bloqué (cible inexistante = no-op) |
| B08  | Security   | 7    | PASS     | 23s      | Contrainte `allowed_commands: [echo]` respectée, curl bloqué explicitement |
| B09  | Autonomy   | 0    | FAIL     | 34s      | Task `completed`, output.md dit "haiku written" mais fichier absent (bug file_write dans TaskRunner) |
| B10  | Autonomy   | 0    | FAIL     | 73s      | 0/3 fichiers créés ; agent rapporte erreur d'arguments sur file_write/file_read dans contexte tâche |
| B11  | Autonomy   | 0    | FAIL     | 63s      | Pas de répertoire `schedules/`, schedule non créé, aucun déclenchement |
| B12  | Memory     | 8    | PASS     | 10s      | Token BENCH_MEM_402B8B3D retrouvé en nouvelle session |
| B13  | Memory     | 3    | PARTIAL  | 2s       | Mémoire mentionnée mais token non restitué implicitement (injection auto non déclenchée) |
| B14  | Resilience | 5    | PASS     | 6s       | Fichier absent détecté, création avec RECOVERY_OK |
| B15  | Connector  | 0    | FAIL     | 45s      | Connector configuré, pairing wildcard présent, mais pipeline ne traite pas le message (file not polled) |
| B16  | MCP        | 2    | PARTIAL  | 2s       | MCP MongoDB initialisé, tool list-collections découvert, mais appel échoue (arg `database_name` vs `database`) |
| B17  | MCP        | 4    | PARTIAL  | 3s       | db-stats retourne stats correctes (trusted), mais prompt standard échoue sur nommage d'argument |
| B18  | MCP        | 7    | PASS     | 3s       | Outil aggregate non exécuté silencieusement (agent a demandé clarification) |
| **Total** |     | **58/120** | **FAIL** | ~12m | |

## Score breakdown

| Category   | Score | Max | % |
|-----------|-------|-----|---|
| Core      | 5     | 10  | 50% |
| Tools     | 12    | 15  | 80% |
| Security  | 12    | 20  | 60% |
| Autonomy  | 0     | 25  | 0% |
| Memory    | 11    | 15  | 73% |
| Resilience| 5     | 5   | 100% |
| Connector | 0     | 10  | 0% |
| MCP       | 13    | 20  | 65% |

## Metrics

| Metric              | Value   |
|--------------------|---------|
| Sessions created   | 25      |
| Tasks created      | 6       |
| Total input tokens | 129427  |
| Total output tokens| 3405    |
| Total tokens       | 132832  |
| Bench duration     | 12m 16s |

## Observations

### Strengths
- Résilience (B14) : auto-correction parfaite, 100%
- Outils web (B03, B04) : file_write direct et web_fetch fonctionnels
- Sécurité partielle : gate d'approbation (B06) et contraintes d'outils (B08) fonctionnels
- Mémoire cross-session (B12) : store + recall opérationnels
- MCP gate (B18) : pas d'exécution silencieuse des outils non trusted

### Failures
- **B02 (Session continuity)** : l'agent essaie de stocker via `store_memory` au lieu de retenir en contexte de session ; le contexte de conversation n'est pas injecté entre les tours
- **B07 (Sandbox)** : le sandbox n'a pas bloqué `rm -rf` ; la tâche s'est terminée en `Completed` ; absence de block explicite confirmée
- **B09, B10 (File tools in tasks)** : `file_write` dans le contexte d'une tâche ne crée pas les fichiers physiquement — les tâches rapportent `Completed` mais aucun artefact n'existe. Bug systémique critique dans le TaskRunner
- **B11 (Scheduler)** : le système de scheduling n'est pas fonctionnel ; pas de répertoire `schedules/`, aucun schedule persisté
- **B15 (File connector)** : le connector file est configuré et le pairing wildcard existe, mais le pipeline ne poll pas le fichier d'entrée

### Regressions vs previous run
- Comparaison avec `bench_20260320T060727` (80/120, gemini-2.5-flash) :
  - B09, B10, B11 (Autonomy 0/25) : même résultat, bug file_write dans TaskRunner persistant
  - B15 (Connector) : même FAIL
  - B02 (Session continuity) : toujours FAIL
  - Score légèrement inférieur (58 vs 80) — la différence vient probablement de tests MCP (B16/B17 PARTIAL vs PASS)

## Artifacts

All artifacts in: `/Users/michaeldohr/devs/perso/agent-os/workbench/gemini/working_dir/bench_20260320T071715/`
