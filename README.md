# ADO Code Review

A Rust workspace for reviewing Azure DevOps pull requests. The `review` command collects PR context,
sends it to the configured Copilot service, and posts a summary and inline findings. Use `--dry-run`
to preview the incremental summary and inline comments on stdout without making Azure DevOps writes.

## Usage

```sh
ado-code-review review --repo-url https://dev.azure.com/org/project/_git/repo --pr 42 --dry-run
```

Set `AZURE_TOKEN` and `COPILOT_GITHUB_TOKEN` in the environment. For GitHub Enterprise Cloud with
data residency, set `COPILOT_GH_HOST` to the bare host. Only run a review on code you may share
with that Copilot service. Azure DevOps requests verify TLS using the machine's trust store;
`SSL_CERT_FILE` can select an explicit CA bundle.

If the token identity already recorded a review for the current PR iteration, the command skips
the Copilot call. Use `--force` to review the same iteration again. Publishing rechecks the PR
before posting, never votes, and leaves human-handled finding threads untouched. Exit codes are
`0` for success or a skipped review, `1` for runtime errors, and `2` for invalid CLI usage.

## Azure Pipelines

The [steps template](templates/review.yml) runs only for pull-request builds. It needs a Linux
x86_64 agent with glibc 2.28 or newer for the Copilot runtime, plus Bash, `curl`, `tar`,
`sha256sum`, `flock`, and Python 3. Store `COPILOT_GITHUB_TOKEN` as a secret pipeline variable.
The default Azure token is `System.AccessToken`; grant its build identity read and comment access
to the PR. An alternate Azure token can be passed by secret variable reference.

Mirror the pinned template into an Azure Repos repository in the same collection, then reference
the mirror by tag:

```yaml
resources:
  repositories:
    - repository: reviewTools
      type: git
      name: MyProject/review-tools
      ref: refs/tags/v0.0.1
steps:
  - template: templates/review.yml@reviewTools
    parameters:
      effort: balanced
      exclude: ['docs/**']
```

GitHub tag pushes (`vX.Y.Z`) build the static binary and publish its archive and SHA-256 sidecar.
After the release is available, `just pin-release vX.Y.Z` pins and commits the version and checksum
to the template. Mirror that commit and tag it in Azure Repos before using the matching template tag.

## Development and quality gates

Open the devcontainer, or install Rust with `rustup` on Linux and run `bash scripts/bootstrap`.
The bootstrap installs the pinned `just` version and runs `just setup`. Use `prek install` to
enable commit hooks when setting up outside the devcontainer.
The isolated review integration tests also require `python3` and `openssl`.

```sh
just check      # run all local gates, stopping at the first failure
just check-all  # run every gate, even after a failure
just test       # unit, CLI integration, and doc tests
just deny       # check dependency policy and RustSec advisories
```

For an opt-in smoke test against the configured Copilot service, set `COPILOT_GITHUB_TOKEN` and run
`cargo test --locked --test review_cli previews_synthetic_pr_with_live_copilot -- --ignored`.
It sends only synthetic PR content and does not contact an Azure DevOps instance.

| Gate | Implementation |
|---|---|
| Format, types, dead code, unsafe code | rustfmt, rustc, and Clippy; see `Cargo.toml` and `clippy.toml` |
| Function length, arguments, nesting | Clippy: 40 code lines, 3 arguments including `self`, and nesting threshold 5 (which includes the enclosing `impl` and function body) |
| Source structure | `xtask`: physical file and `impl` lines; struct fields; public methods per type; statements, locals, exit points, branches, and boolean operands per function |
| Complexity and magic numbers | `xtask`: McCabe ≤8, nesting-weighted cognitive score ≤10, and named constants for numeric literals other than `0`, `1`, `-1`, `0.0`, and `1.0` |
| Duplication | jscpd: minimum 6 lines, zero duplication above the tokenizer's minimum token count |
| Coverage and timeout | cargo-llvm-cov: 90% line and region coverage across both packages; nextest: 2 seconds per test |
| Dependency hygiene | Cargo and cargo-shear for unused dependencies; cargo-deny for advisories, licenses, and sources |
| Copilot runtime pin | `xtask` checks the resolved SDK's runtime manifest; runtime installation verifies the adapter's pinned release archive |
| Config and workflows | Tombi for TOML, prek for YAML syntax, actionlint and zizmor for GitHub Actions |
| Secret scanning | Gitleaks via prek at commit time, including legacy and current Azure DevOps PATs |

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

GitHub Actions runs the gates on pull requests and pushes to `main`.
JUnit and coverage are archived in the run; dependency policy is also checked every Monday.

### Copilot code review

The [review instructions](.github/instructions/vendored-content.instructions.md) ask Copilot to
skip routine feedback on `.agents/skills/`, `.claude/skills/`, `skills-lock.json`, and
`.devcontainer/devcontainer-lock.json`, while still flagging clear security risks. This is review
guidance, not a hard exclusion. `.gitattributes` collapses those diffs by default on GitHub; it
does not control Copilot. Keep the two path lists in sync when adding vendored or generated paths.
