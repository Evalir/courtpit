# Decisions beyond the architecture doc

Choices made where `docs/architecture.md` is silent. Newest at the bottom. Each entry: what,
why, and the PR that introduced it.

| # | Decision | Why | PR |
|---|---|---|---|
| 1 | Runtime-checked sqlx queries (`query_as::<_, T>`) instead of the compile-time macros the doc mentions. | `.sqlx` offline data conflicts on every rebase of a stacked PR; integration tests against real Postgres give the same safety. | 01 |
| 2 | Errors render as `{ "error": { "code", "message" } }` (`application/json`) with the matching HTTP status; `code` is a stable snake_case identifier. | One shape for every failure; clients switch on `code`. | 01 |
| 3 | IDs are UUID v7 generated in Rust. | Time-ordered (good index locality, usable as pagination cursor); PG16 has no native v7. | 01 |
| 4 | Single integration test binary (`tests/it`) with modules. | One link step instead of one per file. | 01 |
