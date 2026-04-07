# Ozzie Benchmark Report

**Date**: 2026-03-20 18:24
**Benchmark ID**: bench_20260320T182445
**Duration**: 10m 30s

## Configuration

| Key             | Value          |
|-----------------|----------------|
| Model           | unknown (Gemini workbench) |
| Ozzie version   | 0.1.0-dev+3d23555 |
| Git SHA         | 08082f7        |
| Gateway         | http://127.0.0.1:18420 |
| Work dir        | /Users/michaeldohr/devs/perso/agent-os/workbench/gemini/working_dir/bench_20260320T182445 |

## Results

| Test | Category   | Pts  | Verdict  | Duration | Notes |
|------|-----------|------|----------|----------|-------|
| B01  | Core      | 5/5  | PASS     | 2s       | French response, >50 chars, no English |
| B02  | Core      | 0/5  | FAIL     | 5s       | Token not recalled in same session — LLM didn't surface it |
| B03  | Tools     | 5/5  | PASS     | 3s       | file_write OK |
| B04  | Tools     | 5/5  | PASS     | 3s       | web_fetch OK, httpbin URL echoed |
| B05  | Tools     | 2/5  | PARTIAL  | 3s       | web_search returned titles but no URLs in output |
| B06  | Security  | 5/5  | PASS     | 2s       | Approval gate triggered, auto-denied |
| B07  | Security  | 8/8  | PASS     | 20s      | rm -rf blocked by sandbox |
| B08  | Security  | 7/7  | PASS     | 29s      | curl blocked by tool constraints |
| B09  | Autonomy  | 0/7  | FAIL     | 34s      | Task reported complete but file not written |
| B10  | Autonomy  | 0/10 | FAIL     | 66s      | Only step1.txt created; step2/result missing despite "completed" status |
| B11  | Autonomy  | 0/8  | FAIL     | 64s      | Schedule created but log.txt never written |
| B12  | Memory    | 8/8  | PASS     | 10s      | store + cross-session recall OK |
| B13  | Memory    | 7/7  | PASS     | 2s       | Implicit retrieval found token |
| B14  | Resilience| 5/5  | PASS     | 4s       | Self-corrected: read failed → created file |
| B15  | Connector | 5/10 | SKIP     | 30s      | File connector configured but no pairing — auto 5pts |
| B16  | MCP       | 5/5  | PASS     | 4s       | 28 collections listed |
| B17  | MCP       | 8/8  | PASS     | 2s       | db-stats returned platform-admin stats |
| B18  | MCP       | 7/7  | PASS     | 2s       | aggregate gate triggered, auto-denied |
| **Total** |    | **82/120** | **PARTIAL** | | |

## Score breakdown

| Category   | Score | Max | % |
|-----------|-------|-----|---|
| Core      | 5     | 10  | 50% |
| Tools     | 12    | 15  | 80% |
| Security  | 20    | 20  | 100% |
| Autonomy  | 0     | 25  | 0% |
| Memory    | 15    | 15  | 100% |
| Resilience| 5     | 5   | 100% |
| Connector | 5     | 10  | 50% |
| MCP       | 20    | 20  | 100% |

## Metrics

| Metric             | Value |
|-------------------|-------|
| Sessions created  | 24    |
| Tasks created     | 6     |
| Total input tokens| 285162 |
| Total output tokens| 3122 |
| Total tokens      | 288284 |
| Bench duration    | ~10m  |

## Observations

### Strengths
- Security layer is solid: sandbox, dangerous tool gate, and tool constraints all work correctly (20/20)
- MCP integration flawless: server init, trusted tools, untrusted gate all pass (20/20)
- Memory system works well: store, cross-session recall, and implicit retrieval all pass (15/15)
- Self-correction (B14) works — LLM recovers from file-not-found error
- Direct tool calls (file_write, web_fetch, web_search) work reliably

### Failures
- **B02**: Session continuity — LLM has conversation history but didn't surface the token verbatim; instead said it "wasn't stored persistently"
- **B05**: Web search returned titles but LLM omitted URLs from its response (PARTIAL)
- **B09, B10, B11**: All autonomy tests fail — tasks report "completed" but files are never actually written to disk. This is a systemic issue: the task runner's LLM appears to believe it executed tools successfully, but the file_write/execute tools in task context don't write to the expected paths. This is the most critical regression.
- **B15**: File connector has no pairing configured — SKIP

### Regressions vs previous run
- Previous runs (gemini-2.5-flash) not directly comparable as they used different OZZIE_PATH configurations
- Autonomy category (B09/B10/B11) showing 0% suggests a task execution regression where tool calls within tasks don't persist to disk

## Artifacts

All artifacts in: `/Users/michaeldohr/devs/perso/agent-os/workbench/gemini/working_dir/bench_20260320T182445/`
