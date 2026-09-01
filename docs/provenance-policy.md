# Provenance policy

`tpt-proto20` is developed as a clean-room, independent implementation
(spec §25). This document is the canonical summary of that policy; the
underlying files it summarizes are [`CONTRIBUTING.md`](../CONTRIBUTING.md)
and [`provenance/`](../provenance/) at the repository root — this page
exists to satisfy the documentation acceptance criterion in spec §27.9, not
to replace those files as the source of truth.

## Why this exists

An infrastructure project that aims to interoperate with an existing
ecosystem (here, Protocol Buffers and gRPC — see
[Compatibility adapters](compatibility-adapters.md)) runs a real risk of
accidentally incorporating that ecosystem's implementation code, licenses,
or design artifacts. The provenance policy exists to keep the project's
implementation independent and its licensing (`MIT OR Apache-2.0`) clean, so
that neither the project nor its users inherit obligations from a upstream
codebase.

## Clean-room policy (spec §25.1)

### Allowed inputs

- The project's own design document, [`spec.txt`](../spec.txt)
- Original design notes authored by project contributors
- Original tests and independently created test vectors
- Publicly documented wire-format and schema conventions (e.g. the general
  concept of a tag-length-value binary format, or gRPC's public HTTP/2
  framing conventions) as described in public specifications

### Disallowed inputs

- Upstream implementation source code for comparable serialization/RPC
  systems (e.g. reading another project's codec source while implementing
  this one's)
- Proprietary or third-party implementations of similar systems
- Copied code-generation templates from other projects
- AI prompts (or their outputs) that themselves contain upstream source code

If it's unclear whether a given input is permitted, the rule from
`CONTRIBUTING.md` is to ask a maintainer before using it — the disallowed
list above is illustrative, not exhaustive.

## AI-assisted contribution policy (spec §25.2)

AI tooling may assist contributions, subject to:

1. **Human review** — every AI-assisted change is reviewed by a human
   contributor who understands the affected subsystem.
2. **Testing** — AI-assisted changes must include or update tests and pass
   CI (`cargo fmt --check`, `cargo clippy --all-targets`, `cargo test`).
3. **Documentation** — public APIs and non-obvious behavior must be
   documented (this documentation set is itself subject to that rule).
4. **Similarity checks** — contributions must not reproduce upstream code;
   maintainers may run similarity checks against known implementations.
5. **Provenance recording** — the origin and assistance method of a
   contribution should be recorded in [`provenance/`](../provenance/) where
   appropriate.

## Where records live

| File | Purpose |
|---|---|
| [`provenance/README.md`](../provenance/README.md) | Index of the provenance directory and how it relates to `CONTRIBUTING.md` / `spec.txt` §25 |
| [`provenance/allowed_inputs.md`](../provenance/allowed_inputs.md) | The permitted/disallowed input lists, restated for quick reference |
| [`provenance/decisions.md`](../provenance/decisions.md) | Dated design-decision log with rationale (e.g. the wire tag scheme's `(field_id << 3) \| wire_class` layout and why it was chosen) |

When you make a notable design decision, add an entry to
`provenance/decisions.md` with the date, the decision, and its rationale —
this is what lets a future contributor (or auditor) understand *why* the
system works the way it does without having to reverse-engineer intent from
code alone.

## Contribution model

Per `CONTRIBUTING.md`, this project **does not accept pull requests**; use
GitHub Issues to report bugs, request features, or propose changes. The
policies above still bind any code changes maintainers make in response.
Development commands:

```sh
cargo build
cargo test
cargo fmt --check
cargo clippy --all-targets
```

## Licensing

The project is dual-licensed `MIT OR Apache-2.0`
([`LICENSE-MIT`](../LICENSE-MIT), [`LICENSE-APACHE`](../LICENSE-APACHE)),
with copyright recorded in [`COPYRIGHT`](../COPYRIGHT). The clean-room policy
above is what makes that licensing meaningful — it asserts the code is
TPT Solutions' own work, not a derivative of a differently-licensed upstream
project.
