# Bash Command Path Extraction Regexes

**Date:** 2026-02-08
**Purpose:** Regex patterns for extracting file paths from bash tool input in hookwise's Tier 0 path policy evaluation

## Overview

When Claude Code invokes the `Bash` tool, hookwise needs to extract file paths from the command string to evaluate them against role-based path policies. This is critical: a `coder` role should be denied `rm -rf tests/` even though they have bash access.

These patterns are designed for Rust's `regex` crate with named capture groups. They operate on the raw command string, not a parsed AST -- this is intentional for speed (~microsecond budget) at the cost of some edge cases.

## Design Principles

1. **Named capture groups** -- `(?P<path>...)` for Rust `regex` named captures
2. **Paths are greedy but bounded** -- match until whitespace, pipe, semicolon, or redirect
3. **Quoted paths handled separately** -- single/double quotes get dedicated alternations
4. **Conservative extraction** -- better to miss a path than to false-positive on a non-path argument
5. **Multiple passes** -- run all patterns and union the extracted paths

### Common Path Pattern Fragment

Reusable fragment for "a thing that looks like a file path":

```
# Unquoted path: starts with / . ~ or word char, no spaces
(?P<path>(?:[/~.]|\w)[\w./_~*?{}\[\]-]*)

# Quoted path variants:
"(?P<qpath>[^"]+)"
'(?P<sqpath>[^']+)'
```

Combined path alternation used in patterns below:

```rust
const PATH: &str = r#"(?:"(?P<{name}_q>[^"]+)"|'(?P<{name}_sq>[^']+)'|(?P<{name}>(?:[/~.]|\w)[\w./_~*?\[\]{}-]*))"#;
```

---

## Pattern 1: `rm` -- Remove files/directories

### Regex

```rust
r#"(?:^|[;&|]\s*)rm\s+(?P<flags>(?:-[rifvdIRP]+\s+)*)(?:"(?P<path_q>[^"]+)"|'(?P<path_sq>[^']+)'|(?P<path>(?:[/~.]|\w)[\w./_~*?\[\]{}-]*))"#
```

### What it matches

Extracts the first path argument after `rm` and any single-letter flags.

### Test cases

| Input | Extracted path(s) |
|-------|-------------------|
| `rm file.txt` | `file.txt` |
| `rm -rf /tmp/build` | `/tmp/build` |
| `rm -f "path with spaces/file.txt"` | `path with spaces/file.txt` |
| `rm -rf src/ tests/` | `src/` (first only -- see multi-path note) |
| `echo hi && rm -f foo.txt` | `foo.txt` |

### Multi-path extraction

`rm` can take multiple paths. To extract all paths after flags:

```rust
r#"(?:^|[;&|]\s*)rm\s+(?:-[rifvdIRP]+\s+)*(?P<paths>.+?)(?:\s*(?:[;&|>]|$))"#
```

Then split `paths` on unquoted whitespace to get individual paths.

### Edge cases

- `rm -- -weird-filename` -- `--` signals end of flags; path starts after it
- `rm -rf ${VAR}` -- variable expansion; extract literal `${VAR}` and mark as unresolvable
- `rm -rf *.log` -- glob pattern; extract and let the path policy evaluate `*.log`

### Limitations

- Does not resolve `~` or environment variables
- Does not handle `xargs rm` or `find -exec rm`
- Brace expansion (`rm file{1,2,3}.txt`) is extracted as literal

---

## Pattern 2: `mv` -- Move/rename

### Regex

```rust
r#"(?:^|[;&|]\s*)mv\s+(?P<flags>(?:-[fintuvTSZ]+\s+)*)(?:"(?P<src_q>[^"]+)"|'(?P<src_sq>[^']+)'|(?P<src>(?:[/~.]|\w)[\w./_~*?\[\]{}-]*))\s+(?:"(?P<dst_q>[^"]+)"|'(?P<dst_sq>[^']+)'|(?P<dst>(?:[/~.]|\w)[\w./_~*?\[\]{}-]*))"#
```

### What it matches

Extracts source and destination paths. Both are relevant for path policy:
- Source: being removed from original location
- Destination: being written to new location

### Test cases

| Input | Source | Destination |
|-------|--------|-------------|
| `mv old.txt new.txt` | `old.txt` | `new.txt` |
| `mv -f src/lib.rs src/main.rs` | `src/lib.rs` | `src/main.rs` |
| `mv "my file.txt" /tmp/` | `my file.txt` | `/tmp/` |
| `mv ../foo ./bar` | `../foo` | `./bar` |

### Edge cases

- `mv -t /dest file1 file2 file3` -- `-t` flag changes argument order (target first)
- `mv file1 file2 /directory/` -- multiple sources, last arg is destination

### Limitations

- Does not handle `-t` target-directory flag reordering
- Multi-source `mv src1 src2 dest/` only captures first two args as src/dst

---

## Pattern 3: `cp` -- Copy

### Regex

```rust
r#"(?:^|[;&|]\s*)cp\s+(?P<flags>(?:-[raflinpuvRPdHLsxTZ]+\s+)*)(?:"(?P<src_q>[^"]+)"|'(?P<src_sq>[^']+)'|(?P<src>(?:[/~.]|\w)[\w./_~*?\[\]{}-]*))\s+(?:"(?P<dst_q>[^"]+)"|'(?P<dst_sq>[^']+)'|(?P<dst>(?:[/~.]|\w)[\w./_~*?\[\]{}-]*))"#
```

### What it matches

Extracts source and destination. The destination is the write target for path policy.

### Test cases

| Input | Source | Destination |
|-------|--------|-------------|
| `cp file.txt backup.txt` | `file.txt` | `backup.txt` |
| `cp -r src/ /tmp/src-backup/` | `src/` | `/tmp/src-backup/` |
| `cp -a "my project/" /mnt/` | `my project/` | `/mnt/` |

### Edge cases

Same as `mv` -- `-t` flag, multiple sources.

### Limitations

Same as `mv`.

---

## Pattern 4: `mkdir` -- Create directory

### Regex

```rust
r#"(?:^|[;&|]\s*)mkdir\s+(?P<flags>(?:-[pmvZ]+\s+)*)(?:"(?P<path_q>[^"]+)"|'(?P<path_sq>[^']+)'|(?P<path>(?:[/~.]|\w)[\w./_~*?\[\]{}-]*))"#
```

### What it matches

Extracts the directory path being created. Always a write operation.

### Test cases

| Input | Extracted path |
|-------|---------------|
| `mkdir /tmp/build` | `/tmp/build` |
| `mkdir -p src/new/module` | `src/new/module` |
| `mkdir "path with spaces"` | `path with spaces` |
| `mkdir -p -m 755 /opt/app` | `/opt/app` |

### Edge cases

- `mkdir -p a/b/c` creates intermediate dirs; policy should check the deepest path
- `mkdir dir1 dir2 dir3` creates multiple dirs

### Limitations

- Does not extract `-m` mode argument separately

---

## Pattern 5: `touch` -- Create/update file

### Regex

```rust
r#"(?:^|[;&|]\s*)touch\s+(?P<flags>(?:-[acmr]+\s+(?:\S+\s+)?)*)(?:"(?P<path_q>[^"]+)"|'(?P<path_sq>[^']+)'|(?P<path>(?:[/~.]|\w)[\w./_~*?\[\]{}-]*))"#
```

### What it matches

Extracts the file path being created/updated. Write operation.

### Test cases

| Input | Extracted path |
|-------|---------------|
| `touch newfile.txt` | `newfile.txt` |
| `touch /tmp/test.log` | `/tmp/test.log` |
| `touch "my file.txt"` | `my file.txt` |
| `touch -r ref.txt target.txt` | `target.txt` |

### Edge cases

- `-r` flag takes a reference file argument before the target path

### Limitations

- Does not distinguish `-r reference` from the target path in complex cases

---

## Pattern 6: Output Redirects (`>`, `>>`)

### Regex

```rust
r#"(?:(?:^|[^>])>{1,2})\s*(?:"(?P<path_q>[^"]+)"|'(?P<path_sq>[^']+)'|(?P<path>(?:[/~.]|\w)[\w./_~*?\[\]{}-]*))"#
```

### What it matches

Extracts the file path after `>` (overwrite) or `>>` (append). Always a write operation.

### Test cases

| Input | Extracted path |
|-------|---------------|
| `echo hello > output.txt` | `output.txt` |
| `cat file >> /tmp/log.txt` | `/tmp/log.txt` |
| `cmd > "path with spaces.txt"` | `path with spaces.txt` |
| `echo a > b > c` | `b`, `c` (both are write targets) |
| `cmd 2>/dev/null` | `/dev/null` |

### Edge cases

- `2>` (stderr redirect) is also a write; pattern matches it
- `>&2` (fd redirect) should NOT match a path -- the `[^>]` lookbehind helps
- Heredoc `<< EOF` should NOT match -- the direction is opposite

### Limitations

- `cmd > $OUTPUT_FILE` extracts `$OUTPUT_FILE` literally
- Does not distinguish `>` (destructive overwrite) from `>>` (append) -- both are write targets
- `noclobber` (`>|`) variant not explicitly handled but `>` still matches

---

## Pattern 7: `tee` -- Pipe target

### Regex

```rust
r#"\|\s*tee\s+(?P<flags>(?:-[ai]+\s+)*)(?:"(?P<path_q>[^"]+)"|'(?P<path_sq>[^']+)'|(?P<path>(?:[/~.]|\w)[\w./_~*?\[\]{}-]*))"#
```

### What it matches

Extracts the file path written by `tee`. Write operation.

### Test cases

| Input | Extracted path |
|-------|---------------|
| `cmd \| tee output.log` | `output.log` |
| `cmd \| tee -a /var/log/app.log` | `/var/log/app.log` |
| `cmd \| tee "my log.txt"` | `my log.txt` |

### Edge cases

- `tee file1 file2` writes to multiple files
- `tee` without pipe (standalone) is unusual but valid

### Limitations

- Only matches `tee` after a pipe `|`; standalone `tee` not captured

---

## Pattern 8: `sed -i` -- In-place file edit

### Regex

```rust
r#"(?:^|[;&|]\s*)sed\s+(?P<flags>(?:-[nEerz]+\s+)*)(?:-i(?:\.(?P<backup>\S+))?\s+)(?:'[^']*'|"[^"]*"|\S+)\s+(?:"(?P<path_q>[^"]+)"|'(?P<path_sq>[^']+)'|(?P<path>(?:[/~.]|\w)[\w./_~*?\[\]{}-]*))"#
```

### What it matches

Extracts the file path being edited in-place by `sed -i`. Write operation. The sed expression (between `-i` and the path) is skipped.

### Test cases

| Input | Extracted path |
|-------|---------------|
| `sed -i 's/foo/bar/' file.txt` | `file.txt` |
| `sed -i.bak 's/old/new/g' config.yml` | `config.yml` |
| `sed -i -e 's/a/b/' -e 's/c/d/' "my file.txt"` | `my file.txt` |
| `sed -i '' 's/x/y/' /etc/hosts` | `/etc/hosts` |

### Edge cases

- `-i.bak` creates a backup file (`config.yml.bak`) -- the backup path is derived
- Multiple `-e` expressions before the file path
- macOS `sed` requires `-i ''` (empty string backup suffix)
- `sed -i` without expression is an error but could still be in input

### Limitations

- Complex sed with multiple files: `sed -i 's/a/b/' file1 file2` only captures first file
- Does not handle `-f script-file` where the script is in a separate file
- Expression containing path-like strings could confuse the pattern

---

## Pattern 9: `chmod` / `chown` -- Permission/ownership change

### Regex

```rust
// chmod
r#"(?:^|[;&|]\s*)chmod\s+(?P<flags>(?:-[RfvcH]+\s+)*)(?:\+?[rwxXstugo0-7,]+)\s+(?:"(?P<path_q>[^"]+)"|'(?P<path_sq>[^']+)'|(?P<path>(?:[/~.]|\w)[\w./_~*?\[\]{}-]*))"#

// chown
r#"(?:^|[;&|]\s*)chown\s+(?P<flags>(?:-[RfvcHhLP]+\s+)*)(?:[\w.:-]+)\s+(?:"(?P<path_q>[^"]+)"|'(?P<path_sq>[^']+)'|(?P<path>(?:[/~.]|\w)[\w./_~*?\[\]{}-]*))"#
```

### What it matches

Extracts the file path after the mode/owner argument. The mode (`755`, `u+x`, `user:group`) is consumed but not captured. Write operation (metadata change).

### Test cases

| Input | Extracted path |
|-------|---------------|
| `chmod 755 script.sh` | `script.sh` |
| `chmod +x /usr/local/bin/app` | `/usr/local/bin/app` |
| `chmod -R 644 src/` | `src/` |
| `chown root:root /etc/config` | `/etc/config` |
| `chown -R user:group "my dir/"` | `my dir/` |

### Edge cases

- `chmod --reference=other file` -- different argument structure
- Numeric modes (`0755`) vs symbolic (`u+rwx,g+rx`)

### Limitations

- Does not handle `--reference` flag
- `chmod` with multiple paths: `chmod 644 a b c` captures only first

---

## Pattern 10: `git checkout -- <path>` -- Discard changes

### Regex

```rust
r#"(?:^|[;&|]\s*)git\s+checkout\s+(?:(?P<flags>(?:-[bBfqm]+\s+)*)(?:--\s+))(?:"(?P<path_q>[^"]+)"|'(?P<path_sq>[^']+)'|(?P<path>(?:[/~.]|\w)[\w./_~*?\[\]{}-]*))"#
```

### What it matches

Extracts file paths after `git checkout --`. The `--` separator distinguishes path arguments from branch names. This is a destructive write (discards uncommitted changes).

### Test cases

| Input | Extracted path |
|-------|---------------|
| `git checkout -- src/main.rs` | `src/main.rs` |
| `git checkout -- "path with spaces.txt"` | `path with spaces.txt` |
| `git checkout -- .` | `.` |
| `git checkout -- src/ tests/` | `src/` (first only) |

### Edge cases

- `git checkout HEAD -- file.txt` -- commit ref before `--`
- `git checkout .` (without `--`) -- ambiguous, could be branch or path
- `git restore --staged file.txt` -- modern equivalent, different command

### Limitations

- Does not handle `git checkout` without `--` (ambiguous branch vs path)
- `git checkout -p -- file.txt` (patch mode) not specifically handled

---

## Supplementary Patterns

### Pattern 11: `cat >` / Heredoc writes

```rust
r#"(?:^|[;&|]\s*)cat\s*>\s*(?:"(?P<path_q>[^"]+)"|'(?P<path_sq>[^']+)'|(?P<path>(?:[/~.]|\w)[\w./_~*?\[\]{}-]*))"#
```

Captures `cat > file` and `cat >> file` patterns used for file creation.

### Pattern 12: `ln` -- Create symlink

```rust
r#"(?:^|[;&|]\s*)ln\s+(?P<flags>(?:-[sfnrivTL]+\s+)*)(?:"(?P<src_q>[^"]+)"|'(?P<src_sq>[^']+)'|(?P<src>(?:[/~.]|\w)[\w./_~*?\[\]{}-]*))\s+(?:"(?P<dst_q>[^"]+)"|'(?P<dst_sq>[^']+)'|(?P<dst>(?:[/~.]|\w)[\w./_~*?\[\]{}-]*))"#
```

Both source and destination matter: the symlink destination is the write target.

### Pattern 13: `curl -o` / `wget -O` -- Download to file

```rust
// curl
r#"curl\s+.*?(?:-o|--output)\s+(?:"(?P<path_q>[^"]+)"|'(?P<path_sq>[^']+)'|(?P<path>(?:[/~.]|\w)[\w./_~*?\[\]{}-]*))"#

// wget
r#"wget\s+.*?(?:-O|--output-document)\s+(?:"(?P<path_q>[^"]+)"|'(?P<path_sq>[^']+)'|(?P<path>(?:[/~.]|\w)[\w./_~*?\[\]{}-]*))"#
```

Downloads write files; the output path should be policy-checked.

### Pattern 14: `dd` -- Disk copy (destructive)

```rust
r#"(?:^|[;&|]\s*)dd\s+.*?of=(?:"(?P<path_q>[^"]+)"|'(?P<path_sq>[^']+)'|(?P<path>[^\s;&|]+))"#
```

`dd of=/dev/sda` is extremely destructive; always check the `of=` path.

---

## Compound Command Handling

Real commands often chain multiple operations. The patterns above use `(?:^|[;&|]\s*)` as a prefix to match commands after `;`, `&&`, `||`, or at the start of the string.

### Example: Compound command

```bash
mkdir -p /tmp/build && cp -r src/ /tmp/build/ && rm -rf dist/
```

Running all patterns extracts:
1. `mkdir` -> `/tmp/build` (write)
2. `cp` -> `src/` (read), `/tmp/build/` (write)
3. `rm` -> `dist/` (destructive write)

### Subshell / Command substitution

```bash
rm $(find /tmp -name "*.log")
```

The `$()` content should be extracted and re-processed, but this is beyond simple regex. Mark `$(...)` as requiring deeper analysis or escalation.

---

## Implementation Strategy

### Extraction Pipeline

```rust
struct ExtractedPath {
    path: String,
    operation: Operation, // Read, Write, Delete, MetadataChange
    command: String,      // The originating command (rm, mv, etc.)
    confidence: f32,      // How confident we are this is actually a path
}

enum Operation {
    Read,
    Write,
    Delete,
    MetadataChange,
}
```

1. Split compound commands on unquoted `&&`, `||`, `;`, `|`
2. For each sub-command, try all patterns
3. Collect all `ExtractedPath` results
4. Evaluate each against the role's path policy
5. Most restrictive result wins (deny > ask > allow)

### Operation Classification

| Command | Operation |
|---------|-----------|
| `rm` | Delete |
| `mv` (source) | Delete |
| `mv` (destination) | Write |
| `cp` (source) | Read |
| `cp` (destination) | Write |
| `mkdir` | Write |
| `touch` | Write |
| `>` / `>>` | Write |
| `tee` | Write |
| `sed -i` | Write |
| `chmod` / `chown` | MetadataChange |
| `git checkout --` | Write (destructive) |
| `ln` (destination) | Write |
| `curl -o` / `wget -O` | Write |
| `dd of=` | Write (destructive) |
| `cat >` | Write |

### Confidence Scoring

| Scenario | Confidence |
|----------|-----------|
| Unquoted absolute path (`/foo/bar`) | 0.95 |
| Quoted path | 0.95 |
| Relative path (`./foo`, `../bar`) | 0.90 |
| Bare filename (`file.txt`) | 0.85 |
| Contains glob (`*.log`) | 0.80 |
| Contains variable (`$VAR`, `${VAR}`) | 0.50 |
| Contains command substitution (`$(...)`) | 0.30 |

### Things These Patterns Cannot Handle

1. **Variable expansion** -- `rm $FILE` extracts literal `$FILE`, not the value
2. **Command substitution** -- `rm $(find ...)` requires eval-like analysis
3. **Brace expansion** -- `rm file{1,2,3}.txt` is extracted as literal
4. **Aliases** -- `alias r='rm -rf'` then `r /tmp/` won't match
5. **Functions** -- `cleanup() { rm -rf "$1"; }; cleanup /tmp/build` requires call tracing
6. **Here-strings** -- `cmd <<< "data"` is not a file write
7. **Process substitution** -- `diff <(cmd1) <(cmd2)` creates temp files internally
8. **Pipe chains modifying files** -- `sort file -o file` writes to `file` via `-o`
9. **Python/Ruby/Node one-liners** -- `python -c "open('file','w')"` embeds file ops in other languages

For cases 1-3, extract the literal and mark confidence as low. For cases 4-9, these are beyond regex capability and should escalate to the LLM supervisor (Tier 3) for analysis.
