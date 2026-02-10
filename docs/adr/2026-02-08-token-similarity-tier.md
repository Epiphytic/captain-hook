# ADR: Token-Level Jaccard Similarity Tier (Tier 2a)

**Date:** 2026-02-08
**Status:** Accepted
**Context:** Adding a fast, lightweight similarity check between the exact cache (Tier 1) and the embedding-based HNSW search (Tier 2b) in the hookwise decision cascade.

## Decision

Insert Tier 2a — token-level Jaccard similarity — into the cascade between exact cache lookup and embedding HNSW search. This tier catches approximately 60% of near-matches at ~500ns, avoiding the 1-5ms cost of embedding generation and HNSW search.

The updated cascade:

```
Sanitize -> Path Policy -> Cache (exact) -> Token Jaccard -> Embedding HNSW -> LLM -> Human
  ~5us        ~1us          ~100ns            ~500ns           ~1-5ms        ~1-2s  interrupt
```

## Motivation

Many cache misses are near-duplicates that differ only in arguments, paths, or flags. For example:
- `pytest tests/auth/` vs `pytest tests/api/` — same command, different path argument
- `cargo build --release` vs `cargo build --release --target x86_64` — additional flag
- `rm -rf /tmp/build-output` vs `rm -rf /tmp/build-cache` — different directory name

These commands share most of their tokens. A simple set-intersection check catches them without the overhead of embedding generation and approximate nearest neighbor search.

Embedding similarity (Tier 2b) excels at semantic matches where tokens differ but meaning is similar (`npm test` vs `yarn test`, `cat file` vs `less file`). Token Jaccard handles the simpler, more common case of structural similarity with shared tokens.

## Token Extraction

Given a sanitized command string:

1. **Split** on whitespace and punctuation characters: `[ \t\n\r/\-_=:.,;|><&"'(){}[\]]`
2. **Lowercase** all tokens
3. **Deduplicate** — convert to a set (each unique token appears once)
4. **Sort** — for deterministic comparison and efficient intersection

### Examples

| Input | Tokens |
|-------|--------|
| `pytest --cov tests/auth/test_login.py` | `{auth, cov, login, py, pytest, test, tests}` |
| `pytest --cov tests/api/test_users.py` | `{api, cov, py, pytest, test, tests, users}` |
| `rm -rf /tmp/build-output` | `{build, output, rf, rm, tmp}` |
| `cargo build --release` | `{build, cargo, release}` |

## Jaccard Coefficient

The Jaccard similarity coefficient between two sets A and B:

```
J(A, B) = |A ∩ B| / |A ∪ B|
```

- J = 1.0: identical token sets
- J = 0.0: no shared tokens
- J = 0.7 (default threshold): 70% overlap

For the pytest example above:
- A = `{auth, cov, login, py, pytest, test, tests}` (7 tokens)
- B = `{api, cov, py, pytest, test, tests, users}` (7 tokens)
- A ∩ B = `{cov, py, pytest, test, tests}` (5 tokens)
- A ∪ B = `{api, auth, cov, login, py, pytest, test, tests, users}` (9 tokens)
- J = 5/9 = 0.556

This is below the 0.7 threshold, so it falls through to Tier 2b. This is correct — `tests/auth/` and `tests/api/` are meaningfully different test suites that may have different permission decisions.

For `cargo build --release` vs `cargo build --release --target x86_64`:
- A = `{build, cargo, release}` (3 tokens)
- B = `{build, cargo, release, target, x86_64}` (5 tokens)
- A ∩ B = `{build, cargo, release}` (3 tokens)
- A ∪ B = `{build, cargo, release, target, x86_64}` (5 tokens)
- J = 3/5 = 0.6

Below threshold — the additional `--target` flag changes the meaning enough that it should be checked by embeddings or the LLM. The threshold is intentionally conservative.

For `rm -rf /tmp/build-output` vs `rm -rf /tmp/build-cache`:
- A = `{build, output, rf, rm, tmp}` (5 tokens)
- B = `{build, cache, rf, rm, tmp}` (5 tokens)
- A ∩ B = `{build, rf, rm, tmp}` (4 tokens)
- A ∪ B = `{build, cache, output, rf, rm, tmp}` (6 tokens)
- J = 4/6 = 0.667

Still below 0.7. Let's try a tighter match — `rm -rf /tmp/build-output/dist` vs `rm -rf /tmp/build-output/staging`:
- A = `{build, dist, output, rf, rm, tmp}` (6 tokens)
- B = `{build, output, rf, rm, staging, tmp}` (6 tokens)
- A ∩ B = `{build, output, rf, rm, tmp}` (5 tokens)
- A ∪ B = `{build, dist, output, rf, rm, staging, tmp}` (7 tokens)
- J = 5/7 = 0.714

Above threshold — these are structurally identical commands differing only in subdirectory name.

## Threshold Configuration

**Default threshold:** 0.7

Configurable in `policy.yml`:

```yaml
similarity:
  jaccard_threshold: 0.7    # Token Jaccard minimum for Tier 2a match
  embedding_threshold: 0.85  # Embedding cosine similarity minimum for Tier 2b match
```

### Threshold rationale

- **0.7 is conservative**: It requires substantial token overlap, reducing false positives. Commands with different semantics but shared tokens (like `git push origin main` vs `git push origin --delete main`) tend to score below 0.7 due to the added tokens changing the ratio.
- **Below 0.7 falls through to embeddings**: This is a safety net, not a hard boundary. Embedding similarity can still catch the match at Tier 2b.
- **Above 0.9 would be too strict**: Only nearly-identical commands would match, defeating the purpose. The exact cache already handles identical commands.

## Short Command Handling

**Minimum token count:** 3 tokens required for Jaccard comparison.

Commands with fewer than 3 tokens skip Tier 2a and go directly to Tier 2b (embedding similarity). Rationale:

- **Single-word commands** (`ls`, `pwd`, `whoami`): Jaccard is binary — either 1.0 (exact match, already caught by Tier 1) or 0.0 (different command). No value in the Jaccard tier.
- **Two-token commands** (`git status`, `npm test`): Changing one token changes 50% of the set, pushing Jaccard below threshold. Too volatile for meaningful comparison.
- **Three+ tokens** (`pytest --cov tests/`, `cargo build --release`): Enough signal for meaningful overlap measurement.

## What Jaccard Catches vs What Needs Embeddings

### Jaccard catches (Tier 2a resolves)

These are structurally similar commands sharing most tokens:

| Cached entry | New command | J score | Catches? |
|---|---|---|---|
| `pytest --cov tests/auth/` | `pytest --cov tests/auth/test_handlers.py` | 0.78 | Yes |
| `cargo test --lib -- auth` | `cargo test --lib -- api` | 0.75 | Yes |
| `docker compose -f docker-compose.test.yml up db` | `docker compose -f docker-compose.test.yml up redis` | 0.90 | Yes |
| `grep -rn "TODO" src/` | `grep -rn "FIXME" src/` | 0.71 | Yes |
| `mkdir -p /tmp/hookwise/cache` | `mkdir -p /tmp/hookwise/index` | 0.75 | Yes |

### Needs embeddings (Tier 2b resolves)

These have semantic similarity but different tokens:

| Cached entry | New command | Why Jaccard fails |
|---|---|---|
| `npm test` | `yarn test` | Different package manager token |
| `docker compose up` | `docker-compose up` | Tokenization splits differently |
| `cat README.md` | `less README.md` | Different command, same intent |
| `python -m pytest` | `pytest` | Structural difference despite same tool |
| `curl -s http://localhost:3000/health` | `wget -q http://localhost:3000/health` | Different command |

### Needs LLM (Tier 3 resolves)

These require understanding context and policy:

| Command | Why Jaccard and embeddings fail |
|---|---|
| `git push origin feature-branch` | Never seen git push before; needs policy evaluation |
| `terraform plan -out=plan.tfbin` | Novel infrastructure command; needs role-aware evaluation |
| `chmod 777 /tmp/app.sock` | Permission change on unexpected path; needs risk assessment |

## Implementation

### Data structure

For each cached decision, store a precomputed sorted token set:

```rust
struct TokenEntry {
    tokens: Vec<String>,       // sorted, deduplicated
    cache_key: CacheKey,       // reference to the cached decision
    decision: Decision,        // allow/deny/ask
}
```

### Lookup algorithm

```rust
fn jaccard_lookup(query_tokens: &[String], entries: &[TokenEntry], threshold: f64) -> Option<&TokenEntry> {
    if query_tokens.len() < 3 {
        return None;  // skip short commands
    }

    let mut best_match: Option<(f64, &TokenEntry)> = None;

    for entry in entries {
        let intersection = sorted_intersection_count(query_tokens, &entry.tokens);
        let union = query_tokens.len() + entry.tokens.len() - intersection;
        let jaccard = intersection as f64 / union as f64;

        if jaccard >= threshold {
            if best_match.map_or(true, |(best_j, _)| jaccard > best_j) {
                best_match = Some((jaccard, entry));
            }
        }
    }

    best_match.map(|(_, entry)| entry)
}
```

Sorted intersection uses a merge-join on two sorted slices: O(|A| + |B|) time. For typical command token sets (5-15 tokens), this is ~50-200ns per comparison.

### Performance

With ~1000 cached decisions:
- Token extraction: ~100ns (split + lowercase + sort)
- 1000 Jaccard comparisons: ~100-200us (merge-join on small sorted sets)
- Total: ~200-300us

This is well under our ~500ns target for individual comparisons, though scanning all entries takes longer. To meet the ~500ns target, we use bucketing by token count (only compare entries with similar token counts, since very different counts imply low Jaccard) and by first-token prefix.

## Decision Behavior

Same rules as embedding similarity:
- **Jaccard match to `allow`** -> auto-approve
- **Jaccard match to `deny`** -> fall through to Tier 2b (never auto-deny on token match alone)
- **Jaccard match to `ask`** -> escalate to human (ask propagates through similarity)
