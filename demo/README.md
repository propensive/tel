# TEL examples

Worked examples of TEL schemas, conforming documents, BinTEL encodings, validators, and
machine operations.

| File | Purpose |
| --- | --- |
| [`walkthrough.md`](walkthrough.md) | A single TEL document shown side-by-side with its presentation model, semantic model, and BinTEL byte sequence. The smallest end-to-end example. |
| [`mutations.md`](mutations.md) | A sequence of §22 machine operations applied to a small document, showing how each operation preserves comments, remarks, and column alignment. |
| [`contact-schema.tel`](contact-schema.tel) | A schema for personal contact records. Demonstrates required/optional fields, repeatable fields, named `record` definitions referenced from multiple positions by `TypeName`, a `select` whose variants are all `Flag`, and a `default` value on a required scalar. |
| [`contact-document.tel`](contact-document.tel) | A document conforming to `contact-schema.tel`. Uses hard-space mode for multi-token Scalar values (full name, address fields), aligned for readability. |
| [`contact-layered-schema.tel`](contact-layered-schema.tel) | A layered schema (base + six layers) demonstrating the §20.3 permitted operations: *Add a Field*, *Refine a RecordDefinition in place*, *Add a SelectRef*, and *Exclude a variant*. |
| [`contact-layered-composed.tel`](contact-layered-composed.tel) | The same schema written without layers, for byte-equivalence comparison. |
| [`contact-layered-document.tel`](contact-layered-document.tel) | A document conforming to the composed layered schema. |
| [`struct-validator-schema.tel`](struct-validator-schema.tel) | A schema with a struct-level validator (`start-precedes-end`) demonstrating cross-field validation per §21.6. |
| [`struct-validator-document.tel`](struct-validator-document.tel) | A document triggering the struct validator's failure path; see `struct_validator_worked_example` in `ref/tel/src/lib.rs` for the runnable companion test. |
| [`atom-forms-schema.tel`](atom-forms-schema.tel) | A schema for a document carrying three Scalar values, one in each atom form (inline, source, literal). |
| [`atom-forms-document.tel`](atom-forms-document.tel) | A document showing the three atom forms in use, including a literal atom with a `#`-prefixed line and a source atom carrying embedded JSON. |
| [`encoding-schema.tel`](encoding-schema.tel) | A schema whose scalars declare `encoding` codecs (§21.7), giving hex digests and decimal sizes compact binary representations in BinTEL. |
| [`document-stream.tel`](document-stream.tel) | Three independent documents in one source, separated by `##` document separators, demonstrating the streams of §6.1. |
| [`tels.bintel.hex`](tels.bintel.hex) | The BinTEL document root encoding of `/tels.tel`, used to recompute the normative value hash pinned in §20.5 of the TEL Specification. |
| [`tels.hash`](tels.hash) | The BLAKE3-256 and BASE-256 forms of the `tels.tel` value hash. |
| [`acceptance-request.tel`](acceptance-request.tel) | An acceptance (§8.4 of the BinTEL Specification): a reader of the layered contact schema asks for the base with the `with-address` layer, plus the `with-phone` and `with-status` layers by 4-byte hash prefix if the writer has them, and falls back to the base alone in self-contained mode. Its bare BinTEL form is 92 bytes. |
| [`acceptance.bintel.hex`](acceptance.bintel.hex) | The BinTEL document root encoding of `/acceptance.tel`, used to recompute the value hash pinned in §8.4 of the BinTEL Specification. |
| [`acceptance.hash`](acceptance.hash) | The BLAKE3-256 and BASE-256 forms of the `acceptance.tel` value hash. |

## Validation

Several examples exercise the validator model defined in §21 of the TEL Specification.

- **Scalar validators** (§21.1) attach to Scalar fields and inspect each value's text. The
  four built-in scalar validators — `string`, `identifier`, `sigil`, and `type-name` — are
  required by every conforming TEL parser, because TELS itself uses them via the built-in
  `TypeName`s `String`, `Identifier`, `Sigil` and `TypeName` (§21.5); everything else is
  application-defined. Multiple scalar validators on the same Field apply in AND-conjunction.
- **Scalar encodings** (§21.7) attach a codec to a scalar Definition via an `encoding`
  line. The codec's encoder doubles as a validator (a value it rejects is E312-invalid)
  and defines the scalar's binary representation in BinTEL (§7.1 of the BinTEL
  Specification). `encoding-schema.tel` is the worked example.
- **Struct validators** (§21.6) attach to Definitions and inline Struct types, and inspect
  the entire struct element. They are the natural place to express cross-field constraints
  ("postcode is required when country is UK", "start date must precede end date").
  `struct-validator-schema.tel` and `struct-validator-document.tel` are the worked example.
- **Diagnostic shape**: an `Invalid` response carries a recursive `Diagnostic` — `Scalar`
  diagnostics may include a `span` pointing into the value text; `Struct` diagnostics may
  include a `fields` map keyed by child keyword, recursively descending to point at any
  nested scalar's specific span. See §21.2 of the spec.

Every schema in this directory is itself a TEL document validated against the **TELS** schema
(`/tels.tel`). Every schema here parses cleanly, type-checks
against TELS with zero errors, and round-trips through schema construction.

Documents reference their schemas by **LIRA coordinate** in the pragma (§8.1 of the TEL
Specification) — `domain/module-name`, optionally with a `:version` or `:tag` selector. The
documents here carry *bare* references (no selector), which are local-only by design: they
resolve against the local schema cache and never trigger network resolution (§8.2, resolution
step 4). Portability is always explicit — a real deployment adds a selector
(`example.org/contact:1.0.0`) or carries the BASE-256-encoded schema signature (§8 of the
BinTEL Specification), which identifies the composed schema by content.
