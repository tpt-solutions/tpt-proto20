# tpt20 Standard Library Stability Policy

## 1. Purpose

This document defines the stability guarantees and versioning policy for the
`tpt20-stdlib` crate (spec §15). The standard library is the set of well-known
message types shipped with every `tpt20` installation.

## 2. Scope

The policy applies to all public types in `tpt20-stdlib`, including:

- message types (`Timestamp`, `Duration`, `Empty`, `Any`, `Struct`, `Value`,
  `ListValue`, `FieldMask`, `Uuid`, `Decimal`, `Money`, `Interval`,
  `Pagination`, `ErrorDetail`)
- wrapper types (`BoolValue`, `BytesValue`, `DoubleValue`, `FloatValue`,
  `Int32Value`, `Int64Value`, `UInt32Value`, `UInt64Value`, `StringValue`)
- enums (`NullValue`)

## 3. Semantic Versioning

The `tpt20-stdlib` crate follows semantic versioning (semver):

- **MAJOR** version increments when breaking changes are introduced.
- **MINOR** version increments when new types or optional fields are added
  in a backward-compatible manner.
- **PATCH** version increments for bug fixes that do not change the wire
  format or public API.

## 4. Wire Contract Stability

Field IDs within standard library messages are part of the wire contract.

### 4.1 Safe changes (minor or patch)

- Adding new optional fields (new field IDs).
- Adding new wrapper types.
- Adding new convenience constructors or methods on existing types.
- Deprecating existing fields (with a documented migration path).

### 4.2 Breaking changes (major)

- Removing or repurposing existing field IDs.
- Changing the type of an existing field.
- Changing the wire class (varint / fixed / length-delimited) of an existing
  field.
- Removing public types.
- Changing the semantics of existing fields.

## 5. Rust API Stability

Public Rust types and their fields are stable. The following are considered
breaking API changes:

- Renaming public structs or enums.
- Removing public fields.
- Changing field types.
- Changing generic parameters.
- Removing public methods.

### 5.1 Non-breaking additions

- Adding new public methods.
- Adding new trait implementations.
- Adding new optional fields to structs (when the struct is used as a schema
  message, this must still follow §4.1).

## 6. Schema Stability

The `.tpt` schemas in `src/schema/` are the canonical source of truth for
code generation. Schema changes follow the same rules as the wire contract:

- New optional fields are safe.
- Existing field IDs must not change.
- Field names may change only if the field ID remains the same (this is a
  WARNING change per spec §20.3).

## 7. Deprecation Policy

When a field or type is deprecated:

1. The field or type remains functional for at least **two** major releases.
2. A `@deprecated` annotation or equivalent documentation is added.
3. A migration path to the replacement is documented.
4. Removal requires a major version bump.

## 8. Compatibility with Generated Code

Code generated from schemas that reference standard library types must
continue to compile and interoperate with the corresponding `tpt20-stdlib`
versions across minor releases.

## 9. Experimental Features

Types or fields marked as experimental in documentation are exempt from the
stability guarantees above and may change or be removed in any release.

## 10. Policy Review

This policy is reviewed with each major release. Proposed changes are
discussed and documented in the project changelog.
