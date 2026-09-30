# Project governance

`tpt-proto20` is maintained by **TPT Solutions**.

- **Contributions are issues, not pull requests.** Bugs, feature requests and
  design discussion go through GitHub Issues (see `CONTRIBUTING.md`).
  Maintainers implement accepted changes.
- **Decisions.** Maintainers decide; notable design decisions and their
  rationale are appended to [`provenance/decisions.md`](../provenance/decisions.md).
  Scope is governed by [`spec.txt`](../spec.txt); changes to it are made by
  maintainers in a commit that explains why.
- **Clean-room and AI-assisted work** follow the [provenance
  policy](provenance-policy.md): allowed/disallowed inputs, human review,
  tests, CI, and documentation for every AI-assisted change.
- **Quality gate.** `master` must pass the CI pipeline (`cargo fmt --check`,
  `cargo clippy --all-targets --all-features`, `cargo build --all-targets
  --all-features`, `cargo test --all-features`, all with `-D warnings`).
- **Releases** follow [Versioning and stability](stability.md) and
  [the release checklist](release-checklist.md).
- **Security reports** should be filed as a GitHub issue asking for a private
  channel (do not include exploit details in the public issue).
- **Conduct.** Be respectful and constructive; maintainers may moderate or
  close issues that are not.
