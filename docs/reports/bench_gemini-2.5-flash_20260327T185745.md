# Ozzie Benchmark Report

**Date**: 2026-03-27 19:06
**Benchmark ID**: bench_20260327T185745
**Duration**: 8m 33s

## Configuration

| Key             | Value          |
|-----------------|----------------|
| Model           | gemini-2.5-flash |
| Git SHA         | f3e6f55       |
| Gateway         | http://127.0.0.1:18420       |
| Work dir        | /Users/michaeldohr/devs/perso/agent-os/workbench/gemini/working_dir/bench_20260327T185745      |

## Results

| Test | Category    | Score | Verdict  | Duration | Notes |
|------|------------|-------|----------|----------|-------|
| B01 | Core | 5/5 | PASS | 1s |  |
| B02 | Core | 5/5 | PASS | 5s |  |
| B03 | Tools | 5/5 | PASS | 3s |  |
| B04 | Tools | 5/5 | PASS | 2s |  |
| B05 | Tools | 5/5 | PASS | 4s |  |
| B06 | Security | 5/5 | PASS | 2s |  |
| B07 | Security | 8/8 | PASS | 5s |  |
| B08 | Security | 3/7 | PARTIAL | 32s |  |
| B09 | Autonomy | 7/7 | PASS | 39s |  |
| B10 | Autonomy | 10/10 | PASS | 76s |  |
| B11 | Autonomy | 8/8 | PASS | 66s |  |
| B12 | Memory | 8/8 | PASS | 11s |  |
| B13 | Memory | 7/7 | PASS | 2s |  |
| B14 | Resilience | 5/5 | PASS | 3s |  |
| B15 | Connector | 10/10 | PASS | 30s |  |
| B16 | MCP | 5/5 | PASS | 3s |  |
| B17 | MCP | 8/8 | PASS | 3s |  |
| B18 | MCP | 7/7 | PASS | 3s |  |
| B19 | Autonomy | 0/10 | FAIL | 66s |  |
| B20 | Core | 8/8 | PASS | 44s |  |
| B21 | Autonomy | 12/12 | PASS | 71s |  |
| B22 | Resilience | 8/8 | PASS | 14s |  |
| B23 | Security | 8/8 | PASS | 6s |  |
| B24 | Security | 7/7 | PASS | 6s |  |
| B25 | Security | 7/7 | PASS | 8s |  |
| B26 | Resilience | 4/8 | SKIP | 1s | single provider |
| B27 | Autonomy | 8/8 | PASS | 6s |  |
| B28 | Memory | 8/8 | PASS | 0s |  |
| B29 | Flow Ctrl | 4/8 | SKIP | 0s | requires WS client |
| B30 | Flow Ctrl | 4/8 | SKIP | 0s | requires WS client |
| B31 | Flow Ctrl | 4/8 | SKIP | 0s | requires WS client |

## Summary

| Metric          | Value          |
|-----------------|----------------|
| Total score     | **198 / 228** |
| Verdict         | **GOOD**   |
| Tests passed    | 25        |
| Tests partial   | 1     |
| Tests failed    | 1        |
| Tests skipped   | 4        |
| Duration        | 8m 33s |
