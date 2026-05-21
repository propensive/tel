# TEL examples

Worked examples of TEL schemas and documents conforming to them.

| File | Purpose |
| --- | --- |
| `contact-schema.tel` | A schema for personal contact records. Demonstrates required/optional fields, repeatable fields, named `define`d structs referenced from multiple positions, a `select` with all-`flag` variants, and a `default` value on a required scalar. |
| `contact-document.tel` | A document conforming to `contact-schema.tel`. |

Each schema is a TEL document validated against the **tel-schema** (the
schema-for-schemas, `/tel-schema.tel` at the repo root). The schema document
parses cleanly, type-checks against tel-schema with zero errors, and round-trips
through schema construction.

The `contact-document.tel` references its schema by URL. In a real deployment
the identifier would either resolve over the network or be replaced with the
hex-encoded BinTEL signature of the schema (§8.1 of the TEL Specification + §8
of the BinTEL Specification).
