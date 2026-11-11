# RaTEL Specification Draft

## Abstract

RaTEL (**random-access TEL**) is the third physical encoding of the semantic model of a TEL
document, as defined by the [TEL Specification](tel.md), alongside the text form and
[BinTEL](bintel.md). Where text TEL is for people and BinTEL is for streams, RaTEL is for data
that is **read in place**: a file memory-mapped by one or more processes, in which looking up one
record touches that record and nothing else.

BinTEL is count-driven: a struct stores how many children it has, integers are varints, and a
repeated field is the same keyword index recurring among the siblings (BinTEL §4, §7). A whole
document can be skipped at constant cost, but reaching the *n*-th child of a struct, or any field
of any struct, means walking every predecessor with the schema in hand. RaTEL is offset-driven:
every struct type has a fixed-width **row** with one **slot** per member at a schema-determined
offset; instances of a type form a contiguous **table**; variable-width values live in separate
regions addressed by offset; and sorted **indexes** over keyed and declared members make lookup a
binary search over mapped memory. Field access is one load at a known offset; nothing is walked.

RaTEL shares with BinTEL everything that is not layout: the semantic model (TEL §18), schemas and
schema layers, the schema signature and the **value hash** (BinTEL §3, §8), the keyword-index
numbering of a struct's members (BinTEL §5), the codec contract for encoded scalars (TEL §21.7),
and self-contained carriage of the composed schema (BinTEL §6.2). A RaTEL file always carries its
schema. Conversion among text TEL, BinTEL and RaTEL is lossless (§12). The storage-layout
annotations a schema may carry — `width`, `intern` and `index` (TEL §20) — are read by RaTEL
and by nothing else; the fixed-width, order-preserving standard codecs on which RaTEL's inline
slots and type-blind index keys rely are defined in the companion
[Standard Codecs Specification](codecs.md).

RaTEL is defined as a byte sequence to be read in place. It has no textual form.

## 1. Status

This document is a draft specification of RaTEL. Version 1 describes **immutable snapshots**: a
publisher writes a whole file and renames it into place. The header reserves what a later
log-structured variant needs (§5.1, §15), but no append semantics are defined here.

## 2. Conformance Language

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHALL**, **SHALL NOT**, **SHOULD**, **SHOULD
NOT**, **RECOMMENDED**, **MAY**, and **OPTIONAL** in this document are to be interpreted as
described in RFC 2119 and RFC 8174 when, and only when, they appear in all capitals.

## 3. Integers, Regions, Tables and Terms

**Structural integers.** Every integer that RaTEL itself defines — header fields, directory
entries, slots, offsets, counts, tags — is an unsigned, fixed-width, **little-endian** integer,
written `u8`, `u16`, `u32` or `u64`, stored at an offset that is a multiple of its width. There
is one exception, stated once here and again where it applies (§9.3): the record indices inside
the bytes of an index entry are **big-endian**, because index entries are compared bytewise and
their order must agree with numeric order. BinTEL's variable-length integers (BinTEL §4) appear
in a RaTEL file only inside the schema section (§5.2), which is a BinTEL fragment.

**Codec bytes are opaque.** The bytes of an encoded scalar (TEL §21.7) are copied into a RaTEL
file verbatim and are never loaded by RaTEL as integers; they are handed to the bound codec, or
to an application that knows the codec. The standard codecs ([Standard Codecs](codecs.md)) are
big-endian so that their bytes compare bytewise in numeric order; that is a property of the codec,
not of RaTEL, and it does not conflict with the little-endian rule above.

**Alignment.** `round4(n)` and `round8(n)` denote `n` rounded up to the next multiple of 4 or 8.
Every section begins at an offset that is a multiple of 8 and has a length that is a multiple of
8; every row begins at a multiple of 8; every item in a region (§5.3, §5.4) begins at a multiple
of 4. Pad bytes MUST be zero when written and MUST be ignored when read.

**Region.** A byte range of at most `2^32` bytes within which a `u32` **ref** is a byte offset
from the region's start. RaTEL has two regions: the string table (§5.3) and the variable region
(§5.4). A ref always denotes the first byte of an **item** — a `u32` header followed by the item's
bytes.

**Table, row, record index.** A **table** holds the instances of one Struct type as `count`
**rows** of `rowSize` bytes each; row `r` (the **record index**, `0 ≤ r < count`) begins at
`tableOffset + r·rowSize`. **Table 0** holds the document root and has exactly one row.

**Slot.** A fixed-width field of a row, at a schema-determined offset, holding a member's value
or a reference to it (§6).

**Flattening, host row, site.** A required, non-repeatable Struct-typed member is **flattened**:
its slots are laid into its parent's row rather than into a row of its own (§6.1). A member's
**host row** is the row that holds its slot, which may be the row of an enclosing struct several
flattening steps up. A **site** is a member position identified by its host table and its
flattened member path (§6.2); the same RecordDefinition reached from two sites has two sites but
one table.

**Composed schema.** Throughout, *the schema* means the composed schema (TEL §20.3) that the
file's schema section names, and *member order* and *keyword order* are those of TEL §20.

## 4. Document Model

A RaTEL file encodes **exactly one** TEL semantic model (TEL §18.2) under one composed schema,
exactly as a BinTEL document does (BinTEL §6). The root is a virtual Struct whose member order is
that of `Schema.document`; it is stored as the single row of table 0. Any **collection** —
records looked up by key, iterated in order, or scanned — is simply an effectively `repeatable`
member of some struct, most often of the root; RaTEL gives it a table (§5.5) and, when its type
has a key field, a primary index (§9.1).

As in BinTEL, a required Scalar member supplied by its `default` is a value like any other and
is stored (BinTEL §7.6); an explicitly empty text and a defaulted empty text are the same value
(BinTEL §7.5); and an invalid document (any E3xx condition of TEL §21.9) is never encoded — a
RaTEL writer MUST fail rather than emit one.

Unlike BinTEL, RaTEL is **not byte-deterministic**: two conforming writers MAY lay out the same
semantic model differently (the order of items in a region, the deduplication of strings, the
placement of sections). Properties P3 and P4 of TEL §22.4 therefore do not apply to RaTEL bytes.
What RaTEL guarantees is **semantic losslessness**, property P5 (§12): materialising a RaTEL file
reproduces the semantic model exactly, and a document's identity is its value hash (BinTEL §3),
computed over its BinTEL encoding, which a RaTEL file MAY carry in its header.

## 5. File Layout

A RaTEL file is a header (§5.1) followed by six **sections** in any order, each located by the
header's directory. Readers locate sections through the directory only; the order in which a
writer lays them out is its own choice, with the recommendation of §15 (tables before indexes).

### 5.1 Header

The header occupies bytes `0..144` of the file:

| Offset | Size | Field |
| -----: | ---: | ----- |
| 0 | 4 | **Magic number**: the bytes `B1 C4 B5 BB`. In the BASE-256 alphabet ([BASE-256](base256.md) §4) these are `ᾱ` (`U+1FB1`), `τ`, `ε`, `λ` — `ᾱτελ`, with the alpha of *access* beside BinTEL's `βτελ`. Every byte is `≥ 0x80`, so the file cannot be mistaken for text. Any other value is **R01**. |
| 4 | 1 | **Version**: `0x01`. Any other value is **R02**. |
| 5 | 1 | Reserved; MUST be `0x00` (**R03**). |
| 6 | 2 | **Flags** (`u16`): bit 0 — a digests section is present (§5.7); bit 1 — the value hash field holds the document's value hash. All other bits MUST be clear (**R03**). |
| 8 | 8 | **Committed length** (`u64`): the number of bytes of the file a reader may use. MUST be `≥ 144`, a multiple of 8, and `≤` the file's actual length (**R04**). Bytes at or beyond the committed length MUST NOT be read. |
| 16 | 96 | **Section directory**: six entries of `(offset: u64, length: u64)`, in the fixed order *schema*, *strings*, *variable*, *tables*, *indexes*, *digests*. |
| 112 | 32 | **Value hash**: the BLAKE3-256 value hash of the document (BinTEL §3) when flag bit 1 is set; otherwise 32 zero bytes. |

Directory constraints, each **R05** when violated: every `offset` is a multiple of 8 and
`≥ 144`; every `length` is a multiple of 8; `offset + length ≤ committed length` (computed
without overflow). Non-empty sections MUST NOT overlap (**R06**). A section MAY have `length = 0`
— the strings, variable, indexes and digests sections are empty when the document needs nothing
of them — and a zero-length section does not overlap anything, whatever its offset. Bytes below
the committed length that belong to no section are padding.

The committed length is the word a later appending writer would advance after adding rows; a
version-1 reader treats it simply as the extent of the file.

### 5.2 Schema Section

The schema section carries the composed schema in the form of BinTEL §6.2 fields 3 and 4,
verbatim:

1. the byte length of the schema signature (BinTEL integer, §4 there) and the signature bytes —
   a palimpsest at the BinTEL-pinned parameters, whose length MUST be `33` or `37 + 2·(n − 2)`,
   `n ≥ 2`, and whose bytes MUST XOR to `0x79` (**R08**; the B03 conditions);
2. the byte length of the schema body (BinTEL integer) and the body — the bare document-root
   encoding of the schema document under `tels` (TEL §20.5), with the root member list taken to
   be `tels.document.members`;
3. zero padding to the section length.

A reader decodes the body under its built-in `tels` axiom (**R09** if it does not decode; the B12
condition), composes the schema per TEL §20.3 — the body MAY contain `layer` compounds — and
recomputes the composed signature per BinTEL §8, which MUST equal the carried signature
byte-for-byte (**R10**; the B11 condition). A reader MUST NOT interpret any table, region or
index before R10 has passed. Having composed the schema, the reader derives the **layout** of
every table (§6.2); an unresolvable `Reference` or `SelectRef` (the B10 condition), a scalar
marked `intern` whose width is fixed, or a secondary-index path that does not resolve under §9.1
means the schema cannot be laid out, and is **R11** — a configuration error of the schema, as
B10 is, not of the file's data.

The embedded schema is always present: RaTEL has no external-schema mode. A file is read by many
processes over a long life, and the one thing every reader must agree on is the schema; carrying
it costs a few hundred bytes once. A reader that already holds the schema identified by the
signature MAY skip decoding the body, as BinTEL §6.2 permits, provided it has previously verified
that schema's signature.

### 5.3 String Table

The string table is a region (§3) of **items**, each:

```
u32 length          byte length of the value
u8  bytes[length]   the value's bytes: UTF-8 for a text scalar, codec bytes for an encoded one
    pad             zero bytes to the next multiple of 4
```

It holds the values of every scalar whose composed ScalarDefinition carries `intern` (TEL §20).
A writer SHOULD store each distinct value once, so that two equal values have equal refs and an
application can compare interned values by comparing refs; a reader MUST NOT rely on this — two
distinct refs MAY hold equal bytes — and MUST compare bytes when equality matters.

### 5.4 Variable Region

The variable region is a region of items with the same grammar as §5.3 (`u32` header, bytes, pad
to 4), holding everything that has no fixed width and is not interned. The header's meaning
depends on the item's **kind**, which the schema determines from the slot that refers to the item;
no tag is stored:

| Kind | Header | Body |
| ---- | ------ | ---- |
| *value item* | `length` | the bytes of one variable-width scalar value |
| *packed array* | `count` | `count` values of a fixed-width scalar, each exactly `w` bytes, contiguous |
| *ref array* | `count` | `count` × `u32` refs, each to a value item (in the string table if the scalar is `intern`, else in this region) |
| *select list* | `count` | `count` × `(u32 tag, u32 payload)` pairs, as §6.1 defines for a `SELECT` slot |

An item's extent — header, then body as computed from the header — MUST lie within the region
(**R14**), and a ref MUST be a multiple of 4 (**R14**). A reader checks the header's extent before
reading the header, and the body's extent before reading the body.

### 5.5 Tables Section

The tables section begins with a **table directory** of `N = 1 + |records|` entries, where
`records` is the composed schema's RecordDefinition list in composed-namespace order (TEL §20.3:
the base schema's records in declaration order, then each layer's new records in layer order).
Table 0 is the document root; table `i + 1` is `records[i]`. Every RecordDefinition has a table,
whether or not any instance of it exists un-flattened — an unused table has `count = 0` and
costs its directory entry alone. Both sides compute this numbering from the schema, so no
reachability analysis is needed and none is permitted to differ.

Each directory entry is 24 bytes:

```
u64 offset        from the start of the tables section; a multiple of 8
u32 count         number of rows
u32 rowSize       bytes per row; MUST equal the layout's rowSize for this table (§6.2)
u8  layout        0 = row-major; every other value is reserved
u8  reserved[7]   MUST be zero
```

Violations are **R12**: `rowSize` differing from the computed value, an unknown `layout` byte,
non-zero reserved bytes, a misaligned `offset`, or `offset + count·rowSize` (computed in 64 bits)
exceeding the section length. Table 0 MUST have `count = 1` (**R13**).

Rows follow the directory at the offsets it gives. Row storage is **row-major**: row `r` of a
table is the `rowSize` bytes at `offset + r·rowSize`, laid out per §6. The `layout` byte is
reserved so that a column-major table — the same slots stored as one array per slot, for scans
over one field across many rows — can be registered without a format break.

### 5.6 Indexes Section

The indexes section begins with an **index directory** of `M` entries, where `M` is the number
of indexes the schema determines (§9.1). Each entry is 16 bytes:

```
u64 offset        from the start of the indexes section; a multiple of 8
u32 count         number of entries in the index
u8  kind          0 = sorted entries (§9); every other value is reserved
u8  reserved[3]   MUST be zero
```

At `offset` lies `u32 offsets[count + 1]`, followed immediately — at
`offset + 4·(count + 1)` — by the **entry region**, a run of concatenated entries. `offsets[i]` is
the position of entry `i` relative to the start of the entry region, and `offsets[count]` is the
position one past the last entry, so that entry `i` occupies
`entryRegion[offsets[i] .. offsets[i + 1])` with no special case. The `offsets` MUST be
non-decreasing and `offset + 4·(count + 1) + offsets[count]` MUST not exceed the section length;
an unknown `kind`, a directory whose entry count differs from `M`, non-zero reserved bytes, or any
violation of these constraints is **R23**.

### 5.7 Digests Section

When flag bit 0 is set, the digests section holds `7 × 32` bytes: the BLAKE3-256 digest of each
of the first five sections (schema, strings, variable, tables, indexes), in directory order, each
computed over exactly the `length` bytes the directory gives for that section (an empty section
digests the empty input), followed by the digest of header bytes `0..144`. A reader that verifies
digests MUST report a mismatch as **R07**. When flag bit 0 is clear, the digests section's length
MUST be `0` (**R07** otherwise).

Digest verification is OPTIONAL and a reader MAY defer it, but a reader that advertises
integrity checking MUST complete it before trusting any section's contents. Digests detect
accidental corruption; they are not an authenticity mechanism (§11).

## 6. Slots and Row Layout

### 6.1 Slot Kinds

Each member `m` of a Struct `S`, with its type resolved through References (TEL §20.2), has a
**slot kind** determined by the member's shape, its effective polarity, and — for Struct-typed
Fields — the **flattening stack**, the set of RecordDefinition names already being flattened into
the current row (§6.2). *Fixed-width* means the scalar's `encoding` is a standard codec of fixed
width ([Standard Codecs](codecs.md) §9), or its composed ScalarDefinition declares `width w`
(TEL §20); any other scalar, including every text scalar, is *variable-width*. `a(w)` denotes the
largest power of two not exceeding `min(w, 8)`.

| Member | Kind | Size | Align | Presence bit |
| ------ | ---- | ---: | ----: | :----------: |
| Field, `Flag`, required, non-repeatable | `NONE` — no storage; the flag is always present | 0 | — | no |
| Field, `Flag`, optional, non-repeatable | `FLAG` — presence bit only | 0 | — | yes |
| Field, `Flag`, repeatable | `FLAG_COUNT` — `u32` number of occurrences | 4 | 4 | no |
| Field, fixed-width Scalar of width `w`, non-repeatable | `INLINE(w)` — the `w` value bytes | `w` | `a(w)` | iff optional |
| Field, variable-width Scalar, non-repeatable | `REF` — `u32` ref to a value item (string table if `intern`, else variable region) | 4 | 4 | iff optional |
| Field, fixed-width Scalar, repeatable | `ARRAY_REF` — `u32` ref to a packed array | 4 | 4 | no |
| Field, variable-width Scalar, repeatable | `ARRAY_REF` — `u32` ref to a ref array | 4 | 4 | no |
| Field, Struct `R`, required, non-repeatable, `R` not on the stack | `FLATTEN(R)` — `R`'s slots laid into this row (§6.2) | — | — | no |
| Field, Struct `R`, required, non-repeatable, `R` on the stack | `RECORD(R)` — `u32` record index into `R`'s table | 4 | 4 | no |
| Field, Struct `R`, optional, non-repeatable | `RECORD(R)` — `u32` record index | 4 | 4 | yes |
| Field, Struct `R`, repeatable | `RANGE(R)` — `u32 start`, `u32 count`: rows `start .. start + count` of `R`'s table, the children in semantic order | 8 | 4 | no |
| SelectRef, non-repeatable | `SELECT` — `u32 tag`, `u32 payload` | 8 | 4 | iff optional |
| SelectRef, repeatable | `SELECT_LIST_REF` — `u32` ref to a select list | 4 | 4 | no |

The *presence bit* column says whether the slot has a bit in the row's presence bitmap (§6.2).
Only optional, non-repeatable members have one: a required member is always present, and a
repeatable member's emptiness is its count of zero.

**Select tag and payload.** `tag` is the **keyword index** (BinTEL §5) of the chosen variant in
the keyword order of the Struct that declares the SelectRef — the same number the semantic model
records (TEL §18.2) and BinTEL encodes. It MUST fall within the range of keyword indices that the
member's variants occupy (**R17**). `payload` depends on the variant's resolved type: a Struct `R`
→ a record index into `R`'s table (variant instances are never flattened); a Scalar → a ref to a
value item holding the value's bytes (in the string table if that scalar is `intern`, otherwise in
the variable region — fixed-width scalars included, for uniformity); `Flag` → `0`.

**Width and intern.** `width w` is a promise that every value of the scalar encodes to exactly
`w` bytes; a writer MUST refuse to encode a document containing a value whose byte length differs
(**R22**) — it is a configuration failure of the writer, as an unresolved encoding is (TEL §21.7),
never a padding instruction and never a document error. `intern` on a fixed-width scalar has no
meaning and is **R11**. Neither annotation changes the semantic model, the value hash, or the
BinTEL encoding.

### 6.2 Layout Algorithm

A table's layout is a pure function of the schema. Every reader and writer computes it by the
procedure below and therefore agrees on every slot offset and row size without any of them being
stored.

```
layout-document(schema):
  tables[0] = layout-struct(schema.document, {})
  for i, R in enumerate(records-in-composed-namespace-order(schema)):
    tables[i + 1] = layout-struct(R.struct, {R.name})
  return tables

layout-struct(S, stack):
  slots = []                                  // built in SLOT ORDER
  walk(S.members, path = [], stack)
  presence = the slots whose presence-bit column is "yes", numbered 0, 1, … in slot order
  bitmapBytes = ceil(|presence| / 8)
  off = bitmapBytes
  for align in [8, 4, 2, 1]:                  // each pass stable in slot order
    for s in slots where s.align == align and s.size > 0:
      off = round-up(off, align)
      s.offset = off
      off += s.size
  return Layout { slots, bitmapBytes, rowSize: round8(off) }

walk(members, path, stack):
  for i, m in enumerate(members):             // MEMBER order (TEL §20), not keyword order
    kind = classify(m, stack)                 // §6.1
    if kind is FLATTEN(R):
      walk(R.struct.members, path ++ [i], stack ∪ {R.name})
    else:
      slots.push(Slot { path: path ++ [i], kind, size, align, presence })
```

Points to note:

- Members are visited in **member order**, under which a SelectRef is one member with one slot;
  keyword order, which expands a SelectRef into one entry per variant, is used only for tags.
- **Slot order** is the order in which `walk` pushes slots — a pre-order traversal with flattened
  structs expanded in place — and is the order in which §8 materialises and §9.1 enumerates
  sites. A slot's **path** (the member indices from the host struct down) identifies the member it
  stores; the stack prevents a required self-reference from flattening without bound. No finite
  valid document can populate a `RECORD` slot produced by the stack rule (such a type is
  unsatisfiable), but a reader MUST still bounds-check it like any other.
- The **presence bitmap** occupies row bytes `0 .. bitmapBytes`; bit `b` is at byte `b >> 3`,
  bit `b & 7` (least significant bit first). Slots are then packed by descending alignment, which
  wastes at most 7 bytes per row to padding.
- `rowSize` is a multiple of 8, so every row of every table is 8-aligned when the table is.

### 6.3 Absence, Emptiness and Defaults

- An **absent** optional member has its presence bit clear. Its slot bytes MUST be zero when
  written and MUST be ignored when read; a verifying reader MAY report non-zero bytes there, or in
  any pad, as **R26**.
- An **empty** repeatable member has a count of `0`: a `RANGE` with `count = 0` (its `start` is
  then irrelevant and SHOULD be `0`), a `FLAG_COUNT` of `0`, or a ref to an array or list item
  whose header is `0`. Absence (presence bit) and emptiness (count) are distinct and never
  conflated: a repeatable member is never absent, an optional non-repeatable member is never
  empty.
- An **empty text** is a value item of `length = 0`. BinTEL §7.5 applies verbatim: the encoding
  does not distinguish an explicitly empty value from a defaulted empty one, and for an encoded
  scalar an empty item is well-formed only if the codec accepts the empty byte sequence.
- A **defaulted** required Scalar (TEL §20, BinTEL §7.6) is stored as its default text's bytes,
  exactly like a written value.

## 7. Reading

### 7.1 Opening

```
open(bytes):
  if len(bytes) < 144: report R04
  header = copy of bytes[0 .. 144]            // a mapped file may change beneath the reader (§11)
  check magic (R01), version (R02), reserved byte and flag bits (R03)
  check committed length (R04); check every directory entry (R05); check overlap (R06)
  if flags.digests: verify §5.7 (R07)         // MAY be deferred; MUST precede trust
  read the schema section: signature (R08), body (R09), recomputed signature (R10)
  schema = compose(body)                      // TEL §20.3
  L = layout-document(schema)                 // R11 if the schema cannot be laid out
  check the table directory against L (R12); check tables[0].count == 1 (R13)
  check the index directory: M entries (§9.1), each well-formed (R23)
  check the codec binding resolves every encoding the schema names (R19)
  return Reader { bytes, schema, L }
```

Every condition in `open` is fatal: a reader MUST NOT return a `Reader` on any of R01–R13 or R19.
After `open`, a reader needs no further validation to serve accesses; what remains is the
per-access bounds checking of §7.2.

### 7.2 Access

With `L` in hand, every non-repeatable step is a constant number of loads:

- **Row address.** `row(T, r) = tablesSection + tables[T].offset + r·rowSize(T)`, after checking
  `r < tables[T].count` (**R15**).
- **Presence.** For a slot with presence bit `b`, test `bytes[row + (b >> 3)] & (1 << (b & 7))`.
- **Slot load.** Read the slot's `size` bytes at `row + slot.offset`.
- **Ref.** For a `REF`, `ARRAY_REF` or `SELECT_LIST_REF`, resolve the `u32` against the slot's
  region: the ref MUST be a multiple of 4 with `ref + 4 ≤ regionLength`, and the item's body as
  given by its header MUST end within the region (**R14**).
- **Range.** For a `RANGE(R)`, `start + count ≤ tables[R].count`, computed in 64 bits (**R15**).
- **Record.** For a `RECORD(R)` or a Struct-variant payload, `r < tables[R].count` (**R15**).
- **Tag.** A `SELECT` tag MUST be a keyword index of one of the member's variants (**R17**).
- **Value.** A text scalar's bytes MUST be valid UTF-8 (**R18**; the B07 condition). An encoded
  scalar's bytes are given to the bound codec's `decode` (**R19** if the encoding is unresolved —
  normally caught at open; **R20** if the codec rejects the bytes, the B14 condition; and
  OPTIONALLY **R21** if `encode(decode(b)) ≠ b`, the B15 condition).

A **lazy** reader performs these checks at the access that needs them and reports R14–R18,
R20–R21 and R23 as failures of that access, continuing to serve others; a **verifying** reader
performs the full materialisation of §8 and the index verification of §9.5 at open and refuses
the file on any failure. Both are conforming; which a tool offers is its contract.

## 8. Materialisation

Materialisation produces the semantic model (TEL §18.2) from a RaTEL file. It is what gives
RaTEL its meaning — every access of §7.2 is defined as agreeing with the materialised model — and
it is the route by which a RaTEL file is converted to BinTEL or to text (§12).

```
materialise(reader):
  visited[T] = empty bitmap of tables[T].count bits, for every table T
  root = Node { keywordIndex: null, type: schema.document,
                children: struct-children(T = 0, r = 0, S = schema.document, depth = 0) }
  return root

struct-children(T, r, S, depth):
  if depth > 256: resource error (outside the R-codes, as BinTEL §11)
  if visited[T][r]: report R16                // the same row reached twice: aliasing or a cycle
  visited[T][r] = true
  row = row(T, r)                             // R15
  return members-of(row, S, S.members, path = [], depth)

members-of(row, S, members, path, depth):
  out = []
  for i, m in enumerate(members):             // member order
    slot = the slot of path ++ [i], if m is not flattened
    switch kind(m):
      NONE:            out.push(Flag(kidx(m)))
      FLAG:            if present(row, slot): out.push(Flag(kidx(m)))
      FLAG_COUNT:      n = load-u32; repeat n times: out.push(Flag(kidx(m)))
      INLINE(w):       if required(m) or present(row, slot):
                         out.push(Value(kidx(m), decode(m, bytes at slot, w)))
      REF:             if required(m) or present(row, slot):
                         out.push(Value(kidx(m), decode(m, item(ref))))
      ARRAY_REF:       for each element of item(ref), in order: out.push(Value(kidx(m), decode(…)))
      FLATTEN(R):      out.push(Node(kidx(m), R,
                         members-of(row, R.struct, R.struct.members, path ++ [i], depth)))
      RECORD(R):       if required(m) or present(row, slot):
                         r' = load-u32; out.push(Node(kidx(m), R, struct-children(tab(R), r', R.struct, depth + 1)))
      RANGE(R):        (start, n) = load; check R15
                       for k in 0 .. n: out.push(Node(kidx(m), R, struct-children(tab(R), start + k, R.struct, depth + 1)))
      SELECT:          if required(m) or present(row, slot): out.push(variant(load tag, load payload))
      SELECT_LIST_REF: for each (tag, payload) of item(ref), in order: out.push(variant(tag, payload))
  return out

variant(tag, payload):
  V = the variant with keyword index tag (R17)
  switch resolved type of V:
    Struct R: return Node(tag, R, struct-children(tab(R), payload, R.struct, depth + 1))
    Scalar:   return Value(tag, decode(V, item(payload)))
    Flag:     return Flag(tag)

decode(m, bytes):
  if m's scalar has no encoding: UTF-8-decode(bytes)       // R18
  else: codec-binding(encoding).decode(bytes)               // R19, R20; optional R21
```

`kidx(m)` is the member's keyword index in `S`'s keyword order — for a Field its own, for a
SelectRef the chosen variant's (the `tag`). Within one member the children appear in **stored
order** — rows `start .. start + n`, array elements, list pairs — which the writer took from the
semantic model, so the within-member order that property P2 (TEL §22.4) preserves is reproduced
exactly; across members the model's order is member order, which `members-of` follows. The result
is therefore `≡` to the model the writer encoded (§12).

A row the walk never reaches is an **orphan**. Orphans are permitted — an appending writer will
create them — and MUST be ignored; a verifying reader MAY report them informatively. A row reached
twice (**R16**) is not permitted: it would make the materialised model larger than the file, or
infinite.

## 9. Indexes

### 9.1 Enumeration and Sites

Indexes are enumerated by the schema, in a fixed order, so that writer and reader agree on `M`
and on which index is which without storing names:

1. Walk the tables in table order (§5.5); within each table, walk its slots in slot order
   (§6.2).
2. Each `RANGE(R)` slot is a **site**. At a site, emit first the **primary index** if `R` has a
   key field (TEL §20, `Field.key`); then one **secondary index** per `index` declaration on the
   Field (TEL §20, `Field.indexes`), in declaration order.
3. Each `ARRAY_REF` slot whose Field declares `index /` (the root path, permitted only on a
   repeatable Scalar-typed Field, TEL §20) is likewise a site with one secondary index over the
   values themselves.

`M` is the total. Sites are per slot, so a RecordDefinition reached from two Fields has an index
per Field; a type's instances reached through a repeatable *Select* variant have no site and no
index in version 1.

A secondary index's **paths** are resolved statically against the child type at open: each path
is a TELP ([TELP](telp.md) §3) whose components, taken in order from the child Struct, each name a
`Field` member (not a Select variant) whose effective `repeatable` is false; every non-final
component resolves to a Struct and the final one to a Scalar, which MAY be optional, as MAY any
intermediate Struct member. The schema constraints E228–E230 (TEL §20.1) guarantee this for a
valid composed schema; a reader finding otherwise reports **R11**.

### 9.2 Key Components

Index keys are compared **bytewise**, with no knowledge of the scalar types involved. This is
possible because text compares in code-point order under bytewise UTF-8 comparison — the exact
code-point equality that TEL's key uniqueness (E314, TEL §21.6) and TELP use — and because the
standard codecs are order-preserving by construction. A **component** is the encoding of one
scalar value, or of its absence:

```
component(value):
  if absent:                  return [0x00]
  if fixed-width (w bytes):   return [0x01] ‖ bytes                       // exactly w bytes
  else:                       return [0x01] ‖ escape(bytes) ‖ [0x00, 0x00]
escape(bytes): every 0x00 byte becomes 0x00 0xFF; every other byte is unchanged
```

`bytes` are the value's UTF-8 bytes for a text scalar and its codec bytes for an encoded one —
the same bytes a value item holds. The encoding is **order-preserving**: for any two values
`a`, `b` of one scalar, `component(a) <ₗₑₓ component(b)` iff `bytes(a) <ₗₑₓ bytes(b)`, and
absence sorts below every value. For variable-width values this holds because the terminator
`00 00` is below the escape `00 FF`, which is below every non-zero byte, so a proper prefix sorts
first and an embedded zero sorts as a zero; for fixed-width values the bytes are compared
directly. Concatenated components therefore compare as tuples.

### 9.3 Entries

An **entry** is:

```
u32BE hostRow ‖ component₁ ‖ … ‖ componentₖ ‖ u32BE childRow
```

`hostRow` is the record index of the row holding the `RANGE` (or `ARRAY_REF`) slot — the parent
of the children indexed — and `childRow` is the record index, in `R`'s table, of the child the
entry names (for an `ARRAY_REF` site, the element's position in the array). Both are
**big-endian** so that bytewise order groups entries by parent and, within a parent, by key and
then by child; these are the only big-endian integers in RaTEL (§3). For a primary index `k = 1`
and the component is the child's key value, which is always present (it may be defaulted); for a
secondary index there is one component per declared path, resolved from the child row, absent
components encoded as `00`.

The entries of an index MUST be pairwise distinct — they are, by the `childRow` tiebreak — and
MUST be sorted in strictly increasing bytewise order (**R24**). Secondary indexes impose no
uniqueness: two children with equal secondary components are adjacent entries distinguished by
`childRow`.

### 9.4 Lookup

A lookup is a lower-bound binary search over `offsets[]` comparing a **probe** — a prefix of an
entry — against each entry's bytes, one `memcmp` per step and no type knowledge at search time:

```
find(index, hostRow, key₁, …, keⱼ):              // j ≤ k components given
  probe = u32BE(hostRow) ‖ component(key₁) ‖ … ‖ component(keyⱼ)
  lo = 0; hi = index.count
  while lo < hi:
    mid = (lo + hi) / 2
    e = entry(mid)                                // R23 if offsets are inconsistent
    if compare-prefix(e, probe) < 0: lo = mid + 1 else: hi = mid
  first = lo
  // every entry from `first` on whose first len(probe) bytes equal `probe` matches;
  // the matches are contiguous
  return the childRow (last four bytes, big-endian) of each matching entry, in order
```

`compare-prefix(e, probe)` compares `e[0 .. len(probe)]` with `probe` bytewise (an entry shorter
than the probe compares as its own bytes followed by nothing, which sorts first). A probe of
`u32BE(hostRow)` alone enumerates a parent's children in key order; a full primary probe returns
zero or one child; a secondary probe returns a run. A reader MUST bounds-check every `offsets[i]`
and entry it touches (**R23**).

### 9.5 Verification

A verifying reader (§7.2) checks every index: each entry parses (presence bytes are `00` or
`01`, each variable component is properly escaped and terminated, each fixed component has its
width, the entry ends exactly after `childRow` — **R23**); entries are strictly increasing
(**R24**); and each entry agrees with the rows: `hostRow` and `childRow` are in range, `childRow`
lies within the `RANGE` that `hostRow`'s slot gives, the components recomputed from the child row
equal the stored bytes, and the index's `count` equals the total number of children at the site
over all host rows (**R25**).

## 10. Writing

A RaTEL writer takes a semantic model and its composed schema and produces a file. Normatively,
a writer:

- MUST encode only a valid document (TEL §21.9), and MUST be configured with a codec binding
  resolving every encoding the schema names (TEL §21.7), failing without output otherwise — as a
  BinTEL encoder does (BinTEL §7.1);
- MUST refuse a document in which any value of a `width w` scalar encodes to other than `w` bytes
  (**R22**);
- MUST compute every layout by §6.2, store every row by §6, every item by §5.3–§5.4, every index
  by §9, and zero every pad byte and every absent member's slot;
- MUST store a repeatable Struct member's children as consecutive rows in semantic order, and
  every repeatable member's elements in semantic order;
- MAY arrange sections in any non-overlapping order, intern or not intern as §5.3 permits, and
  order items within a region as it pleases.

**Canonical writer (informative).** Because RaTEL bytes are not deterministic, this
specification's test vectors are produced by one particular strategy, which an implementation
MAY follow and which keeps rows contiguous even for recursive types:

1. Compute all layouts. Keep a row cursor per table; allocate the root row.
2. `emit(node, T, r)`: fill `r`'s scalar and flag slots in slot order, appending variable items
   to the variable region (or interning into the string table, first occurrence first) as they
   are met; for each `RANGE(R)` slot **reserve** `(start = cursor[R], count)` and advance the
   cursor *before* descending, so that siblings are contiguous whatever the children allocate;
   for each `RECORD(R)` slot and Struct-variant payload allocate one row; then recurse into every
   allocated child row in slot order.
3. Build each index by computing its entries from the finished rows and sorting them.
4. Lay out the sections in directory order — schema, strings, variable, tables, indexes, digests
   — each 8-aligned; fill the header last.

## 11. Security Considerations

A RaTEL reader works on memory it did not write and MUST treat every integer it loads as
adversarial.

**Bounds.** Every offset, length, count, ref, record index and tag is checked before use (§7.2);
arithmetic on `offset + count·rowSize`, `start + count` and `ref + 4 + length` is performed in
64 bits so that it cannot wrap. A reader MUST NOT allocate memory proportional to a count before
the count has been bounded by the file.

**Termination.** Materialisation (§8) is bounded by the per-table visited bitmaps — each row is
visited at most once, so the work is linear in the number of rows — and by the depth limit of
256, matching BinTEL §11 and TEL §20.2; exceeding either is a resource error outside the R-code
taxonomy. Index search touches `log₂(count)` entries, each bounds-checked.

**The file may change beneath the reader.** A memory-mapped file whose publisher rewrites it in
place can present different bytes to successive loads. The publication discipline RaTEL is
designed for never mutates a published file — a new version is written beside it and renamed over
it, and a reader keeps its mapping of the old inode — but a reader cannot verify that. A reader
MUST copy the header before acting on it (§7.1), MUST NOT assume that a value loaded twice is the
same value, and MUST tolerate a bounds check that passed once failing later.

**The schema is executable structure.** As in BinTEL §6.2, the embedded schema decides how every
other byte is interpreted and arrives from the same source. R10 confirms only that the body is
consistent with its signature. A reader that requires a trusted schema MUST compare the verified
signature against the signatures it trusts, and MUST NOT infer trust from successful decoding.

**Digests and the value hash are not authenticity.** They detect accidental corruption and
identify content; a forger can recompute both. Authenticity, where needed, is a separate layer —
for a published file, the publisher's lock and the directory's ownership.

**Codecs run on attacker bytes.** As BinTEL §11: a codec's `decode` is invoked on value bytes
taken from the file and MUST handle malformed input safely; R21 is a hardening check, not a
substitute.

## 12. Round-Trip and Conversion

**P5 (RaTEL losslessness).** For every valid semantic model `M` under composed schema `S` with
codec binding `B`, and every conforming writer `ratel-encode`,

```
materialise(open(ratel-encode(M, S, B))) ≡ M
```

where `≡` is semantic-model equality (TEL §22.4). Consequently
`value-hash(bintel-encode(materialise(F)))` is well defined for any valid RaTEL file `F`, is the
same for every RaTEL file encoding `M`, and — when `F`'s header carries a value hash — MUST equal
it; a verifying reader MAY check this (**R27**). The value hash, not the file's bytes, is a RaTEL
document's identity.

**Conversion.** Text TEL, BinTEL and RaTEL are three encodings of one model. Converting between
any two is: obtain the semantic model (TEL §18.3 from text, BinTEL §7.8 from BinTEL, §8 from
RaTEL), then encode (TEL §22.3 canonical text, BinTEL §7, §10 here). Every direction is lossless
in the semantic model; presentation detail — comments, atom forms, tabulation — is not in any of
the binary encodings (BinTEL §7.8). A tool that inspects TEL documents SHOULD inspect all three:
dump a RaTEL file's layout, rows and indexes; look up a record by key; convert among the forms.

## 13. Worked Example

Schema (base, no layers):

```tel
tel 1.0 specification.tel/tels:2.1.0

name catalogue

record Entry
  field name String key
  field info String optional

document
  field entry Entry repeatable
```

Document:

```tel
tel 1.0 ‹signature›

entry alpha
  info First letter

entry beta
```

**Layouts.** Table 0 (root): one member, `entry`, repeatable Struct → `RANGE(Entry)`; no presence
bits; `start` at offset 0, `count` at 4; `rowSize = 8`. Table 1 (`Entry`): `name` is a required
variable-width scalar → `REF`, no bit; `info` is optional → `REF`, presence bit 0.
`bitmapBytes = 1`; after the align-4 pass `name` is at 4 and `info` at 8; `rowSize = round8(12)
= 16`. Indexes: one site (table 0, path `[0]`); `Entry` has a key field → `M = 1`, a primary
index on `name`.

**Variable region** (36 bytes, padded to 40), as the canonical writer emits it:

```
00: 05 00 00 00  61 6C 70 68 61 00 00 00                   "alpha"         (pad 3)
0C: 0C 00 00 00  46 69 72 73 74 20 6C 65 74 74 65 72       "First letter"
1C: 04 00 00 00  62 65 74 61                               "beta"
24: 00 00 00 00                                            section pad
```

**Tables section** (88 bytes):

```
00: 30 00 00 00 00 00 00 00  01 00 00 00  08 00 00 00  00 00 00 00 00 00 00 00
                                     table 0: offset 48, count 1, rowSize 8, row-major
18: 38 00 00 00 00 00 00 00  02 00 00 00  10 00 00 00  00 00 00 00 00 00 00 00
                                     table 1: offset 56, count 2, rowSize 16
30: 00 00 00 00  02 00 00 00         root row: entry = rows 0..2 of table 1
38: 01 00 00 00  00 00 00 00  0C 00 00 00  00 00 00 00
                                     Entry 0: bitmap 01 (info present); name → 0x00; info → 0x0C
48: 00 00 00 00  1C 00 00 00  00 00 00 00  00 00 00 00
                                     Entry 1: bitmap 00; name → 0x1C; info absent (zero)
```

**Indexes section** (64 bytes):

```
00: 10 00 00 00 00 00 00 00  02 00 00 00  00  00 00 00    index 0: offset 16, count 2, sorted
10: 00 00 00 00  10 00 00 00  1F 00 00 00                  offsets[3] = 0, 16, 31
1C: 00 00 00 00  01 61 6C 70 68 61 00 00  00 00 00 00      host 0 | 01 "alpha" 00 00 | child 0
2C: 00 00 00 00  01 62 65 74 61 00 00  00 00 00 01         host 0 | 01 "beta"  00 00 | child 1
3B: 00 00 00 00 00                                         section pad
```

**Schema section**: the 33-byte signature of `catalogue` preceded by its length byte `21`, then
the varint length and bytes of the schema body under `tels`, padded to a multiple of 8 — `L_s`
bytes in all, whose exact value depends on the body's encoding and is pinned by the reference
implementation's vectors.

**Header**: `B1 C4 B5 BB 01 00 00 00`; committed length `336 + L_s`; directory: schema
`(144, L_s)`, strings `(144 + L_s, 0)`, variable `(144 + L_s, 40)`, tables `(184 + L_s, 88)`,
indexes `(272 + L_s, 64)`, digests `(336 + L_s, 0)`; flags `0`; value hash zeros (or flag bit 1
and the document's BLAKE3-256 value hash).

**Lookup of `beta`**: probe `00 00 00 00 01 62 65 74 61 00 00`; the search compares it against
entry 1 (`mid = 1`) — equal on the prefix — then entry 0 (`alpha` < `beta`), settling on
`first = 1`; the match's last four bytes give child row 1, whose row is at
`56 + 1·16 = 72` in the tables section, where `name`'s ref `0x1C` finds `beta` and the clear
presence bit says `info` is absent.

## 14. Reader Error Handling

Codes are local to RaTEL; they do not overlap with the E1xx/E2xx/E3xx taxonomy of the TEL
Specification or the B-codes of BinTEL, though several restate a B-code's condition.

| Code | Condition | When |
| ---- | --------- | ---- |
| R01 | Magic number is not `B1 C4 B5 BB` | open |
| R02 | Unsupported version byte | open |
| R03 | Reserved header byte or reserved flag bit non-zero | open |
| R04 | File shorter than 144 bytes; committed length `< 144`, greater than the file, or not a multiple of 8 | open |
| R05 | A directory offset or length is not a multiple of 8, an offset is `< 144`, or `offset + length` exceeds the committed length | open |
| R06 | Two non-empty sections overlap | open |
| R07 | A section or header digest does not match (flag bit 0 set), or flag bit 0 is clear and the digests section is non-empty | open / verify |
| R08 | Schema signature structurally invalid — the B03 conditions | open |
| R09 | Embedded schema body does not decode under `tels` — the B12 condition | open |
| R10 | Recomputed composed signature differs from the carried signature — the B11 condition | open |
| R11 | The schema cannot be laid out: unresolvable `Reference`/`SelectRef` (the B10 condition), `intern` on a fixed-width scalar, a secondary-index path that does not resolve per §9.1 | open (configuration) |
| R12 | Table directory inconsistent with the layout: wrong `rowSize`, unknown `layout` byte, non-zero reserved bytes, misaligned offset, rows exceeding the section | open |
| R13 | Table 0 does not have exactly one row | open |
| R14 | A ref is not a multiple of 4, or an item's header or body extends beyond its region | access / verify |
| R15 | A record index is `≥` its table's count, or a range's `start + count` exceeds it | access / verify |
| R16 | A row reached twice during materialisation — aliasing or a cycle | materialise |
| R17 | A select tag is not the keyword index of one of the member's variants | access / verify |
| R18 | A text scalar's bytes are not valid UTF-8 — the B07 condition | access / verify |
| R19 | An encoding named by the schema is not resolved by the codec binding — the B13 condition | open |
| R20 | A codec's decoder rejects a value's bytes — the B14 condition | access / verify |
| R21 | OPTIONAL re-encode check found `encode(decode(b)) ≠ b` — the B15 condition | access / verify |
| R22 | *Writer:* a value of a `width w` scalar encodes to other than `w` bytes; the writer refuses. Never a reader condition | write |
| R23 | Index directory or entries malformed: unknown kind, entry count differing from `M`, non-zero reserved bytes, offsets non-monotone or beyond the section, an entry that does not parse | open / lookup / verify |
| R24 | Index entries not in strictly increasing bytewise order | verify |
| R25 | An index entry disagrees with the rows: host or child out of range, child outside the host's range, recomputed components differ, or the count differs from the children at the site | verify |
| R26 | Non-zero bytes in an absent member's slot or in padding | verify (optional) |
| R27 | The header's value hash differs from the BLAKE3-256 value hash of the materialised document | verify (optional) |

R01–R13 and R19 are **open-time** conditions: a reader MUST report them and MUST NOT return a
reader. R14–R18, R20, R21 and R23 arise on access; they are fatal for the access that encounters
them, and a lazy reader MAY continue to serve other accesses while a verifying reader rejects the
file. R24–R27 arise only in verification. Resource limits (depth, memory) are outside the
taxonomy, as in BinTEL §11.

**Precedence.** Header checks are performed in table order, R01 first. Within one slot access,
a record index is checked (R15) before a ref (R14), a tag (R17) before its payload, and the
value's bytes (R18/R20) last. Where a verifying reader could report several codes for one defect,
it MUST report the first in this order.

## 15. Future Work (Informative)

The following are anticipated and provided for, but not specified:

- **An append log.** The committed length (§5.1) is the single word an appender would advance
  after writing new rows past the old committed length and rebuilding the indexes in a side
  file; placing the tables section before the indexes section, as the canonical writer does,
  keeps the appended rows adjacent to the old ones. Appending *rows to a table* is easy;
  appending *children to an existing parent* is not, because a `RANGE` is contiguous — a
  log-structured variant will need an overflow mechanism (a continuation range, or a per-site
  overflow table) and a rule for indexes over rows the base file does not know. Nothing in the
  version-1 format precludes either.
- **Column-major tables**, under the reserved `layout` byte (§5.5), for scans over one field.
- **Hashed indexes**, under the reserved index `kind` (§5.6), for pure point lookups where a
  binary search's `log₂ n` probes matter.
- **Wide refs** — `u64` refs for regions beyond 4 GiB — under a reserved `layout` value.
- **Indexes over Select-variant instances** (§9.1).
