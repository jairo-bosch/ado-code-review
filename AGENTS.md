# Agent instructions

| Task | Command |
|---|---|
| Format | `just fmt` |
| Run a test by name | `cargo nextest run --workspace --locked -E 'test(name)'` |
| Run all local quality gates | `just check` |
| Diagnose multiple gate failures | `just check-all` |

- Keep the executable limited to process I/O and exit codes; put CLI behavior in the library.
- Commit `Cargo.lock` with dependency changes.
- Fix gate failures in code. Ask before adding an exception or `#[expect]`, or raising a limit.
- Keep secret scanning in the commit hook, not the CI workflow.
- Write code, diagnostics, docs, and commit subjects in English.

For setup, quality gate semantics, or CI changes, read [Development and quality gates](README.md#development-and-quality-gates).
