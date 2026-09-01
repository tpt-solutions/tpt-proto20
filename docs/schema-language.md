# Schema language reference

`tpt-proto20` schemas are written in `.tpt` files and compiled by
`tpt20-language` → `tpt20-compiler`. This reference documents the grammar as
implemented by the parser (`compiler/tpt20-language/src/parser.rs`).

> **Note on `spec.txt` §6:** the design document's illustrative examples use
> `method(Req) -> Resp;` for services and `@range(min: 0, max: 150)` for
> named annotation arguments. The implemented grammar uses `returns (...)`
> for methods and **positional** annotation arguments (`@range(0, 150)`).
> This document describes what the compiler actually accepts today; the
> differences are noted inline below.

## File structure

A `.tpt` file contains an optional `package` declaration, zero or more
`import` statements, and any number of top-level `message`, `enum`, and
`service` declarations.

```tpt
package user.v1;

import "common.tpt";

message User {
  1: id int64;
  2: name string;
  3: email string?;
  4: roles repeated Role;
  5: created_at Timestamp;
}

enum Role {
  UNKNOWN = 0;
  ADMIN = 1;
  MEMBER = 2;
  AUDITOR = 3;
}

message GetUserRequest {
  1: user_id int64;
}

service UserService {
  GetUser(GetUserRequest) returns (User);
  WatchUsers(WatchUsersRequest) returns (stream User);
}
```

Package names conventionally end in a version segment (`user.v1`) — see
[Schema evolution](#schema-evolution).

## Fields

The field grammar is:

```text
<field_id>: <name> [repeated] <Type> [?] [@annotation(...) ...];
```

- `<field_id>` is a positive integer. It is the wire tag and, once published,
  must never be reused for a different field (spec §6.2).
- `<name>` comes **before** the type.
- `repeated` (if present) comes between the name and the type.
- A trailing `?` marks explicit presence.
- Annotations may appear before the field ID or after the presence marker;
  both positions attach to the same field.

```tpt
1: id int64;                      // implicit presence
2: age int32?;                    // explicit presence
3: tags repeated string;          // repeated
4: username string @max_len(32);  // annotated
```

### Presence

- **Implicit presence** (no `?`): absence on the wire is indistinguishable
  from the type's default value (`0`, `""`, `false`, empty list/map).
- **Explicit presence** (`?`): absence is distinguishable from the default;
  generated code represents the field as `Option<T>`.
- Message-typed fields are always presence-aware in generated code.
- **`required` fields are not part of the language.** The parser rejects the
  `required` keyword with `ParseError::RequiredNotAllowed`.

### Repeated fields

```tpt
1: tags repeated string;
```

Repeated fields decode from both packed and unpacked wire encodings; see
[Wire format § Repeated fields](wire-format.md#repeated-fields).

### Maps

```tpt
1: labels map<string, string>;
```

Map keys must be a scalar or `string` type. Maps are encoded as repeated
synthetic entry messages on the wire (see [Wire format § Maps](wire-format.md#maps)).

### Oneofs

```tpt
message Contact {
  oneof method {
    1: email string;
    2: phone string;
    3: device_token string;
  }
}
```

Oneof members are mutually exclusive; generated code represents a oneof as a
Rust enum (e.g. `ContactMethod::Email(String)`).

## Enums

```tpt
enum Status {
  UNKNOWN = 0;
  ACTIVE = 1;
  SUSPENDED = 2;
}
```

- Every value has a stable numeric ID.
- `open` / `closed` modifiers may appear before or after the enum name
  (`open enum Feature { ... }` or `enum Feature open { ... }`); closed is the
  default. Closed enums reject unknown wire values on decode
  (`DecodeError::InvalidEnumValue`); open enums are expected to preserve them.
- A value may be marked as an alias of a previous numeric ID with the
  `alias` keyword: `DEFAULT alias = 0;`.
- Enums may be declared at the top level or nested inside a `message` (as in
  the example schema below).

Enums, like messages, may be nested inside a message body:

```tpt
message Outer {
  enum Status {
    ACTIVE = 0;
    INACTIVE = 1;
  }
  1: status Status;

  open enum Feature {
    NONE = 0;
    BETA = 1;
  }
  2: feature Feature;
}
```

## Services

```text
<MethodName>([stream] <RequestType>) returns ([stream] <ResponseType>);
```

`stream` may appear on the request type, the response type, both, or
neither, giving the four RPC shapes:

```tpt
service UserService {
  GetUser(GetUserRequest) returns (User);                    // unary
  WatchUsers(WatchUsersRequest) returns (stream User);        // server streaming
  UploadLogs(stream LogEntry) returns (UploadSummary);        // client streaming
  Chat(stream ChatMessage) returns (stream ChatMessage);      // bidirectional
}
```

See [RPC model](rpc-model.md) for how these map onto generated server/client
code and the transport layer.

## Reserved IDs and names

```tpt
message User {
  reserved 3, 5..7, "old_field_name";
  1: id int64;
  2: name string;
}
```

- Numeric IDs may be listed individually or as a range, spelled either
  `5..7` / `5..=7` or `5 to 7`.
- Quoted strings reserve a field *name* so it cannot be reintroduced with a
  different ID.
- Reserving a removed field ID is how the compatibility checker (see below)
  is told a removal was intentional, turning what would otherwise be a
  `BREAKING` change into a `SAFE` one.

## Annotations

Annotations attach metadata to fields, enums, services, and messages using
`@name(args)`. Arguments are **positional** (string, integer, or bare
identifier/boolean literals) — there is no `key: value` argument syntax.

```tpt
message CreateUserRequest {
  1: email string? @max_len(254);
  2: age int32? @range(0, 150);
  3: username string @pattern("^[a-z0-9_]{3,32}$");
}
```

Core annotations recognized by the Rust code generator's builder validation
(`--builders`, see [Code generation](code-generation.md)):

| Annotation | Applies to | Effect |
|---|---|---|
| `@max_len(n)` | `string`/`bytes` fields | Builder rejects values longer than `n`. |
| `@min_len(n)` | `string`/`bytes` fields | Builder rejects values shorter than `n`. |
| `@range(min, max)` | integer fields | Builder rejects values outside `[min, max]`. |
| `@pattern("regex")` | `string` fields | Documented; not yet enforced by generated builders. |
| `@deprecated` | any declaration | Documented marker; the linter's `DeprecatedUsage` rule flags files containing it. |

Annotation names beyond this core set are accepted by the parser (any
identifier is a valid annotation name) but have no compiler- or
codegen-defined behavior — this is the "custom annotation" extension point
described in spec §6.9.

## Schema evolution

The compiler's compatibility checker (`tpt20 diff`, see
[CLI reference](cli-reference.md#diff)) classifies every change between two
schema files as one of:

| Class | Examples |
|---|---|
| `SAFE` | Add an optional/repeated field, add a new value to an open enum, add a new service method, reserve a removed field ID |
| `WARNING` | Rename a field while keeping its ID, rename a method, change documentation/annotation semantics |
| `BREAKING` | Change a field's type, change a field's ID, remove a field without reserving it, remove a method without a compatibility policy, change a method's streaming direction, change a request/response type incompatibly |

```sh
tpt20 diff schema/v1/user.tpt schema/v2/user.tpt
```

```text
SAFE     added field 6 created_at
WARNING  renamed field username to login
BREAKING removed field 3 without reservation
```

Every compiled schema also has a stable **schema fingerprint**, derived from
the canonical descriptor, exposed via `compiled.fingerprint` and printed by
`tpt20 gen rust` (as a comment) and `tpt20 registry publish`. It is stable
across semantically-identical re-serializations and is used for registry
storage, dynamic-message validation, and debugging.

## Diagnostics

Compiler errors (`tpt20 check`) report file, line, column, span, severity, an
error code, a human-readable explanation, and — where possible — a suggested
fix:

```text
error[E0042]: field ID 3 was removed without reservation
  --> schema/user.v1.tpt:12:3
  |
  = help: reserve field ID 3 to preserve compatibility
```
