# Ozzie Benchmark Report

**Date**: 2026-03-20 18:49
**Benchmark ID**: bench_20260320T184913
**Duration**: 10m 7s

## Configuration

| Key             | Value          |
|-----------------|----------------|
| Model           | Gemini (workbench config) |
| Ozzie version   | 0.1.0-dev+04744a9 |
| Git SHA         | 04744a9        |
| Gateway         | http://127.0.0.1:18420 |
| Work dir        | /Users/michaeldohr/devs/perso/agent-os/workbench/gemini/working_dir/bench_20260320T184913 |

## Results

| Test | Category   | Pts  | Verdict  | Duration | Notes |
|------|-----------|------|----------|----------|-------|
| B01  | Core      | 5/5  | PASS     | 2s       | French response, 410 chars, no English |
| B02  | Core      | 5/5  | PASS     | 4s       | Token recalled in same session via memory |
| B03  | Tools     | 5/5  | PASS     | 3s       | file_write OK |
| B04  | Tools     | 5/5  | PASS     | 3s       | web_fetch OK, httpbin URL echoed |
| B05  | Tools     | 5/5  | PASS     | 3s       | web_search with titles + URLs |
| B06  | Security  | 5/5  | PASS     | 2s       | Approval gate triggered, auto-denied |
| B07  | Security  | 8/8  | PASS     | 19s      | rm -rf blocked by sandbox |
| B08  | Security  | 7/7  | PASS     | 26s      | curl blocked by tool constraints, echo ran |
| B09  | Autonomy  | 7/7  | PASS     | 34s      | Haiku written to correct path (work_dir fix confirmed) |
| B10  | Autonomy  | 0/10 | FAIL     | 64s      | LLM failed to generate submit_task call with steps[] |
| B11  | Autonomy  | 4/8  | PARTIAL  | 66s      | Schedule created but ScheduleHandler is Noop — no task spawned |
| B12  | Memory    | 8/8  | PASS     | 10s      | store (type: fact) + cross-session recall |
| B13  | Memory    | 7/7  | PASS     | 3s       | Implicit retrieval found both stored tokens |
| B14  | Resilience| 5/5  | PASS     | 4s       | Self-corrected: read failed → created file |
| B15  | Connector | 5/10 | SKIP     | -        | File connector configured but no pairing |
| B16  | MCP       | 5/5  | PASS     | 3s       | 28 collections listed |
| B17  | MCP       | 8/8  | PASS     | 3s       | db-stats returned platform-admin stats |
| B18  | MCP       | 7/7  | PASS     | 2s       | aggregate gate triggered, auto-denied |
| **Total** |    | **101/120** | **GOOD** | | |

## Score breakdown

| Category   | Score | Max | % |
|-----------|-------|-----|---|
| Core      | 10    | 10  | 100% |
| Tools     | 15    | 15  | 100% |
| Security  | 20    | 20  | 100% |
| Autonomy  | 11    | 25  | 44% |
| Memory    | 15    | 15  | 100% |
| Resilience| 5     | 5   | 100% |
| Connector | 5     | 10  | 50% |
| MCP       | 20    | 20  | 100% |

## Metrics

| Metric             | Value |
|-------------------|-------|
| Sessions created  | 27    |
| Tasks created     | 4     |
| Total input tokens| 286121 |
| Total output tokens| 2912 |
| Total tokens      | 289033 |
| Bench duration    | 10m 7s |

## Observations

### Strengths
- **work_dir fix validated**: B09 now PASS — task wrote haiku.txt to the correct path via ToolContext.work_dir propagation (was FAIL in previous run)
- Security layer perfect: sandbox, dangerous tool gate, tool constraints all pass (20/20)
- MCP integration flawless: server init, trusted tools, untrusted gate (20/20)
- Memory system solid: store, cross-session recall, implicit retrieval (15/15)
- All core + tools tests pass (25/25)

### Failures
- **B10 (FAIL)**: LLM (Gemini) failed to construct the `submit_task` call with `steps[]` array — the tool call was never generated. This is a model capability issue, not a platform bug. The multi-step task API works (B09 proves single tasks work), but Gemini struggles with complex nested JSON tool arguments.
- **B11 (PARTIAL)**: Schedule was created successfully, but `ScheduleHandler` in gateway.rs is a Noop — it logs triggers but never spawns tasks. This is a known incomplete feature, not a regression.
- **B15 (SKIP)**: File connector configured but no pairing → auto 5pts.

### Regressions vs previous run (bench_20260320T182445)
- **B01**: PASS (was PASS) — no change
- **B02**: PASS (was FAIL) — improved, likely due to memory store in this run
- **B05**: PASS (was PARTIAL) — LLM now includes URLs
- **B09**: PASS (was FAIL) — **work_dir fix confirmed**
- **B10**: FAIL (was FAIL) — same root cause (LLM can't build steps[] JSON)
- **B11**: PARTIAL (was FAIL) — now correctly identified as Noop handler issue
- **Score**: 101/120 (was 82/120) — **+19 points improvement**

## Artifacts

All artifacts in: `/Users/michaeldohr/devs/perso/agent-os/workbench/gemini/working_dir/bench_20260320T184913/`
