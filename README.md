# F-Opener

**Because Created doesn't mean Ready.**

F-Opener is a file-watcher automation tool. It monitors folders for new files,
waits until they finish writing (stable size check), then launches a configured
action — no polling loops, no race conditions, no half-written files.

---

## Workspace crates

| Crate | Purpose |
|---|---|
| `fopener-core` | Domain types, matching logic, validation, events, errors |
| `fopener-config` | config.json load / save / migration / default paths |
| `fopener-watcher` | notify integration, debounce, wait-until-stable, events |
| `fopener-actions` | Placeholder rendering, process launch, ActionRunner trait |
| `fopener-cli` | CLI: `watch`, `validate-config`, `test-rule`, `open-once` |
| `fopener-gui-fltk` | FLTK GUI: rules table, edit dialog, log panel |

---

## Quick start

```bash
# Build everything
cargo build --workspace

# Validate a config file
cargo run -p fopener-cli -- validate-config --config config.json

# Test a rule against a filename
cargo run -p fopener-cli -- test-rule --config config.json \
  --rule "Open XML exports" --file export_123.xml

# Start watching
cargo run -p fopener-cli -- watch --config config.json

# Open one file immediately
cargo run -p fopener-cli -- open-once --config config.json --file /tmp/watch/export_123.xml

# Launch the GUI
cargo run -p fopener-gui-fltk
```

---

## How it works

1. A `WatchRule` defines a folder, file mask (glob), optional regex, and an action.
2. The watcher receives `Create`/`Modify` events from the OS via `notify`.
3. Each file is matched against rules. Default ignore patterns (`.tmp`, `.part`,
   `.crdownload`, `.download`, `~*`, `.swp`) are always filtered first.
4. Matched files optionally go through a **stability check**: the file's size is
   sampled repeatedly; once it stays constant for N checks the file is declared
   ready.
5. The action is launched with placeholder substitution:
   `{file}`, `{dir}`, `{filename}`, `{stem}`, `{ext}`, `{rule}`.

---

## Config format

```json
{
  "version": 1,
  "rules": [
    {
      "id": "unique-uuid",
      "name": "Open XML exports",
      "enabled": true,
      "path": "/tmp/watch",
      "include_subdirectories": false,
      "file_mask": "*.xml",
      "regex": "^export_.*\\.xml$",
      "action": {
        "executable": "xdg-open",
        "arguments": ["{file}"]
      },
      "debounce_ms": 1000,
      "wait_until_stable": true,
      "stable_check_ms": 300,
      "stable_checks_count": 3,
      "ready_timeout_sec": 30
    }
  ]
}
```

---

## Running tests

```bash
cargo test --workspace
```
