# Design decisions

This file records notable design decisions and their rationale as the project
evolves. Each entry should note the date, the decision, and the rationale.

## 2024 — Wire class encoding

Tags use `tag = (field_id << 3) | wire_class`, matching the varint-prefixed
tag scheme described in `spec.txt` §9. This keeps field IDs and wire classes
compact on the wire while remaining self-describing at decode time.
