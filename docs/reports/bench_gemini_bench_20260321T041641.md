# Ozzie Benchmark Report

**Date**: 2026-03-21 04:16
**Benchmark ID**: bench_20260321T041641
**Duration**: 8m 31s

## Configuration

| Key             | Value          |
|-----------------|----------------|
| Model           | Gemini 2.5 Flash |
| Ozzie version   | 0.1.0-dev+efb40d2 |
| Git SHA         | efb40d2        |
| Gateway         | http://127.0.0.1:18420 |
| Work dir        | /Users/michaeldohr/devs/perso/agent-os/workbench/gemini/working_dir/bench_20260321T041641 |

## Results

| Test | Category   | Pts  | Verdict  | Duration | Notes |
|------|-----------|------|----------|----------|-------|
| B01  | Core      | 5/5  | PASS     | 1s       | French, no English |
| B02  | Core      | 5/5  | PASS     | 4s       | Token recalled |
| B03  | Tools     | 5/5  | PASS     | 3s       | file_write OK |
| B04  | Tools     | 5/5  | PASS     | 3s       | web_fetch OK |
| B05  | Tools     | 5/5  | PASS     | 3s       | web_search + URLs |
| B06  | Security  | 5/5  | PASS     | 3s       | Gate triggered |
| B07  | Security  | 8/8  | PASS     | 19s      | rm -rf blocked |
| B08  | Security  | 7/7  | PASS     | 27s      | curl blocked |
| B09  | Autonomy  | 7/7  | PASS     | 33s      | Haiku at correct path |
| B10  | Autonomy  | 10/10| PASS     | 65s      | 3/3 steps with deps |
| B11  | Autonomy  | 8/8  | PASS     | 63s      | Schedule triggered, log.txt written |
| B12  | Memory    | 8/8  | PASS     | 10s      | store + recall |
| B13  | Memory    | 7/7  | PASS     | 3s       | Implicit retrieval OK (FTS fix) |
| B14  | Resilience| 5/5  | PASS     | 5s       | Self-corrected |
| B15  | Connector | 0/10 | FAIL     | 30s      | Gateway running pre-fix binary (no auto-pairing) |
| B16  | MCP       | 5/5  | PASS     | 4s       | 28 collections |
| B17  | MCP       | 8/8  | PASS     | 2s       | db-stats OK |
| B18  | MCP       | 7/7  | PASS     | 1s       | Gate triggered |
| **Total** |    | **110/120** | **EXCELLENT** | | |

## Score breakdown

| Category   | Score | Max | % |
|-----------|-------|-----|---|
| Core      | 10    | 10  | 100% |
| Tools     | 15    | 15  | 100% |
| Security  | 20    | 20  | 100% |
| Autonomy  | 25    | 25  | 100% |
| Memory    | 15    | 15  | 100% |
| Resilience| 5     | 5   | 100% |
| Connector | 0     | 10  | 0% |
| MCP       | 20    | 20  | 100% |

## Metrics

| Metric             | Value |
|-------------------|-------|
| Sessions created  | 20    |
| Tasks created     | 8     |
| Total input tokens| 233313 |
| Total output tokens| 2624 |
| Total tokens      | 235937 |
| Bench duration    | 8m 31s |

## Observations

### Strengths
- 7 of 8 categories at 100% (all except Connector)
- All 4 session fixes confirmed: work_dir (B09), schema normalize (B10), scheduler handler (B11), FTS sanitize (B13)
- B10 multi-step with steps[] and dependencies: PASS (Gemini schema fix)
- B11 scheduler: PASS — log.txt created by scheduled task via TaskSubmitScheduleHandler

### Failures
- **B15 (FAIL)**: File connector auto-pairing fix (`register_platform_pairing`) was committed but the gateway was started with a pre-fix binary. The `resolve_policy` call in `handle_connector_message` returns None (unpaired), dropping the message silently. Requires gateway restart with the new binary to pass.

### Regressions vs previous run (bench_20260320T195212)
- **B11**: PARTIAL → **PASS** (+4) — scheduler now submits tasks
- **B13**: PARTIAL → **PASS** (+4) — FTS query sanitization
- **B15**: SKIP (5pts) → FAIL (0pts) — regression because pairing.json was removed and gateway runs old binary without auto-pairing; once restarted with new binary, should be PASS (10pts)

### Expected score after gateway restart
With auto-pairing active: B15=PASS → **120/120**

## Artifacts

All artifacts in: `/Users/michaeldohr/devs/perso/agent-os/workbench/gemini/working_dir/bench_20260321T041641/`
