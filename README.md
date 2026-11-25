# ADO Code Review

A Rust workspace for migrating the Azure DevOps code review CLI. The current executable is a small
calculator that exercises command parsing, typed errors, and the same quality checks that will apply
to the review code. It does not contact Azure DevOps.

## Usage

```sh
cargo run --locked -- divide 7 2     # 3.5
cargo run --locked -- multiply -3 2  # -6
cargo run --locked -- --help
```

Use `add`, `subtract`, `multiply`, or `divide` with exactly two `f64` operands. Negative numbers are
accepted. The process exits with `0` on success, `1` for arithmetic or I/O errors, and `2` for invalid
CLI usage. Non-finite operands and results, including division by either signed zero, are rejected.

The executable delegates parsing and arithmetic to `ado_code_review::run`, which accepts an
argument iterator and output writer. Tests cover the arithmetic in isolation, call the library with
in-memory I/O, and run the real executable.

## Development and quality gates

Open the devcontainer, or install Rust with `rustup` on Linux and run `bash scripts/bootstrap`.
The bootstrap installs the pinned `just` version and runs `just setup`. Use `prek install` to
enable commit hooks when setting up outside the devcontainer.

```sh
just check      # run all local gates, stopping at the first failure
just check-all  # run every gate, even after a failure
just test       # unit, property, CLI integration, and doc tests
just deny       # check dependency policy and RustSec advisories
```

| Gate | Implementation |
|---|---|
| Format, types, dead code, unsafe code | rustfmt, rustc, and Clippy; see `Cargo.toml` and `clippy.toml` |
| Function length, arguments, nesting | Clippy: 40 code lines, 3 arguments including `self`, and nesting threshold 5 (which includes the enclosing `impl` and function body) |
| Source structure | `xtask`: physical file and `impl` lines; struct fields; public methods per type; statements, locals, exit points, branches, and boolean operands per function |
| Complexity and magic numbers | `xtask`: McCabe ≤8, nesting-weighted cognitive score ≤10, and named constants for numeric literals other than `0`, `1`, `-1`, `0.0`, and `1.0` |
| Duplication | jscpd: minimum 6 lines, zero duplication above the tokenizer's minimum token count |
| Coverage and timeout | cargo-llvm-cov: 90% line and region coverage across both packages; nextest: 2 seconds per test |
| Dependency hygiene | Cargo and cargo-shear for unused dependencies; cargo-deny for advisories, licenses, and sources |
| Config and workflows | Tombi for TOML, prek for YAML syntax, actionlint and zizmor for GitHub Actions |
| Secret scanning | Gitleaks via prek at commit time, including the Azure DevOps PAT rule |

Custom source thresholds and justified per-file exceptions are in `[workspace.metadata.quality]` in
`Cargo.toml`. The source checks inspect the application and `xtask` ASTs before macro expansion,
following `mod` declarations and `#[path]` overrides from crate roots. It skips test-only items,
files reached only through test-only modules, and `tests.rs`. Unlinked source files are still
checked. Public methods are counted per type within each crate target.
McCabe counts `&&`/`||` decisions but not `?`. Cognitive
complexity counts syntactically visible direct recursion; it does not resolve indirect call cycles.
Stable Rust region coverage is not branch coverage; doctests run separately. Nextest uses a process
per test instead of shuffling.
Enforced network isolation and architecture boundaries are deferred until those layers exist.

GitHub Actions runs the gates and builds the container on pull requests and pushes to `main`.
JUnit and coverage are archived in the run; dependency policy is also checked every Monday.

### Copilot code review

The [review instructions](.github/instructions/vendored-content.instructions.md) ask Copilot to
skip routine feedback on `.agents/skills/`, `.claude/skills/`, `skills-lock.json`, and
`.devcontainer/devcontainer-lock.json`, while still flagging clear security risks. This is review
guidance, not a hard exclusion. `.gitattributes` collapses those diffs by default on GitHub; it
does not control Copilot. Keep the two path lists in sync when adding vendored or generated paths.
