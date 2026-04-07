---
title: "Prompting Claude — Practical Guide"
---

## Declare the expected output type

Start the prompt by specifying what you expect:

| Phrasing | What Claude produces |
|----------|---------------------|
| `plan:` | Implementation plan, no code |
| `execute:` | Code directly, no prior plan |
| `explain:` | Explanation of a concept or behavior |
| `answer:` | Short answer, no code, no plan |

Without instruction, Claude chooses — usually too broad.

## Prompt structure

### Short task (bug, question, adjustment)

```
<output type>
<task description>
<example or edge case if needed>
```

### Complex or structural task

```
<output type>
Goal: <vision in one sentence — what we are trying to achieve>
Context: <current situation, reason for the task>
Task: <precise description>
- edge case 1
- edge case 2
Constraints: <what NOT to do>
```

The goal is only useful for structural tasks or when Claude has drifted. Do not repeat it in every prompt — that is the role of `CLAUDE.md`.

## Scope explicitly

Tell Claude what to do **and what not to do**:

- `modify only X, do not touch Y`
- `refactor without changing observable behavior`
- `answer without suggesting alternatives`

Lack of scope is the primary cause of overly broad responses.

## Syntax & tone

- Bullet points over long sentences
- Markdown headings to separate sections in a complex prompt
- Direct tone — avoid polite hedging that softens the intent (`if possible`, `ideally`, `you could maybe`)
- Vague phrasing produces vague responses

## Iteration

Start small, validate, then expand — do not ask for the complete solution in the first prompt.

```
# Avoid
"Implement the full billing module with business rules, tests and documentation"

# Prefer
"Implement the billing domain model" → validate → "add commands" → ...
```

## When to open a new conversation

Starting fresh is often more effective than correcting:
- The context contains multiple failed attempts
- Claude is going in circles or repeating the same mistake
- The direction has changed significantly

In that case: summarize the current state in 2-3 points, open a new session, start clean.

## Anchor Claude to concrete references

Concrete references are more reliable than descriptions. Provide them explicitly.

**File paths** — point to existing files rather than describing their content:
```
Read src/domain/user.rs before answering.
Follow the pattern in di/company_container.rs.
```

**Existing patterns** — Claude imitates better than it invents. Pointing to a working example
produces more consistent results than describing the pattern in prose:
```
Follow the XxxContainer pattern from di/company_container.rs — same two-phase init, same accessor structure.
```

**URLs and documentation** — provide the exact URL for external references. Do not rely on
Claude's training data for fast-moving libraries:
```
See https://docs.rs/leptos/latest/leptos/ — check the current API before answering.
```
For versioned libraries (Leptos, Tokio, etc.), explicitly ask Claude to consult the source:
```
Use context7 to verify the current Leptos 0.8 API before generating code.
```

**Definition of done** — state the completion criterion to avoid partial or over-engineered responses:
```
Done when the four quality gates pass.
Answer in 3 bullet points max.
Stop after the domain model — do not implement the infrastructure layer.
```

## Correcting and refocusing

**If Claude drifts:** refocus with the goal and constraint, not a long explanation.

```
# Too long
"No that's not what I meant, what I'm actually looking for is..."

# Effective
"Goal: X. Constraint: no Y. Start over."
```

**If Claude is right and you are wrong:** accept directly, no justification. Any non-constructive explanation pollutes the context and steers Claude toward validating your position instead of the right solution.

```
# Avoid
"Yes you're right but in my case it's different because..."

# Effective
"OK. Continue with your approach."
```

**If Claude is right but you have a verified edge case:** skip the apology, state the edge case as a fact.

```
# Avoid
"You're right in general but sorry I should have mentioned that in our case..."

# Effective
"Edge case: <precise description>. Does this change the approach?"
```

**Interrupt + examples:** if the response is heading in the wrong direction, interrupt immediately, provide a concrete example (code, diagram, counter-example), then restart.
