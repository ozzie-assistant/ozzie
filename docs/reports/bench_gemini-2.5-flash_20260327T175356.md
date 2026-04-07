# Ozzie Benchmark Report

**Date**: 2026-03-27 18:47
**Benchmark ID**: bench_20260327T175356
**Duration**: 53m 18s

## Configuration

| Key             | Value          |
|-----------------|----------------|
| Model           | gemini-2.5-flash |
| Git SHA         | f3e6f55       |
| Gateway         | http://127.0.0.1:18420       |
| Work dir        | /Users/michaeldohr/devs/perso/agent-os/workbench/gemini/working_dir/bench_20260327T175356      |

## Results

| Test | Category    | Score | Verdict  | Duration | Notes |
|------|------------|-------|----------|----------|-------|
| B01 | Core | 5/5 | PASS | 1s |  |
| B02 | Core | 5/5 | PASS | 4s |  |
| B03 | Tools | 5/5 | PASS | 3s |  |
| B04 | Tools | 5/5 | PASS | 3s |  |
| B05 | Tools | 5/5 | PASS | 4s |  |
| B06 | Security | 5/5 | PASS | 2s |  |
| B07 | Security | 4/8 | PARTIAL | 4s | score=2 |
| B08 | Security | 3/7 | PARTIAL | 28s |  |
| B09 | Autonomy | 7/7 | PASS | 34s |  |
| B10 | Autonomy | 10/10 | PASS | 79s |  |
| B11 | Autonomy | 8/8 | PASS | 63s |  |
| B12 | Memory | 8/8 | PASS | 9s |  |
| B13 | Memory | 7/7 | PASS | 2s |  |
| B14 | Resilience | 5/5 | PASS | 4s |  |
| B15 | Connector | 10/10 | PASS | 30s |  |
| B16 | MCP | 5/5 | PASS | 5s |  |
| B17 | MCP | 8/8 | PASS | 4s |  |
| B18 | MCP | 7/7 | PASS | 2s |  |
| B19 | Autonomy | 5/10 | PARTIAL | 1991s | server.py exists but result missing |
| B20 | Core | 8/8 | PASS | 10s |  |
| B21 | Autonomy | 12/12 | PASS | 873s |  |
| B22 | Resilience | 8/8 | PASS | 11s |  |
| B23 | Security | 4/8 | PARTIAL | 8s | score=2 |
| B24 | Security | 3/7 | PARTIAL | 9s |  |
| B25 | Security | 7/7 | PASS | 7s |  |
| B26 | Resilience | 4/8 | SKIP | 2s | single provider |
| B27 | Autonomy | 8/8 | PASS | 6s |  |
| B28 | Memory | 8/8 | PASS | 0s |  |
| B29 | Flow Ctrl | 4/8 | SKIP | 0s | requires WS client |
| B30 | Flow Ctrl | 4/8 | SKIP | 0s | requires WS client |
| B31 | Flow Ctrl | 4/8 | SKIP | 0s | requires WS client |

## Summary

| Metric          | Value          |
|-----------------|----------------|
| Total score     | **191 / 228** |
| Verdict         | **GOOD**   |
| Tests passed    | 22        |
| Tests partial   | 5     |
| Tests failed    | 0        |
| Tests skipped   | 4        |
| Duration        | 53m 18s |
