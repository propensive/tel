package tel

import soundness.*

// The two schema documents every conforming implementation carries built in, embedded so the
// registry can always preload them: the TELS meta-schema (the schema-for-schemas, matching
// `Tels.Axiom.tels`, TEL §20.5) and the `acceptance` schema (BinTEL §8.4). Each is a verbatim
// copy of the file of the same name at the root of the TEL repository; keep them in step (the
// registry refreshes its copies from these whenever they differ, so a stale copy never
// survives an upgrade). Kept as plain triple-quoted Strings (no interpolation/escapes) and
// converted to `Text`.
object MetaSchema:
  // `tels.tel`, published as `specification.tel/tels:2.0.0`.
  val source: Text = """tel 1.0

# The TEL schema-for-schemas, published as `specification.tel/tels:2.0.0`
# (the coordinate pinned in §8.1). Self-describing: parsing this document
# under the schema it defines yields a valid semantic model. The vocabulary
# and surface conventions are specified in §20.5 of the TEL Specification.
#
# Naming. Every Definition is a PascalCase TypeName; every variant or field
# keyword is kebab-case. The two grammars are disjoint by leading letter case
# (§20.7).
#
# Polarity. The default for every Field and SelectRef is required and
# irrepeatable. `optional`/`repeatable` (Flag fields) loosen on the base side;
# `required`/`irrepeatable` re-tighten on the layer side (§20 surface
# syntax).
#
# Type references. Every `field type` and `variant type` atom, and every
# SelectRef's first inline atom, is a TypeName that resolves through the
# composed Definition namespace to a record, scalar, select, or one of the
# five built-ins (`Flag`, `String`, `Identifier`, `Sigil`, `TypeName`).

name tels

# ---------------------------------------------------------------------------
# Record definitions
# ---------------------------------------------------------------------------

# A `field` declaration at a member position. The four loosen/tighten flags
# yield the per-axis Polarity (§20). `key` marks the identifying field of the
# enclosing struct (E219–E221; instance-level uniqueness is E314). It must
# precede `default` in member order so that a trailing `key` atom is consumed
# as a flag, not as a default value (§20.5). `default` is permitted only on
# required Scalar-typed fields (E203).

record Field
  description
      A field declaration at a member position.
  field keyword Identifier
  field type TypeName
  field optional Flag optional
  field required Flag optional
  field repeatable Flag optional
  field irrepeatable Flag optional
  field key Flag optional
  field default String optional
  field description String optional

# A `select` declaration at a member position — a SelectRef. The first inline
# atom names a top-level SelectDefinition; polarity lives at the use site.

record SelectRef
  description
      A select declaration at a member position, referencing a top-level SelectDefinition.
  field reference TypeName
  field optional Flag optional
  field required Flag optional
  field repeatable Flag optional
  field irrepeatable Flag optional

# A `variant` declaration inside a Select body.

record Variant
  description
      A variant declaration inside a Select body.
  field keyword Identifier
  field type TypeName
  field description String optional

# A `record` declaration: a named struct definition.

record Record
  description
      A record declaration: a named struct definition.
  field name TypeName
  select Member optional repeatable
  field description String optional

# A `scalar` declaration: a named scalar definition, optionally constrained
# by named validators and/or RE2 pattern constraints, with an optional
# binary encoding. A scalar with no constraints accepts every value, like
# the built-in `String`.

record Scalar
  description
      A scalar declaration: a named scalar definition constrained by validators and/or RE2 patterns, with an optional encoding.
  field name TypeName
  field validate Identifier optional repeatable
  field pattern String optional repeatable
  field encoding Identifier optional
  field description String optional

# A top-level `select` declaration. `exclude` children are permitted
# lexically but only valid inside a layer's select body; appearing in a
# base-schema select body raises E216 during construction.

record Select
  description
      A top-level select declaration: a named sum type.
  field name TypeName
  select SelectChild repeatable
  field description String optional

# The shared struct-shape used by `document` (Schema.document) and `overlay`
# (Layer.overlay): a sequence of member declarations.

record Body
  description
      The shared struct shape used by document and overlay.
  select Member optional repeatable

# A `layer` declaration: per-layer Definitions and an optional overlay.

record Layer
  description
      A layer declaration: per-layer definitions and an optional overlay.
  field name Identifier
  field record Record optional repeatable
  field scalar Scalar optional repeatable
  field select Select optional repeatable
  field overlay Body optional

# ---------------------------------------------------------------------------
# Select definitions
# ---------------------------------------------------------------------------

# Members admissible inside a struct-shaped Body, Record body, or Overlay.

select Member
  description
      Members admissible inside a struct-shaped body: a field, select, or validator.
  variant field Field
  variant select SelectRef
  variant validate Identifier

# Children admissible inside a Select body. `exclude` is layer-only (E216 in
# a base) but is lexically permitted here.

select SelectChild
  description
      Children admissible inside a Select body: a variant, exclude, or validator.
  variant variant Variant
  variant exclude Identifier
  variant validate Identifier

# ---------------------------------------------------------------------------
# Schema document root
# ---------------------------------------------------------------------------

document
  field name Identifier
  field sigil Sigil optional
  field record Record optional repeatable
  field scalar Scalar optional repeatable
  field select Select optional repeatable
  field document Body
  field layer Layer optional repeatable
""".tt

  // `acceptance.tel`, published as `specification.tel/acceptance:1.0.0`.
  val acceptance: Text = """tel 1.0

# The TEL acceptance schema, published as `specification.tel/acceptance:1.0.0`
# (the coordinate pinned in §8.4 of the BinTEL Specification). An acceptance
# tells a writer which composed schemas a reader can consume, in decreasing
# preference order, and which further components of each base's lineage it
# can resolve and would like included if the writer holds them.
#
# Every alternative is one `accept` line: its first atom is the schema
# signature (a palimpsest, BinTEL §8.2) the reader will use as its invocation
# schema; the two optional flags follow; every remaining atom names a further
# component by its hash or by a prefix of at least four bytes. The flag
# keywords contain a hyphen, which is not in the BASE-256 alphabet, so a flag
# can never be read as a component nor a component as a flag.
#
# Encodings. `schema-signature` and `base-256` are the two codecs BinTEL §8.4
# defines; an implementation that supports acceptances binds both.

name acceptance

# A schema signature: a palimpsest of component hashes at the BinTEL-pinned
# parameters. The codec rejects any byte sequence that is not structurally a
# signature (a length other than 33, 37, 39, 41, …, or a cadence byte other
# than 0x79), so a malformed signature is E312 at validation time.

scalar Signature
  description
      A schema signature (BinTEL §8.2): a palimpsest of component hashes at the pinned parameters.
  encoding schema-signature

# A component hash — a layer or an atom of the required base's lineage — or a
# prefix of one, at least four bytes long.

scalar Component
  description
      A component hash, or a prefix of one at least four bytes long.
  pattern .{4,32}
  encoding base-256

# One composition the reader accepts. Member order is load-bearing for inline
# atoms (TEL §20.2): `schema` is the first atom, the optional flags are skipped
# when the next atom does not match them, and `component` — a repeatable
# Scalar — takes every atom that remains.

record Alternative
  description
      One composition the reader accepts, with the further components it can resolve.
  field schema Signature
  field self-contained Flag optional
  field any-published Flag optional
  field component Component optional repeatable

document
  field accept Alternative repeatable
""".tt
