# Ozzie Plugin Development — MCP-assisted workflow

You are developing an Ozzie WASM plugin with **live MCP testing**. This skill guides you through discovery, design, implementation, and validation — with real MCP calls to verify behavior.

## Arguments

- `$ARGUMENTS` — Plugin name and optional context (e.g. `github "Interact with GitHub API"`)

Parse the first word as the plugin name. The rest is optional context — you will gather full requirements in Phase 0.

---

## Workflow overview

```
Discover → Design → Scaffold → Code → Build → Test via MCP → Iterate → Finalize
```

---

## Phase 0 — Discovery & Requirements

Before writing any code, gather all the information needed to design the plugin.

### 0.1 — Check if plugin already exists

Look for `plugins/<name>/` in the codebase. If the plugin exists:

1. Read the existing `manifest.jsonc` and `main.go`
2. Show the user what tools already exist with their descriptions
3. Ask: **What changes do you want?** (add tools, modify behavior, fix bugs, refactor)
4. Skip to the relevant phase based on the answer

### 0.2 — Gather requirements (new plugin)

Ask the user the following questions. Adapt based on what was already provided in `$ARGUMENTS`.

**Business context** (optional but valuable):
- What is the business purpose of this plugin?
- What system/API/service does it interact with?
- Are there security or compliance constraints?

**Tools to implement:**
For each tool, collect:

| Field | Description | Example |
|-------|-------------|---------|
| **Name** | Snake_case identifier | `list_jobs` |
| **Description** | What the tool does (in English, 1 sentence) | `List all scheduled jobs with their status` |
| **Input parameters** | Name, type, required?, description | `status: string, optional — filter by status` |
| **Output** | What the tool returns | `JSON array of job objects` |
| **Side effects** | Does it modify state? (→ `dangerous: true`) | `No, read-only` |
| **Role** | Read, write, admin, monitoring... | `Read` |

Present a summary table of all tools and ask for confirmation before proceeding.

### 0.3 — Existing implementation

Ask: **Is there an existing implementation** (library, API client, CLI tool, code snippet) that you want to adapt or wrap?

If yes:
- Ask for the source (URL, file path, or paste)
- Read and analyze the existing code
- Map existing functions to Ozzie tool signatures
- Note what needs to change (TinyGo compatibility, JSON I/O, no net/http, etc.)

### 0.4 — Capabilities assessment

Based on the gathered requirements, determine the minimum capabilities needed:

| Capability | When needed |
|------------|-------------|
| `"http": { "allowed_hosts": [...] }` | Tool calls external APIs |
| `"kv": true` | Tool needs persistent key-value storage |
| `"log": true` | Tool needs structured logging |
| `"filesystem": { "allowed_paths": {...} }` | Tool reads/writes files |

**Deny-by-default** — only grant what's strictly required.

### 0.5 — Design summary

Before moving to implementation, present a clear summary:

```
Plugin: <name>
Description: <what the plugin does>
Tools: <N> tools
Capabilities: <list>
Dangerous: <which tools and why>

Tools:
  1. <tool_name> — <description>
     Input:  { <params> }
     Output: { <shape> }
     Dangerous: yes/no

  2. ...
```

Wait for user approval before proceeding to Phase 1.

---

## Phase 1 — Scaffold

Create the plugin source under `plugins/<name>/`:

### go.mod

```
module github.com/dohr-michael/ozzie/plugins/<name>

go 1.23

require github.com/extism/go-pdk v1.1.1
```

Run `cd plugins/<name> && go mod tidy` after creating.

### manifest.jsonc

```jsonc
{
    "name": "<name>",
    "description": "<description — ENGLISH>",
    "level": "tool",
    "provider": "extism",
    "wasm_path": "<name>.wasm",
    "dangerous": false,
    "capabilities": {
        // From Phase 0.4 assessment
    },
    "tools": [
        {
            "name": "<tool_name>",
            "description": "<ENGLISH — clear, concise, for LLM consumption>",
            "func": "<wasm_export>",       // omit for single-tool (defaults to "handle")
            "dangerous": false,             // true if modifies state
            "parameters": {
                "<param>": {
                    "type": "string",       // string | number | integer | boolean
                    "description": "<ENGLISH>",
                    "required": true
                }
            }
        }
    ]
}
```

**CRITICAL: All `description` fields in the manifest MUST be in English.** These descriptions are consumed by LLMs via MCP — they must be clear, concise, and in English for maximum compatibility.

### main.go — single-tool pattern

```go
package main

import (
    "encoding/json"
    "github.com/extism/go-pdk"
)

//export handle
func handle() int32 {
    input := pdk.Input()
    var req myInput
    if err := json.Unmarshal(input, &req); err != nil {
        return outputError("invalid input: " + err.Error())
    }
    // ... logic ...
    pdk.Output(resultJSON)
    return 0
}

func outputError(msg string) int32 {
    out, _ := json.Marshal(map[string]string{"error": msg})
    pdk.Output(out)
    return 1
}

func main() {}
```

### main.go — multi-tool pattern

```go
//export list_items
func listItems() int32 { /* ... */ }

//export create_item
func createItem() int32 { /* ... */ }

func main() {}
```

### Host functions (via go-pdk)

- `pdk.GetVar(key)` / `pdk.SetVar(key, val)` — KV store (needs `"kv": true`)
- `pdk.Log(level, msg)` — logging (needs `"log": true`)
- `pdk.HttpRequest(req, body)` — HTTP (needs `"http"` capability)

---

## Phase 2 — Build

Build the WASM into `build/plugins/<name>/` alongside its manifest:

```bash
mkdir -p build/plugins/<name>
cd plugins/<name> && tinygo build -target wasip1 -o ../../build/plugins/<name>/<name>.wasm .
cp plugins/<name>/manifest.jsonc build/plugins/<name>/
```

Also rebuild the ozzie binary if needed: `go build -o build/ozzie ./cmd/ozzie`

TinyGo limitations: no `reflect`, no `net/http` (use `pdk.HttpRequest`), no goroutines, limited stdlib.

If build fails, fix and retry before moving on.

---

## Phase 3 — Configure MCP for testing

Ensure `.mcp.json` in the project root has an entry for the plugin under development.
The config points to `build/plugins` via `configs/config.dev.jsonc`, and `OZZIE_PATH` is
set to `build/ozzie-home` to isolate dev from `~/.ozzie`:

```json
{
    "mcpServers": {
        "ozzie-dev": {
            "type": "stdio",
            "command": "./build/ozzie",
            "args": ["mcp-serve", "--config", "./configs/config.dev.jsonc"],
            "env": {
                "OZZIE_PATH": "./build/ozzie-home"
            }
        }
    }
}
```

To expose only the plugin under development, add its name as an extra arg:
```json
"args": ["mcp-serve", "--config", "./configs/config.dev.jsonc", "<name>"]
```

Before testing, build everything: `make build-all`

---

## Phase 4 — Test via MCP (the core loop)

**Call the tool directly using the MCP server.** The `ozzie-dev` MCP server exposes the plugin's tools. Use them as you would any MCP tool — pass arguments, get results, analyze.

### Test loop

1. **Call the tool** via MCP with representative inputs
2. **Analyze the result** — check for errors, unexpected output, edge cases
3. **If it works** — try edge cases (empty inputs, invalid params, boundary values)
4. **If it fails** — read the error, fix `main.go`, rebuild (`tinygo build` + copy to `build/plugins/`), and call again

### What to test

- Happy path with valid inputs
- Missing required parameters
- Invalid parameter types or values
- Edge cases specific to the tool's domain
- Error messages are clear and useful

Each iteration: **edit → build → copy to build/plugins → call MCP tool → check result → repeat**

---

## Phase 5 — Finalize

Once all tools work correctly via MCP:

1. **Add to Makefile** — add build target in `build-plugins` and verify `clean` covers `build/`
2. **Run quality gates**:
   ```bash
   go build ./...
   ~/go/bin/staticcheck ./...
   go test ./...
   ```
3. **Summarize** what was built: tools, capabilities, any caveats

---

## Rules

- **Respond in French** — all explanations and conversations with the user
- **Code in English** — ALL code, comments, identifiers, descriptions in manifest, commit messages
- **Manifest descriptions in English** — these are consumed by LLMs via MCP, they MUST be in English
- **Deny-by-default capabilities** — never grant more than needed
- **`dangerous: true`** on tools that modify state, run commands, or write to filesystem
- **JSON in, JSON out** — all tool I/O is JSON. Errors: `{"error": "message"}`
- **Build artifacts in `build/`** — source in `plugins/`, WASM+manifest in `build/plugins/`
- **Iterate with real MCP calls** — don't just assume the tool works, call it and verify
- **Always check existing code first** — never overwrite without understanding what's there
