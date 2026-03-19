# Claude Code Guidelines

@README.md
@docs/SPECIFICATIONS.md

## Before Starting

Read `docs/ticket_format_specification.md` if working on markdown output format (directory layout, ticket.md structure).

## Project Layout

```
config.toml                              — runtime configuration (all fields required)
src/main.rs                              — entry point, file discovery, rayon parallel processing, summary
src/config.rs                            — TOML config parsing and validation
src/types.rs                             — all shared types (Ticket, Message, Attachment, Config, etc.)
src/pii/mod.rs                           — PII public API: build_name_matcher(), filter_pii(), re-exports
src/pii/redact.rs                        — string-level PII: regexes, hmac_tag(), redact_text(), all helpers
src/pii/json.rs                          — recursive JSON tree PII sanitization (athos-compatible)
src/pii/attachments.rs                   — text file detection + PII redaction for attachment files
src/pipeline/mod.rs                      — per-ticket processing orchestration
src/pipeline/load.rs                     — JSON deserialization into internal types
src/pipeline/filter.rs                   — ticket-level and message-level filtering rules
src/pipeline/normalize.rs                — message text cleaning (7-step pipeline)
src/pipeline/dedup.rs                    — consecutive duplicate message removal
src/pipeline/timeline.rs                 — merge messages + attachments into chronological timeline
src/pipeline/attachments.rs              — filename sanitization, uniqueness, file copying/symlinking, PII-aware writing
src/export/mod.rs                        — output format dispatch
src/export/markdown.rs                   — markdown rendering per ticket_format_specification.md
src/export/json.rs                       — JSON export with write-back, recursive PII, sorted keys
docs/SPECIFICATIONS.md                   — full implementation specification
docs/ticket_format_specification.md      — markdown output format (directory layout, ticket.md structure)
```

## Key Principles

- **No fallbacks**: missing or bad data is a hard error with a clear message naming the file and field. No silent defaults.
- **No serde derives on our types**: use `serde_json::Value` and `toml::Value` directly.
- **No logging framework**: `eprintln!` for errors, `println!` for summary output.
- **Crate-first**: use existing crates (rayon, chrono, regex, etc.), don't reinvent.
- **Exporters own their output**: directory layout, file naming, and reserved filenames are format-specific.

## Keeping Docs in Sync

When adding or changing features, update **all** of:
1. The code
2. `README.md` (How It Works section)
3. `docs/SPECIFICATIONS.md` (relevant section)
4. `docs/ticket_format_specification.md` (if output format rules change)

## Building and Running

- **Always use `--release`** for `cargo run` and `cargo build`. Debug mode is far too slow for processing thousands of tickets.
- Use `cargo check` for fast compilation checks (no need for `--release` there).
- Run `cargo clippy` after large changes and fix all warnings before committing.

```sh
cargo check            # fast compilation check
cargo clippy           # run after large changes; fix all warnings
cargo build --release  # build only
cargo run --release    # run against config.toml input_dir
```