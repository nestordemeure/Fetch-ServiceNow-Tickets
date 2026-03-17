# Claude Code Guidelines

## Before Starting

Always read these files first to understand the project:

1. `README.md` — what this does, how to use it, pipeline steps overview
2. `docs/SPECIFICATIONS.md` — full implementation spec (config, data model, pipeline details, error handling)
3. `docs/ticket_format_specification.md` — output format rules (markdown structure, normalization, filtering)

## Project Layout

```
config.toml                              — runtime configuration (all fields required)
src/main.rs                              — entry point, file discovery, rayon parallel processing, summary
src/config.rs                            — TOML config parsing and validation
src/types.rs                             — all shared types (Ticket, Message, Attachment, Config, etc.)
src/pipeline/mod.rs                      — per-ticket processing orchestration
src/pipeline/load.rs                     — JSON deserialization into internal types
src/pipeline/filter.rs                   — ticket-level and message-level filtering rules
src/pipeline/normalize.rs                — message text cleaning (7-step pipeline)
src/pipeline/dedup.rs                    — consecutive duplicate message removal
src/pipeline/timeline.rs                 — merge messages + attachments into chronological timeline
src/pipeline/attachments.rs              — filename sanitization, uniqueness, file copying
src/export/mod.rs                        — output format dispatch
src/export/markdown.rs                   — markdown rendering per ticket_format_specification.md
docs/SPECIFICATIONS.md                   — full implementation specification
docs/ticket_format_specification.md      — output format and normalization rules
```

## Key Principles

- **No fallbacks**: missing or bad data is a hard error with a clear message naming the file and field. No silent defaults.
- **No serde derives on our types**: use `serde_json::Value` and `toml::Value` directly.
- **No logging framework**: `eprintln!` for errors, `println!` for summary output.
- **Crate-first**: use existing crates (rayon, chrono, regex, etc.), don't reinvent.
- **Exporters own their output**: directory layout, file naming, and reserved filenames are format-specific.

## Keeping Docs in Sync

When adding or changing features, update **all three** of:
1. The code
2. `README.md` (How It Works section)
3. `docs/SPECIFICATIONS.md` (relevant section)
4. `docs/ticket_format_specification.md` (if normalization or filtering rules change)

## Testing

```sh
cargo check          # fast compilation check
cargo run --release  # run against config.toml input_dir
```