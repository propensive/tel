# TEL examples

Worked examples of TEL schemas, conforming documents, BinTEL encodings, validators, and
machine operations.

| File | Purpose |
| --- | --- |
| [`walkthrough.md`](walkthrough.md) | A single TEL document shown side-by-side with its presentation model, semantic model, and BinTEL byte sequence. The smallest end-to-end example. |
| [`mutations.md`](mutations.md) | A sequence of §22 machine operations applied to a small document, showing how each operation preserves comments, remarks, and column alignment. |
| [`contact-schema.tel`](contact-schema.tel) | A schema for personal contact records. Demonstrates required/optional fields, repeatable fields, named `define`d structs referenced from multiple positions via `type`, a `select` with all-`flag` variants, and a `default` value on a required scalar. |
| [`contact-document.tel`](contact-document.tel) | A document conforming to `contact-schema.tel`. Uses hard-space mode for multi-token Scalar values (full name, address fields), aligned for readability. |
| [`contact-layered-schema.tel`](contact-layered-schema.tel) | A layered schema (base + six layers) demonstrating §20.3 layer composition: Field-append, Definition-merge, Select-add, and variant-exclude. |
| [`contact-layered-composed.tel`](contact-layered-composed.tel) | The same schema written without layers, for byte-equivalence comparison. |
| [`contact-layered-document.tel`](contact-layered-document.tel) | A document conforming to the composed layered schema. |
| [`struct-validator-schema.tel`](struct-validator-schema.tel) | A schema with a struct-level validator (`start-precedes-end`) demonstrating cross-field validation per §21.6. |
| [`struct-validator-document.tel`](struct-validator-document.tel) | A document triggering the struct validator's failure path; see `tests::struct_validator_worked_example` for the runnable companion test. |
| [`atom-forms-schema.tel`](atom-forms-schema.tel) | A schema for a document carrying three Scalar values, one in each atom form (inline, source, literal). |
| [`atom-forms-document.tel`](atom-forms-document.tel) | A document showing the three atom forms in use, including a literal atom with a `#`-prefixed line and a source atom carrying embedded JSON. |
| [`tel-schema.bintel.hex`](tel-schema.bintel.hex) | The BinTEL document root encoding of `/tel-schema.tel`, used to recompute the normative value hash pinned in §20.5 of the TEL Specification. |
| [`tel-schema.hash`](tel-schema.hash) | The SHA-256 and BASE-256 forms of the `tel-schema.tel` value hash. |

## Validation

Several examples exercise the validator model defined in §21 of the TEL Specification.

- **Scalar validators** (§21.1) attach to Scalar fields and inspect each value's text. The
  three built-in scalar validators — `identifier`, `sigil`, and `string` — are required
  by every conforming TEL parser; everything else is application-defined. Multiple scalar
  validators on the same Field apply in AND-conjunction.
- **Struct validators** (§21.6) attach to Definitions and inline Struct types, and inspect
  the entire struct element. They are the natural place to express cross-field constraints
  ("postcode is required when country is UK", "start date must precede end date").
  `struct-validator-schema.tel` and `struct-validator-document.tel` are the worked example.
- **Diagnostic shape**: an `Invalid` response carries a recursive `Diagnostic` — `Scalar`
  diagnostics may include a `span` pointing into the value text; `Struct` diagnostics may
  include a `fields` map keyed by child keyword, recursively descending to point at any
  nested scalar's specific span. See §21.2 of the spec.

Every schema in this directory is itself a TEL document validated against the **tel-schema**
(the schema-for-schemas, `/tel-schema.tel`). Every schema here parses cleanly, type-checks
against tel-schema with zero errors, and round-trips through schema construction.

Documents reference their schemas by URL in the pragma. In a real deployment the identifier
would either resolve over the network or be replaced by the BASE-256-encoded BinTEL signature
of the schema (§8.1 of the TEL Specification + §8 of the BinTEL Specification).
