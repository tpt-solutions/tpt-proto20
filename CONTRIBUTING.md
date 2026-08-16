# Contributing to tpt-proto20

Thank you for your interest in contributing to `tpt-proto20`.

This project is developed under a **clean-room** process and an
**AI-assisted contribution policy**. Both are described below. By contributing,
you agree to follow these policies.

## 1. Clean-room policy (spec §25.1)

The `tpt-proto20` implementation is intended to be an independent,
clean-room design. To preserve that status:

### Allowed inputs

- Publicly published specifications (e.g. the project's own `spec.txt`).
- Original design notes and tests created by project contributors.
- Independently created test vectors and conformance fixtures.
- Public knowledge of wire-format conventions documented in `spec.txt`.

### Disallowed inputs

- Upstream implementation source code for comparable systems.
- Proprietary or third-party implementations of similar systems.
- Copied code-generation templates taken from other projects.
- AI prompts (or their outputs) that themselves contain upstream source code.

If you are unsure whether a particular input is permitted, ask a maintainer
before using it.

## 2. AI-assisted contribution policy (spec §25.2)

AI tooling may be used to assist contributions, subject to these constraints:

- **Human review.** Every AI-assisted change must be reviewed by a human
  contributor who understands the affected subsystem.
- **Testing.** AI-assisted changes must include or update tests, and must pass
  the CI pipeline (`fmt --check`, `clippy`, `test`).
- **Documentation.** Public APIs and non-obvious behavior must be documented.
- **Similarity checks.** Contributions must not reproduce upstream code;
  maintainers may run similarity checks.
- **Provenance recording.** The origin and assistance method of each
  contribution should be recorded in `provenance/`.

## 3. Development setup

```sh
cargo build
cargo test
cargo fmt --check
cargo clippy --all-targets
```

## 4. Code of conduct

Be respectful and constructive. Governance details are tracked toward
Phase 20 of the project todo.
