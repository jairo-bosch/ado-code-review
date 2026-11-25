set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

binstall_version := "1.28.0"
nextest_version := "0.9.152"
llvm_cov_version := "0.9.2"
deny_version := "0.22.0"
shear_version := "1.14.1"
prek_version := "0.5.11"
zizmor_version := "1.33.1"
tombi_version := "1.11.3"
jscpd_version := "5.4.3"
actionlint_version := "1.7.12"
gates := "format lint source duplication deps toml-format toml-lint hooks coverage doctests"

default: check

setup:
    #!/usr/bin/env bash
    set -euo pipefail
    export PATH="${CARGO_HOME:-"$HOME/.cargo"}/bin:$PATH"
    arch=$(uname -m)
    case "$(uname -s):$arch" in
        Linux:x86_64)
            amd=amd64; js=x64
            binstall_hash=6de9f8fef45a831a7a31e8829e8d73479a67cb6007964ac0b2aa651eebb1eefd
            tombi_hash=36672279d05f093455f01fa5a2799a8cc2162a9195cbbb23f0f8d1107d0de497
            jscpd_hash=291452d6005e3fa17b531552bf76f87ff85bc4ae31533e0c17a7997f84079868
            actionlint_hash=8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8 ;;
        Linux:aarch64)
            amd=arm64; js=arm64
            binstall_hash=b70ef4098ca891c042028cd1c9ce80128c7a35d4c5dc64d55f4ec5d963c45e69
            tombi_hash=083614471a69e15f147ba68c96fe1e3ea02e8efe38d6bcdb4606b5a37ab8758a
            jscpd_hash=f363875348817f8e999ca090266b1265ba29322d6b5d1f6280aa0d136ab1ca9e
            actionlint_hash=325e971b6ba9bfa504672e29be93c24981eeb1c07576d730e9f7c8805afff0c6 ;;
        *) printf 'Tool setup supports Linux x86_64 and aarch64\n' >&2; exit 1 ;;
    esac
    bash scripts/install-release cargo-bins/cargo-binstall v{{binstall_version}} \
        "cargo-binstall-$arch-unknown-linux-gnu.tgz" cargo-binstall "$binstall_hash"
    cargo binstall --no-confirm --locked \
        cargo-nextest@{{nextest_version}} cargo-llvm-cov@{{llvm_cov_version}} \
        cargo-deny@{{deny_version}} cargo-shear@{{shear_version}} prek@{{prek_version}} zizmor@{{zizmor_version}}
    bash scripts/install-release tombi-toml/tombi v{{tombi_version}} \
        "tombi-cli-{{tombi_version}}-$arch-unknown-linux-musl.tar.gz" tombi "$tombi_hash"
    bash scripts/install-release kucherenko/jscpd v{{jscpd_version}} \
        "jscpd-linux-$js-musl.tar.gz" jscpd "$jscpd_hash"
    bash scripts/install-release rhysd/actionlint v{{actionlint_version}} \
        "actionlint_{{actionlint_version}}_linux_$amd.tar.gz" actionlint "$actionlint_hash"
    cargo fetch --locked

fmt:
    cargo fmt --all
    tombi format

fix:
    cargo clippy --fix --allow-dirty --workspace --all-targets --locked

test:
    cargo nextest run --workspace --all-targets --locked
    cargo test --workspace --doc --locked

deny:
    cargo deny --locked check

check: (_check "")

check-all: (_check "yes")

_check keep_going:
    @status=0; for gate in {{gates}}; do \
        just "check-$gate" || { [[ -n "{{keep_going}}" ]] || exit 1; status=1; }; \
        done; exit "$status"

check-format:
    cargo fmt --all --check

check-lint:
    cargo clippy --workspace --all-targets --locked

check-source:
    cargo run --locked --quiet -p xtask -- source

check-duplication:
    limits=$(cargo run --locked --quiet -p xtask -- duplication); read -r lines threshold <<< "$limits"; \
        jscpd --min-lines "$lines" --threshold "$threshold" --ignore "**/tests.rs" --fail-on-empty src xtask/src

check-deps:
    cargo shear --deny-warnings

check-toml-format:
    tombi format --check

check-toml-lint:
    tombi lint --error-on-warnings

check-hooks:
    SKIP=cargo-fmt,cargo-clippy,gitleaks prek run --all-files

check-coverage:
    mkdir -p target/reports
    cargo llvm-cov clean --workspace
    minimum=$(cargo run --locked --quiet -p xtask -- coverage); \
        cargo llvm-cov nextest --workspace --all-targets --locked --profile ci --no-report; \
        cargo llvm-cov report --workspace --lcov --output-path target/reports/lcov.info; \
        cargo llvm-cov report --workspace --html --output-dir target/reports; \
        cargo llvm-cov report --workspace --fail-under-lines "$minimum" --fail-under-regions "$minimum" | tee target/reports/coverage.txt

check-doctests:
    cargo test --workspace --doc --locked
