# Versioning, stability and compatibility policy

Spec §24. This page states what `tpt-proto20` promises and when. Until
**1.0** everything below is *intended* policy; pre-1.0 releases (`0.x`) may
break any of it in a minor version, and each break is recorded in the
affected crate's `CHANGELOG.md`.

## Semantic versioning

The project uses [SemVer](https://semver.org). All workspace crates share one
version (`[workspace.package] version`). From 1.0: patch = fixes only, minor =
backward-compatible additions, major = breaking changes to any surface below.
`0.x`: the minor number plays the role of the major.

The minimum supported Rust version is declared in the workspace
(`rust-version`); raising it is a minor-version change (after 1.0), noted in
the changelog.

## Schema package versioning

Schemas should carry a version segment in the package name
(`package user.v1;`). A new major schema version (`user.v2`) is the mechanism
for breaking contract changes; within `v1` only compatible changes are
allowed, which `tpt20 diff` classifies as SAFE / WARNING / BREAKING
(spec §21.4):

- SAFE: add a field, enum value, message, service, or method; deprecate.
- BREAKING: remove or renumber a field without reserving it, change a field's
  type or wire class, change `repeated`/map shape, remove an enum value or
  method, change method streaming shape.
- Field IDs and reserved IDs/names are never reused.

## Stability by surface

| Surface | Promise (from 1.0) |
|---|---|
| **Native wire format** | Frozen: tag = `(field_id << 3) \| wire_class`, the four wire classes, canonical-encoding rules (spec §9). A change must be backward-compatible (old decoders still read new messages that do not use the new feature) **or** be gated behind a new protocol version that is negotiated explicitly; it is never silently changed. Decoders keep accepting both packed and unpacked repeated fields. |
| **Descriptor format** | The binary form starts with a 4-byte magic that carries the format version (currently `TPD1`); the JSON form follows the IR serde model. A change bumps the magic (`TPD2`, …); readers reject unknown magics instead of guessing, and a release that introduces a new version keeps reading the previous one for at least one major version. Fingerprints (`tpt20_ir::fingerprint`) change only with a format-version bump. |
| **Generated code** | Generated *public items* (message structs and their fields, `encode`/`decode`/`decode_borrowed`/`to_json`/`from_json`, enums, oneof enums, builders, service traits/clients/servers) are API: they follow SemVer with the runtime crates they depend on. Items marked `#[doc(hidden)]` (`unknown_fields`, `__support`) and the internal structure of the emitted file are not. Regenerating with a newer `tpt20 gen rust` may add items; it will not remove or change the meaning of existing ones within a major version. Flattened nested-type names (`Outer_Inner`) are part of the contract. |
| **Public Rust APIs** | Items exported from the runtime/compiler crates' roots are SemVer-stable from 1.0. `pub(crate)` items, `#[doc(hidden)]` items and test/bench crates (`tests/`, `benches/`, `fuzz/`, `tpt20-conformance`) are excluded. |
| **RPC conventions** | Method path `<package>.<Service>/<Method>`; status in `grpc-status` / `grpc-message` trailers; deadline in `grpc-timeout`; binary metadata keys end in `-bin` (base64). Changing these is a breaking change with a protocol-version gate. |
| **CLI output** | Exit codes (`0` ok, `1` failure, `2` usage) and the documented flags are stable. Machine-readable outputs (`--format json`, descriptor JSON/binary, text format, `registry list` columns as documented) are stable; human-oriented messages, diagnostics wording and table spacing are not. Removing or renaming a command/flag, or changing a stable output, is a breaking change (major). |
| **Registry** | The on-disk layout (`<root>/<version>/descriptor.json`, `manifest.json` with `version`, `fingerprint`, `policy`, `published_at`) is stable; published versions are immutable, and `get` verifies the fingerprint. New manifest fields are additive. |

## Deprecation

Deprecations are announced in a minor release (`#[deprecated]` with a note,
CLI warning on stderr, changelog entry) and removed no earlier than the next
major release. Schema-level deprecation uses the `@deprecated` annotation.

## Security fixes

Security-relevant changes (e.g. tightening `DecoderLimits` defaults) may ship
in a patch release even when they change behavior for inputs that exceeded the
documented limits.
