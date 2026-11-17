# RaTEL Specification Draft

## Abstract

RaTEL (**random-access TEL**) is the third physical encoding of the semantic model of a TEL
document, as defined by the [TEL Specification](tel.md), alongside the text form and
[BinTEL](bintel.md). Where text TEL is for people and BinTEL is for streams, RaTEL is for data
that is **kept and changed in place**: a memory-mapped file in which looking up one record touches
that record and nothing else, and in which records can be added, changed, reordered and removed
without the document ever being materialised on the heap.

BinTEL is count-driven: a struct stores how many children it has, integers are varints, and a
repeated field is the same keyword index recurring among the siblings (BinTEL §4, §7). A whole
document can be skipped at constant cost, but reaching the *n*-th child of a struct, or any field
of any struct, means walking every predecessor with the schema in hand. RaTEL is offset-driven:
every struct type has a fixed-width **row** with one **slot** per member at a schema-determined
offset; instances of a type form a **table**; variable-width values and growable lists live in
separate regions addressed by offset; and **B+tree indexes** over keyed and declared members make
lookup a descent through mapped memory. Field access is one load at a known offset; nothing is
walked.

Every growable structure — a table, a region, a child list, an index — is allocated with slack and
relocated by doubling when it fills, so insertion costs amortised constant time per row and
logarithmic time per index entry. Space left behind by relocations and deletions is reclaimed by
an explicit **repack**. A file is changed by one **owner** at a time; to publish it to other
processes, the owner **seals** it, producing a compact, read-only file of the same format that
carries the document's value hash.

RaTEL shares with BinTEL everything that is not layout: the semantic model (TEL §18), schemas and
schema layers, the schema signature and the **value hash** (BinTEL §3, §8), the keyword-index
numbering of a struct's members (BinTEL §5), the codec contract for encoded scalars (TEL §21.7),
and self-contained carriage of the composed schema (BinTEL §6.2). A RaTEL file always carries its
schema. Conversion among text TEL, BinTEL and RaTEL is lossless (§12), and the changes a RaTEL
owner may make are exactly the semantic machine operations of TEL §22.2. The storage-layout
annotations a schema may carry — `width`, `intern` and `index` (TEL §20) — are read by RaTEL
and by nothing else; the fixed-width, order-preserving standard codecs on which RaTEL's inline
slots and type-blind index keys rely are defined in the companion
[Standard Codecs Specification](codecs.md).

RaTEL is defined as a byte sequence to be read in place. It has no textual form.

## 1. Status

This document is a draft specification of RaTEL version 1. It defines one format, which a file
holds in one of two **states** (§5.8):

- **unsealed** — a working file, changed in place by a single owner (§10), with slack in its
  structures and possibly garbage between them;
- **sealed** — the compact, read-only result of `seal` (§10.5), with no slack and no garbage,
  optionally carrying the document's value hash and section digests. Sealed files are what a
  publisher writes beside the old version and renames into place, for any number of processes to
  map.

Both states are read by the same procedures (§7–§9). Version 1 does not provide concurrent
readers of a file that is being changed, nor crash-safe in-place updates (§15): an owner's
interrupted session leaves the file **dirty** (§5.8), and durability comes from sealing.

## 2. Conformance Language

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHALL**, **SHALL NOT**, **SHOULD**, **SHOULD
NOT**, **RECOMMENDED**, **MAY**, and **OPTIONAL** in this document are to be interpreted as
described in RFC 2119 and RFC 8174 when, and only when, they appear in all capitals.

## 3. Integers, Extents, Regions, Tables and Terms

**Structural integers.** Every integer that RaTEL itself defines — header fields, directory
entries, row headers, slots, offsets, counts, capacities, tags, page fields — is an unsigned,
fixed-width, **little-endian** integer, written `u8`, `u16`, `u32` or `u64`, stored at an offset
that is a multiple of its width. There is one exception, stated once here and again where it
applies (§9.3): the record indices inside the bytes of an index entry are **big-endian**, because
index entries are compared bytewise and their order must agree with numeric order. BinTEL's
variable-length integers (BinTEL §4) appear in a RaTEL file only inside the schema section
(§5.2), which is a BinTEL fragment.

**NONE.** The `u32` value `0xFFFFFFFF` is written **NONE**. It is never a valid record index, ref
or page number, and stands for "no row", "no item" or "no page" wherever one of those is
expected. A collection slot holding NONE is the **empty ref** (§6.3).

**Codec bytes are opaque.** The bytes of an encoded scalar (TEL §21.7) are copied into a RaTEL
file verbatim and are never loaded by RaTEL as integers; they are handed to the bound codec, or
to an application that knows the codec. The standard codecs ([Standard Codecs](codecs.md)) are
big-endian so that their bytes compare bytewise in numeric order; that is a property of the codec,
not of RaTEL, and it does not conflict with the little-endian rule above.

**Alignment.** `round4(n)` and `round8(n)` denote `n` rounded up to the next multiple of 4 or 8.
Every extent begins at a file offset that is a multiple of 8; every row begins at a multiple of
8; every item in a region (§5.3, §5.4) and every cell in an index page (§9.4) begins at a
multiple of 4. Pad bytes MUST be zero when written and MUST be ignored when read.

**Extent, length, capacity.** An **extent** is a byte range of the file `[offset, offset +
capacity)` that holds one growable structure: a section (§5), the rows of one table (§5.5), or the
pages of one index (§5.6). Its **length** is the part in use, from its start; the rest, up to its
**capacity**, is **slack** that allocation consumes without moving anything. When an extent's
slack is exhausted, the owner **relocates** it: copies its used bytes to a new, larger extent and
changes one directory entry (§10.2). Every address RaTEL stores *within* the file is relative —
a record index, a ref into a region, a page number within an index — so a relocation never
invalidates anything but the directory entry it rewrites.

**Region.** An extent of at most `2^32` bytes within which a `u32` **ref** is a byte offset
from the region's start. RaTEL has two regions: the string table (§5.3) and the variable region
(§5.4). A ref always denotes the first byte of an **item** (§5.3, §5.4).

**Table, row, record index.** A **table** holds the instances of one Struct type as **rows** of
`rowSize` bytes each; row `r` (the **record index**) begins at `tableOffset + r·rowSize`. Rows
`0 .. count` have been allocated; each is either **live** — part of the document — or **free**,
awaiting reuse (§6.2). **Table 0** holds the document root and has exactly one row. A live row's
record index is stable for as long as the file is not repacked or sealed, so an application MAY
hold record indices as handles between changes.

**Slot.** A fixed-width field of a row, at a schema-determined offset, holding a member's value
or a reference to it (§6).

**Flattening, host row, site.** A required, non-repeatable Struct-typed member is **flattened**:
its slots are laid into its parent's row rather than into a row of its own (§6.1). A member's
**host row** is the row that holds its slot, which may be the row of an enclosing struct several
flattening steps up. A **site** is a member position identified by its host table and its
flattened member path (§6.2); the same RecordDefinition reached from two sites has two sites but
one table.

**Owner, garbage.** The **owner** is the one process that has the file open for changing (§10.1).
**Garbage** is space no structure of the document uses and no free list reaches: items abandoned
by updates, deletions and list growth, and extents abandoned by relocation. Garbage is harmless;
`repack` reclaims it (§10.5).

**Composed schema.** Throughout, *the schema* means the composed schema (TEL §20.3) that the
file's schema section names, and *member order* and *keyword order* are those of TEL §20.

## 4. Document Model

A RaTEL file encodes **exactly one** TEL semantic model (TEL §18.2) under one composed schema,
exactly as a BinTEL document does (BinTEL §6). The root is a virtual Struct whose member order is
that of `Schema.document`; it is stored as the single row of table 0. Any **collection** —
records looked up by key, iterated in order, or scanned — is simply an effectively `repeatable`
member of some struct, most often of the root; RaTEL gives it a table (§5.5), a **child list**
recording its order (§5.4), and, when its type has a key field, a primary index (§9.1).

As in BinTEL, a required Scalar member supplied by its `default` is a value like any other and
is stored (BinTEL §7.6); an explicitly empty text and a defaulted empty text are the same value
(BinTEL §7.5); and an invalid document (any ↯/t.3 condition of TEL §21.9) is never encoded — a
RaTEL writer MUST fail rather than emit one, and an owner MUST reject any change that would
produce one (§10.3). Every clean RaTEL file, sealed or not, therefore holds a valid document.

RaTEL is **not byte-deterministic**: two conforming writers MAY lay out the same semantic model
differently, and an unsealed file's bytes depend on the history of changes made to it. Properties
P3 and P4 of TEL §22.4 therefore do not apply to RaTEL bytes. What RaTEL guarantees is **semantic
losslessness**, property P5 (§12): materialising a RaTEL file reproduces the semantic model
exactly, and a document's identity is its value hash (BinTEL §3), computed over its BinTEL
encoding, which a sealed file MAY carry in its header. The canonical `seal` procedure (§10.5) is
deterministic, and is what this specification's test vectors use.

## 5. File Layout

A RaTEL file is a header (§5.1) followed by six **sections** and by the extents of the tables and
indexes, in any order, each located through a directory: the sections by the header's section
directory, the tables by the table directory (§5.5), the indexes by the index directory (§5.6).
Readers locate everything through the directories only.

### 5.1 Header

The header occupies bytes `0..200` of the file:

| Offset | Size | Field |
| -----: | ---: | ----- |
| 0 | 4 | **Magic number**: the bytes `B1 C4 B5 BB`. In the BASE-256 alphabet ([BASE-256](base256.md) §4) these are `ᾱ` (`U+1FB1`), `τ`, `ε`, `λ` — `ᾱτελ`, with the alpha of *access* beside BinTEL's `βτελ`. Every byte is `≥ 0x80`, so the file cannot be mistaken for text. Any other value is **↯/t.5.1**. |
| 4 | 1 | **Version**: `0x01`. Any other value is **↯/t.5.2**. |
| 5 | 1 | Reserved; MUST be `0x00` (**↯/t.5.3**). |
| 6 | 2 | **Flags** (`u16`): bit 0 — a digests section is present (§5.7); bit 1 — the value hash field holds the document's value hash; bit 2 — **sealed**; bit 3 — **dirty** (§5.8). All other bits MUST be clear (**↯/t.5.3**). |
| 8 | 8 | **Committed length** (`u64`): the end of allocated space. Every extent lies below it, and the owner allocates new extents from it upwards (§10.2). MUST be `≥ 200`, a multiple of 8, and `≤` the file's actual length (**↯/t.5.4**). Bytes at or beyond the committed length are **reserve** and MUST NOT be read. |
| 16 | 144 | **Section directory**: six entries of `(offset: u64, length: u64, capacity: u64)`, in the fixed order *schema*, *strings*, *variable*, *tables*, *indexes*, *digests*. |
| 160 | 8 | **Garbage** (`u64`): the owner's count of garbage bytes (§3). Advisory: readers MUST NOT rely on it, and owners use it to decide when to repack (§10.5). MUST be `0` in a sealed file (**↯/t.5.28**). |
| 168 | 32 | **Value hash**: the BLAKE3-256 value hash of the document (BinTEL §3) when flag bit 1 is set; otherwise 32 zero bytes. |

Directory constraints, each **↯/t.5.5** when violated: every `offset` is a multiple of 8 and
`≥ 200`; every `capacity` is a multiple of 8; `length ≤ capacity`; `offset + capacity ≤ committed
length` (computed without overflow). The schema, tables, indexes and digests sections have a fixed
size, and their `length` MUST equal their `capacity`. The `length` of a region (strings, variable)
is its allocation mark: a multiple of 4, beyond which the region holds no items; a region length
that is not a multiple of 4 is a misaligned entry, and likewise **↯/t.5.5**. A section MAY have
`capacity = 0`; a zero-capacity extent does not overlap anything, whatever its offset.

Non-empty extents — the six sections, the rows of every table (§5.5) and the pages of every
index (§5.6) — MUST NOT overlap one another, nor the header (**↯/t.5.6**). Bytes below the committed length that belong to no
extent are garbage (in an unsealed file) or forbidden (in a sealed file, §5.8).

### 5.2 Schema Section

The schema section carries the composed schema in the form of BinTEL §6.2 fields 3 and 4,
verbatim:

1. the byte length of the schema signature (BinTEL integer, §4 there) and the signature bytes —
   a palimpsest at the BinTEL-pinned parameters, whose length MUST be `33` or `37 + 2·(n − 2)`,
   `n ≥ 2`, and whose bytes MUST XOR to `0x79` (**↯/t.5.8**; the ↯/t.4.3 conditions);
2. the byte length of the schema body (BinTEL integer) and the body — the bare document-root
   encoding of the schema document under `tels` (TEL §20.5), with the root member list taken to
   be `tels.document.members`;
3. zero padding to the section length.

A reader decodes the body under its built-in `tels` axiom (**↯/t.5.9** if it does not decode; the
↯/t.4.12 condition), composes the schema per TEL §20.3 — the body MAY contain `layer` compounds —
and recomputes the composed signature per BinTEL §8, which MUST equal the carried signature
byte-for-byte (**↯/t.5.10**; the ↯/t.4.11 condition). A reader MUST NOT interpret any table, region
or index before ↯/t.5.10 has passed. Having composed the schema, the reader derives the **layout**
of every table and the numbering of links (§6.2); an unresolvable `Reference` or `SelectRef` (the
↯/t.4.10 condition), a scalar marked `intern` whose width is fixed, a secondary-index path that
does not resolve under §9.1, or more than 65535 links means the schema cannot be laid out, and is
**↯/t.5.11** — a configuration error of the schema, as ↯/t.4.10 is, not of the file's data.

The embedded schema is always present: RaTEL has no external-schema mode. A file is read by many
processes over a long life, and the one thing every reader must agree on is the schema; carrying
it costs a few hundred bytes once. A reader that already holds the schema identified by the
signature MAY skip decoding the body, as BinTEL §6.2 permits, provided it has previously verified
that schema's signature. The schema section never changes after a file is created.

### 5.3 String Table

The string table is a region (§3) of **value items**, each:

```
u32 length          byte length of the value
u8  bytes[length]   the value's bytes: UTF-8 for a text scalar, codec bytes for an encoded one
    pad             zero bytes to the next multiple of 4
```

It holds the values of every scalar whose composed ScalarDefinition carries `intern` (TEL §20).
A sealed file SHOULD store each distinct value once, so that two equal values have equal refs and
an application can compare interned values by comparing refs; an owner adding values to an
unsealed file MAY append them without looking for an equal one. A reader MUST NOT rely on
deduplication — two distinct refs MAY hold equal bytes — and MUST compare bytes when equality
matters.

Because an interned item may be shared by many slots, it is **never modified or reclaimed in
place**; a slot whose value changes is pointed at another item, and the old one, if no longer
used, is garbage that only `repack` can identify.

### 5.4 Variable Region

The variable region is a region of items, each beginning with a `u32` and padded to a multiple of
4, holding everything that has no fixed width and is not interned. An item's **kind** is
determined by the slot that refers to it, through the schema; no tag is stored:

| Kind | Layout | Holds |
| ---- | ------ | ----- |
| *value item* | `u32 length`, `u8 bytes[length]` | the bytes of one variable-width scalar value |
| *child list* | `u32 capacity`, `u32 count`, `u32 rows[capacity]` | the record indices of a repeatable Struct member's children, in semantic order (§6.1) |
| *packed array* | `u32 capacity`, `u32 count`, `u8 values[capacity·w]` | the values of a repeatable fixed-width scalar, each exactly `w` bytes |
| *ref array* | `u32 capacity`, `u32 count`, `u32 refs[capacity]` | refs to value items (in the string table if the scalar is `intern`, else in this region), one per value |
| *select list* | `u32 capacity`, `u32 count`, `(u32 tag, u32 payload)[capacity]` | the fillings of a repeatable SelectRef, as §6.1 defines for a `SELECT` slot |

Each item is followed by zero padding to the next multiple of 4. The last four kinds are
**growable**: their first `count` elements are meaningful, and the remaining `capacity − count` are
slack that an owner fills in place; `count ≤ capacity` is required, and an item with `count = 0`
is permitted but SHOULD be replaced by the empty ref (§6.3). Slack elements SHOULD be zero, MUST
be ignored when read, and MUST be zero in a sealed file, where moreover `capacity = count`
(**↯/t.5.28**).

An item's extent — the header, then the body as computed from the header — MUST lie within the
region's `length`, `count ≤ capacity` MUST hold, and a ref MUST be a multiple of 4
(**↯/t.5.14**). A reader checks the header's extent before reading the header, and the body's extent
before reading the body.

**Ownership of items.** Every growable item and every variable-region value item is referred to
by **exactly one** slot or element of the document; this is what lets an owner change it in place
or abandon it as garbage. Value items are nevertheless never modified in place: an update appends
a new item and abandons the old (§10.3). The variable region also holds the **spilled** entries
and separators of index pages (§9.4), each a value item referred to by exactly one cell.

### 5.5 Tables Section

The tables section is the **table directory**: `N = 1 + |records|` entries, where `records` is the
composed schema's RecordDefinition list in composed-namespace order (TEL §20.3: the base schema's
records in declaration order, then each layer's new records in layer order). Table 0 is the
document root; table `i + 1` is `records[i]`. Every RecordDefinition has a table, whether or not
any instance of it exists un-flattened — an unused table has `capacity = 0` and costs its
directory entry alone. Both sides compute this numbering from the schema, so no reachability
analysis is needed and none is permitted to differ. The section's length is exactly `32·N`.

Each directory entry is 32 bytes:

```
u64 offset        file offset of the table's extent; a multiple of 8
u32 count         rows allocated: rows 0 .. count exist, each live or free
u32 capacity      rows the extent can hold; the extent is capacity·rowSize bytes
u32 rowSize       bytes per row; MUST equal the layout's rowSize for this table (§6.2)
u32 freeHead      the first row of the table's free list (§6.2), or NONE
u8  layout        0 = row-major; every other value is reserved
u8  reserved[7]   MUST be zero
```

Violations are **↯/t.5.12**: a section length other than `32·N`; `rowSize` differing from the
computed value; an unknown `layout` byte; non-zero reserved bytes; a misaligned `offset`;
`count > capacity`; `count = NONE`; `freeHead` neither NONE nor `< count`; or `offset +
capacity·rowSize` (computed in 64 bits) exceeding the committed length. Table 0 MUST have
`count = 1` and `freeHead = NONE`, and its row MUST be live (**↯/t.5.13**).

Row storage is **row-major**: row `r` of a table is the `rowSize` bytes at `offset + r·rowSize`,
laid out per §6. The `layout` byte is reserved so that a column-major table — the same slots stored
as one array per slot, for scans over one field across many rows — can be registered without a
format break.

### 5.6 Indexes Section

The indexes section is the **index directory**: `M` entries, where `M` is the number of indexes the
schema determines (§9.1). The section's length is exactly `32·M`. Each entry is 32 bytes:

```
u64 offset        file offset of the index's page extent; a multiple of 8
u32 pageCount     pages allocated: pages 0 .. pageCount exist, each in the tree or free
u32 pageCapacity  pages the extent can hold; the extent is pageCapacity·pageSize bytes
u32 root          page number of the root page, or NONE when the index has no pages in use
u32 freeHead      the first page of the index's free-page list, or NONE
u32 count         number of entries in the index
u8  kind          0 = B+tree (§9.4); every other value is reserved
u8  pageShift     the page size is pageSize = 2^pageShift bytes; 9 ≤ pageShift ≤ 16
u8  height        levels in the tree: 0 when root is NONE, otherwise ≥ 1 (1 = the root is a leaf)
u8  reserved      MUST be zero
```

Page `p` of an index occupies `offset + p·pageSize .. offset + (p + 1)·pageSize`. An unknown
`kind`; a `pageShift` out of range; a section length other than `32·M`; a non-zero reserved byte;
`pageCount > pageCapacity`; `root` or `freeHead` neither NONE nor `< pageCount`; `root = NONE`
without `height = 0` and `count = 0`, or `root ≠ NONE` with `height = 0`; `height > pageCount`; or
an extent beyond the committed length is **↯/t.5.23**. Pages are 4096 bytes (`pageShift = 12`) by
RECOMMENDATION, and an owner SHOULD align page extents to the page size so that each page is one
virtual-memory page.

### 5.7 Digests Section

Digests are carried only by a sealed file. When flag bit 0 is set, the digests section holds two
BLAKE3-256 digests, `64` bytes in all; its length MUST be exactly `64`, and it MUST be the last
extent of the file, with `offset + 64 = committed length` (**↯/t.5.7** otherwise):

| Offset | Size | Digest of |
| -----: | ---: | --------- |
| 0 | 32 | the **body**: file bytes `200 .. offset`, where `offset` is the digests section's own |
| 32 | 32 | the **header**, bytes `0..200` |

The header is digested as it is finally written: its input includes the flags (with bits 0 and 2
set), the committed length, every directory entry — that of the digests section itself among
them — and the value hash field. The digests section is the input of neither digest, so nothing
is circular: a writer completes the body and header, then computes the two digests and writes
them.

A reader that verifies digests MUST report a mismatch as **↯/t.5.7**. When flag bit 0 is clear, the
digests section's capacity MUST be `0` (**↯/t.5.7** otherwise).

Digest verification is OPTIONAL and a reader MAY defer it, but a reader that advertises
integrity checking MUST complete it before trusting any section's contents. Digests detect
accidental corruption; they are not an authenticity mechanism (§11).

### 5.8 States

The **sealed** and **dirty** flags (bits 2 and 3) place a file in one of three states:

- **Unsealed and clean** (neither bit): a working file, closed by its last owner after a complete
  session (§10.1). It may contain slack, free rows, free pages and garbage. A reader MAY read it,
  but only while no owner has it open (§10.1, §11).
- **Dirty** (bit 3 alone): an owner has the file open for changing, or did and never closed it.
  Its contents may be inconsistent. A reader MUST refuse a dirty file (**↯/t.5.29**); only its owner
  — the process that set the bit, holding the lock of §10.1 — reads it.
- **Sealed** (bit 2 alone): the output of `seal` (§10.5). A sealed file is never changed in place,
  and MAY be shared with any number of readers.

Flag bits 0 (digests) and 1 (value hash) MAY be set only in a sealed file, and bits 2 and 3 MUST
NOT both be set (**↯/t.5.28**). A sealed file MUST moreover be **compact**, every condition below
being **↯/t.5.28**:

- `garbage = 0`;
- every section's capacity is `round8(length)`; every table's `capacity = count` and `freeHead =
  NONE`; every index's `pageCapacity = pageCount` and `freeHead = NONE`;
- the extents, taken in file order, tile `200 .. committed length` with no gap other than the
  zero padding needed to align each extent to 8;
- *(verification only)* every row is live, every page is in its tree, and every growable item
  has `capacity = count`.

The directory-level conditions are checked at open; the last only by a verifying reader.

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
| Field, fixed-width Scalar, repeatable | `ARRAY_REF` — `u32` ref to a packed array, or the empty ref | 4 | 4 | no |
| Field, variable-width Scalar, repeatable | `ARRAY_REF` — `u32` ref to a ref array, or the empty ref | 4 | 4 | no |
| Field, Struct `R`, required, non-repeatable, `R` not on the stack | `FLATTEN(R)` — `R`'s slots laid into this row (§6.2) | — | — | no |
| Field, Struct `R`, required, non-repeatable, `R` on the stack | `RECORD(R)` — `u32` record index into `R`'s table | 4 | 4 | no |
| Field, Struct `R`, optional, non-repeatable | `RECORD(R)` — `u32` record index | 4 | 4 | yes |
| Field, Struct `R`, repeatable | `CHILDREN(R)` — `u32` ref to a child list (§5.4) naming rows of `R`'s table, the children in semantic order, or the empty ref | 4 | 4 | no |
| SelectRef, non-repeatable | `SELECT` — `u32 tag`, `u32 payload` | 8 | 4 | iff optional |
| SelectRef, repeatable | `SELECT_LIST_REF` — `u32` ref to a select list, or the empty ref | 4 | 4 | no |

The *presence bit* column says whether the slot has a bit in the row's presence bitmap (§6.2).
Only optional, non-repeatable members have one: a required member is always present, and a
repeatable member's emptiness is its count of zero.

**Select tag and payload.** `tag` is the **keyword index** (BinTEL §5) of the chosen variant in
the keyword order of the Struct that declares the SelectRef — the same number the semantic model
records (TEL §18.2) and BinTEL encodes. It MUST fall within the range of keyword indices that the
member's variants occupy (**↯/t.5.17**). `payload` depends on the variant's resolved type: a Struct `R`
→ a record index into `R`'s table (variant instances are never flattened); a Scalar → a ref to a
value item holding the value's bytes (in the string table if that scalar is `intern`, otherwise in
the variable region — fixed-width scalars included, for uniformity); `Flag` → `0`.

**Width and intern.** `width w` is a promise that every value of the scalar encodes to exactly
`w` bytes; a writer or owner MUST refuse a value whose byte length differs (**↯/t.5.22**) — it is a
configuration failure of the writer, as an unresolved encoding is (TEL §21.7), never a padding
instruction and never a document error. `intern` on a fixed-width scalar has no meaning and is
**↯/t.5.11**. Neither annotation changes the semantic model, the value hash, or the BinTEL
encoding.

### 6.2 Layout Algorithm, Row Headers and Links

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
  off = 8 + bitmapBytes                       // after the row header
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
- The **presence bitmap** occupies row bytes `8 .. 8 + bitmapBytes`; bit `b` is at byte
  `8 + (b >> 3)`, bit `b & 7` (least significant bit first). Slots are then packed by descending
  alignment, which wastes at most 7 bytes per row to padding.
- `rowSize` is a multiple of 8, so every row of every table is 8-aligned when the table is.

**Row header.** Bytes `0..8` of every row are its header:

```
u32 parent    live row: the record index of the row holding the slot that refers to this row,
              or NONE for the root; free row: the next row of the table's free list, or NONE
u16 link      live row: the link number (below) of that slot, or 0xFFFF for the root;
              free row: 0xFFFF
u16 flags     bit 0: live. All other bits MUST be zero.
```

A row is **live** if its live bit is set and **free** otherwise. The free rows of a table form a
singly linked **free list** through their `parent` fields, starting at the table's `freeHead`; an
owner takes rows from it before extending the table (§10.2). Readers never follow it. A reader
that reaches a free row through any slot or list reports **↯/t.5.15**.

The parent link exists so that an owner holding only a record index — a handle, or the
`childRow` of an index entry — can find the slot that refers to the row, and from it the index
entries a change must update (§10.3); it is not needed for reading. A verifying reader checks that
every live row's link names the slot that actually reaches it, that every live row is reached,
and that header bits are as above (**↯/t.5.30**).

**Links.** The slots that can refer to a row — those of kinds `RECORD`, `CHILDREN`, `SELECT` and
`SELECT_LIST_REF` — are numbered `0, 1, …` by walking the tables in table order (§5.5) and each
table's slots in slot order. A slot's number is its **link number**; it determines the host table
of the parent. A schema with more than 65535 links cannot be laid out (**↯/t.5.11**).

### 6.3 Absence, Emptiness and Defaults

- An **absent** optional member has its presence bit clear. Its slot bytes MUST be zero when
  written and MUST be ignored when read; a verifying reader MAY report non-zero bytes there, or in
  any pad, as **↯/t.5.26**.
- An **empty** repeatable member has a count of `0`: a `FLAG_COUNT` of `0`, or a collection slot
  (`CHILDREN`, `ARRAY_REF`, `SELECT_LIST_REF`) holding the **empty ref**, NONE, or a ref to a
  growable item whose `count` is `0`. An owner MAY leave an emptied item in place, keeping its
  capacity for later insertions; a sealed file always uses the empty ref (**↯/t.5.28**, verification
  only). Absence (presence bit) and emptiness (count) are distinct and never conflated: a
  repeatable member is never absent, an optional non-repeatable member is never empty.
- An **empty text** is a value item of `length = 0`. BinTEL §7.5 applies verbatim: the encoding
  does not distinguish an explicitly empty value from a defaulted empty one, and for an encoded
  scalar an empty item is well-formed only if the codec accepts the empty byte sequence.
- A **defaulted** required Scalar (TEL §20, BinTEL §7.6) is stored as its default text's bytes,
  exactly like a written value.

## 7. Reading

### 7.1 Opening

```
open(bytes):
  if len(bytes) < 200: report ↯/t.5.4
  header = copy of bytes[0 .. 200]            // a mapped file may change beneath the reader (§11)
  check magic (↯/t.5.1), version (↯/t.5.2), reserved byte and flag bits (↯/t.5.3)
  check the flag combination (↯/t.5.28); refuse a dirty file (↯/t.5.29)
  check committed length (↯/t.5.4); check every section directory entry (↯/t.5.5)
  check that the sections do not overlap (↯/t.5.6)
  if flags.digests: verify §5.7 (↯/t.5.7)         // MAY be deferred; MUST precede trust
  read the schema section: signature (↯/t.5.8), body (↯/t.5.9), recomputed signature (↯/t.5.10)
  schema = compose(body)                      // TEL §20.3
  L = layout-document(schema)                 // and the links; ↯/t.5.11 if not possible
  check the table directory against L (↯/t.5.12); check table 0 and its root row (↯/t.5.13)
  check the index directory: M entries (§9.1), each well-formed (↯/t.5.23)
  check that no two extents — sections, tables, indexes — overlap (↯/t.5.6)
  if flags.sealed: check the directory-level compactness conditions of §5.8 (↯/t.5.28)
  check the codec binding resolves every encoding the schema names (↯/t.5.19)
  return Reader { bytes, schema, L }
```

Every condition in `open` is fatal: a reader MUST NOT return a `Reader` on any of ↯/t.5.1–↯/t.5.13,
↯/t.5.19, ↯/t.5.23, ↯/t.5.28 or ↯/t.5.29 raised there. After `open`, a reader needs no further
validation to serve accesses; what remains is the per-access bounds checking of §7.2. The overlap
check sorts the extents by offset, so it costs `O((N + M) log (N + M))`, independent of the number
of rows.

### 7.2 Access

With `L` in hand, every non-repeatable step is a constant number of loads:

- **Row address.** `row(T, r) = tables[T].offset + r·rowSize(T)`, after checking
  `r < tables[T].count` and that the row's live bit is set (**↯/t.5.15**).
- **Presence.** For a slot with presence bit `b`, test `bytes[row + 8 + (b >> 3)] & (1 << (b & 7))`.
- **Slot load.** Read the slot's `size` bytes at `row + slot.offset`.
- **Ref.** For a `REF`, or for an `ARRAY_REF`, `CHILDREN` or `SELECT_LIST_REF` other than the
  empty ref, resolve the `u32` against the slot's region: the ref MUST be a multiple of 4 with
  `ref + 4 ≤ length` (or `ref + 8 ≤ length` for a growable item), and the item's body as given
  by its header MUST end within the region's `length`, with `count ≤ capacity` (**↯/t.5.14**).
- **Children.** The `k`-th child of a `CHILDREN(R)` slot is the row named by `rows[k]` of its
  child list, for `k < count`; that record index is checked as a row address in `R`'s table
  (**↯/t.5.15**).
- **Record.** For a `RECORD(R)` or a Struct-variant payload, the record index is checked as a row
  address in `R`'s table (**↯/t.5.15**).
- **Tag.** A `SELECT` tag MUST be a keyword index of one of the member's variants (**↯/t.5.17**).
- **Value.** A text scalar's bytes MUST be valid UTF-8 (**↯/t.5.18**; the ↯/t.4.7 condition). An encoded
  scalar's bytes are given to the bound codec's `decode` (**↯/t.5.19** if the encoding is unresolved —
  normally caught at open; **↯/t.5.20** if the codec rejects the bytes, the ↯/t.4.14 condition; and
  OPTIONALLY **↯/t.5.21** if `encode(decode(b)) ≠ b`, the ↯/t.4.15 condition).

A **lazy** reader performs these checks at the access that needs them and reports ↯/t.5.14–↯/t.5.18,
↯/t.5.20–↯/t.5.21 and ↯/t.5.23 as failures of that access, continuing to serve others; a **verifying**
reader performs the full materialisation of §8, the index verification of §9.6, and the row-header
checks of §6.2 at open, and refuses the file on any failure. Both are conforming; which a tool
offers is its contract.

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
  if depth > 256: resource error (outside the ↯/t.5 codes, as BinTEL §11)
  row = row(T, r)                             // ↯/t.5.15: in range and live
  if visited[T][r]: report ↯/t.5.16                // the same row reached twice: aliasing or a cycle
  visited[T][r] = true
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
      CHILDREN(R):     for each r' of child-list(ref), in order:
                         out.push(Node(kidx(m), R, struct-children(tab(R), r', R.struct, depth + 1)))
      SELECT:          if required(m) or present(row, slot): out.push(variant(load tag, load payload))
      SELECT_LIST_REF: for each (tag, payload) of item(ref), in order: out.push(variant(tag, payload))
  return out

variant(tag, payload):
  V = the variant with keyword index tag (↯/t.5.17)
  switch resolved type of V:
    Struct R: return Node(tag, R, struct-children(tab(R), payload, R.struct, depth + 1))
    Scalar:   return Value(tag, decode(V, item(payload)))
    Flag:     return Flag(tag)

decode(m, bytes):
  if m's scalar has no encoding: UTF-8-decode(bytes)       // ↯/t.5.18
  else: codec-binding(encoding).decode(bytes)               // ↯/t.5.19, ↯/t.5.20; optional ↯/t.5.21
```

A collection slot holding the empty ref yields no elements; `item(ref)` and `child-list(ref)`
yield the first `count` elements of the item. `kidx(m)` is the member's keyword index in `S`'s
keyword order — for a Field its own, for a SelectRef the chosen variant's (the `tag`). Within one
member the children appear in **stored order** — child-list entries, array elements, list pairs —
which the writer took from the semantic model and every change of §10.3 maintains, so the
within-member order that property P2 (TEL §22.4) preserves is reproduced exactly; across members
the model's order is member order, which `members-of` follows. The result is therefore `≡` to the
model the file holds (§12).

Every live row is reached by the walk exactly once; a row reached twice (**↯/t.5.16**) is not
permitted, as it would make the materialised model larger than the file, or infinite. A live row
that the walk never reaches is **↯/t.5.30** for a verifying reader; free rows are never reached.

## 9. Indexes

### 9.1 Enumeration and Sites

Indexes are enumerated by the schema, in a fixed order, so that writer and reader agree on `M`
and on which index is which without storing names:

1. Walk the tables in table order (§5.5); within each table, walk its slots in slot order
   (§6.2).
2. Each `CHILDREN(R)` slot is a **site**. At a site, emit first the **primary index** if `R` has a
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
intermediate Struct member. The schema constraints ↯/t.2.28–↯/t.2.30 (TEL §20.1) guarantee this for a
valid composed schema; a reader finding otherwise reports **↯/t.5.11**.

### 9.2 Key Components

Index keys are compared **bytewise**, with no knowledge of the scalar types involved. This is
possible because text compares in code-point order under bytewise UTF-8 comparison — the exact
code-point equality that TEL's key uniqueness (↯/t.3.14, TEL §21.6) and TELP use — and because the
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

`hostRow` is the record index of the row holding the `CHILDREN` (or `ARRAY_REF`) slot — the parent
of the children indexed — and `childRow` is the record index, in `R`'s table, of the child the
entry names (for an `ARRAY_REF` site, the element's position in the array). Both are
**big-endian** so that bytewise order groups entries by parent and, within a parent, by key and
then by child; these are the only big-endian integers in RaTEL (§3). For a primary index `k = 1`
and the component is the child's key value, which is always present (it may be defaulted); for a
secondary index there is one component per declared path, resolved from the child row, absent
components encoded as `00`.

The entries of an index are pairwise distinct — by the `childRow` tiebreak — and are kept in
strictly increasing bytewise order (**↯/t.5.24**). Secondary indexes impose no uniqueness: two
children with equal secondary components are adjacent entries distinguished by `childRow`.

Because `childRow` is a record index, and record indices are stable until a repack (§3), an
index entry stays valid while its child's position in the child list changes; only a change to
the child's indexed values, or its removal, touches the index. For an `ARRAY_REF` site the
`childRow` is a *position*, so inserting, removing or reordering an element before the end of an
array rewrites the entries of the elements after it (§10.3).

### 9.4 B+tree Pages

An index of `kind = 0` is a **B+tree** whose entries are the byte strings of §9.3, ordered
bytewise. Its pages are `pageSize` bytes each, numbered within the index's extent (§5.6). Every
page begins with a 16-byte header:

| Offset | Size | Field |
| -----: | ---: | ----- |
| 0 | 1 | **Type**: `0` — free; `1` — leaf; `2` — branch. |
| 1 | 1 | Reserved; MUST be zero. |
| 2 | 2 | **Count** (`u16`): the number of cells in the page. |
| 4 | 4 | **Cell start** (`u32`): the lowest byte offset, within the page, of the cell area; `16 + 2·count ≤ cellStart ≤ pageSize`. |
| 8 | 4 | **Link** (`u32`): for a leaf, the page number of the next leaf in key order, or NONE for the last leaf; for a branch, the page number of its **leftmost child**; for a free page, the next page of the free-page list, or NONE. |
| 12 | 4 | Reserved; MUST be zero. |

The header is followed by `count` **cell pointers**, `u16` byte offsets within the page, in
increasing key order of the cells they point at. Cells occupy the cell area, `cellStart ..
pageSize`, in any order and with any gaps; each begins at a multiple of 4, is padded to a
multiple of 4, lies wholly within the cell area, and overlaps no other cell.

A **leaf cell** holds one entry, either inline or spilled:

```
inline:   u16 length            1 ≤ length ≤ pageSize / 8
          u8  entry[length]
spilled:  u16 0xFFFF
          u16 reserved          MUST be zero
          u32 ref               a value item in the variable region (§5.4) whose bytes are the entry
```

An entry is inline if its length is at most `pageSize / 8` and spilled otherwise; this rule is
exact, in both directions, so that every page holds at least six cells and a page split always
succeeds. A **branch cell** is `u32 child` followed by a **separator** in the same two forms, by
the same length rule. A branch with cells `(child₁, sep₁) … (childₙ, sepₙ)` and leftmost child
`child₀` has `n + 1` children, and satisfies `sep₁ < sep₂ < … < sepₙ`; every entry in the subtree
of `child₀` is `< sep₁`, and every entry in the subtree of `childᵢ` (`i ≥ 1`) is `≥ sepᵢ` and, for
`i < n`, `< sepᵢ₊₁`. A separator need not equal any entry.

The tree as a whole satisfies:

- the root is page `root`; every page reachable from it is reached exactly once; branches are at
  depths `0 .. height − 2` and leaves at depth `height − 1` exactly;
- following the leaves' links from the leftmost leaf visits every leaf once, in key order, and
  the last leaf's link is NONE;
- within a page, cells are in strictly increasing order of their entries or separators;
- the total number of leaf cells is the directory's `count`;
- pages that are not in the tree are on the free-page list, with type `0`; in a sealed file there
  are none.

No minimum occupancy is required: an owner MAY leave pages partly full, and a non-root page MAY
even be empty after deletions. Malformed page fields — an unknown type, a reserved byte, a cell
pointer or cell outside the cell area or misaligned, a length out of range, a spilled ref that
does not resolve (§5.4), a child or link that is not a page number below `pageCount` — are
**↯/t.5.23**; violations of the ordering and tree invariants above are **↯/t.5.24**.

### 9.5 Lookup

A lookup is a descent from the root followed by a walk along the leaves, comparing a **probe** —
a prefix of an entry — against entries and separators with one `memcmp` each, and no type
knowledge at search time:

```
find(index, hostRow, key₁, …, keⱼ):              // j ≤ k components given
  probe = u32BE(hostRow) ‖ component(key₁) ‖ … ‖ component(keyⱼ)
  if index.root == NONE: return []
  p = index.root
  repeat index.height − 1 times:                 // descend through the branches
    page = branch page p                          // ↯/t.5.23
    i = the number of separators ≤ probe          // binary search over the cells
    p = (i == 0) ? page.link : child of cell i
  leaf = leaf page p                              // ↯/t.5.23
  k = the first cell of leaf whose entry ≥ probe  // binary search; may be one past the end
  results = []
  loop:                                           // at most index.count matches, pageCount leaves
    while k < leaf.count:
      e = entry of cell k
      if e does not begin with probe: return results
      results.push(childRow of e)                  // its last four bytes, big-endian
      k += 1
    if leaf.link == NONE: return results
    leaf = leaf page leaf.link; k = 0
```

Comparison is bytewise lexicographic, with a proper prefix sorting first; an entry beginning with
the probe is therefore `≥` the probe, and the entries beginning with it are contiguous in key
order. A probe of `u32BE(hostRow)` alone enumerates a parent's children in key order; a full
primary probe returns zero or one child; a secondary probe returns a run. A spilled entry or
separator is loaded from the variable region for comparison. A reader MUST bounds-check every
page, cell pointer, cell and spilled ref it touches (**↯/t.5.23**), and MUST stop a leaf walk that
has visited more than `pageCount` pages (**↯/t.5.24**).

### 9.6 Verification

A verifying reader (§7.2) checks every index:

- every page reachable from the root, and every page on the free-page list, is well-formed
  (**↯/t.5.23**); each leaf entry parses — presence bytes are `00` or `01`, each variable component
  is properly escaped and terminated, each fixed component has its width, and the entry ends
  exactly after `childRow` (**↯/t.5.23**);
- the tree invariants of §9.4 hold, and entries are strictly increasing along the leaf chain
  (**↯/t.5.24**);
- each entry agrees with the rows: `hostRow` is a live row of the site's host table, `childRow`
  appears in the child list that `hostRow`'s slot refers to (for an `ARRAY_REF` site, is below the
  array's `count`), the components recomputed from the child equal the stored bytes, and the
  index's `count` equals the total number of children at the site over all host rows
  (**↯/t.5.25**).

### 9.7 Maintenance (Informative)

An owner keeps every index in step with the rows as it changes them (§10.3), by the usual B+tree
procedures, which this specification constrains only through the invariants of §9.4:

- **Insert.** Descend as `find` does for the whole entry, to the leaf where it belongs, and add a
  cell and a cell pointer. If the leaf lacks room, first compact its cell area — gaps left by
  deleted cells are reusable — and if it still lacks room, **split** it: take a page from the
  free-page list or the extent's slack (relocating the extent if needed, §10.2), move the upper
  half of the cells by bytes to it, link it after the old leaf, and insert into the parent a
  separator equal to the new page's first entry. A full parent splits likewise; a split root
  becomes the leftmost child of a new root branch, and `height` grows by one.
- **Delete.** Find the entry with a full probe, then remove its cell pointer; its bytes become a
  gap, and a spilled entry's item becomes garbage. Separators are left as they are — the
  invariants of §9.4 still hold. An owner MAY merge or redistribute underfull pages, and SHOULD
  free a non-root leaf that becomes empty: unlink it from the leaf chain, remove its separator
  from the parent, and push it onto the free-page list.
- **Bulk load.** Given entries in sorted order, fill leaves left to right, each with as many
  cells as fit, then build each branch level over the one below, using each child's first entry
  (other than the leftmost child's) as its separator, until one page remains. `repack` and
  `seal` build every index this way (§10.5).

Insert and delete each touch `O(height)` pages, `O(log n)` in the number of entries.

## 10. Writing and Mutation

A RaTEL file is created from a semantic model, changed in place by an owner through the
operations of TEL §22.2, and reorganised by `repack` and `seal`, which write a new file. In all of
these, normatively, a writer or owner:

- MUST hold only valid documents (TEL §21.9), and MUST be configured with a codec binding
  resolving every encoding the schema names (TEL §21.7), failing without output otherwise — as a
  BinTEL encoder does (BinTEL §7.1);
- MUST refuse any value of a `width w` scalar that encodes to other than `w` bytes
  (**↯/t.5.22**);
- MUST compute every layout and every link number by §6.2, store every row by §6, every item by
  §5.3–§5.4, every index by §9, and zero every pad byte, every absent member's slot, and every
  row and page it allocates before filling it;
- MUST store every repeatable member's elements in semantic order, and keep every row header,
  free list and index consistent with the rows at the end of each operation.

### 10.1 Ownership and the Dirty Flag

A file has at most one owner at a time. To become the owner, a process MUST take an **exclusive
lock** on the file (an operating-system advisory lock, such as `flock` with `LOCK_EX`) and hold it
until it closes the file; a reader of an unsealed file SHOULD take a shared lock, so that the two
exclude each other. Then:

1. The owner refuses a file that is already dirty (it was not closed cleanly, and its contents
   are unspecified), and a sealed file unless it first **unseals** it — clearing flag bits 0, 1
   and 2 and the value hash, setting the digests section's length and capacity to `0`, and adding
   the capacity it had to the garbage count — which it MUST NOT do to a file that other processes
   may have mapped. A publisher changes a published
   file by opening a copy.
2. Before its first change, the owner sets the dirty bit and makes the header durable (for
   instance with `msync` of the header page).
3. On a clean close, the owner makes every change durable, then clears the dirty bit and makes
   the header durable again, then releases the lock.

A crash between steps 2 and 3 leaves the file dirty. RaTEL version 1 defines no recovery: a dirty
file's last sealed or repacked copy is its last durable state. An owner MAY nevertheless attempt
to salvage a dirty file by verifying it (§7.2) and repacking it.

### 10.2 Allocation and Growth

An owner allocates without moving anything wherever it can, and relocates by doubling where it
cannot, so that every allocation costs amortised constant time:

- **Rows.** A row is taken from the table's free list if it is non-empty, else at `count` if
  `count < capacity`, else after relocating the table's extent to one of at least twice the
  capacity (and at least one row). It is zeroed, its header written, and its live bit set.
  Freeing a row zeroes it, pushes it onto the free list, and adds nothing to garbage.
- **Items.** An item is appended at the region's `length`, which advances by the item's padded
  size; a region without room is relocated to one of at least twice the capacity (subject to the
  `2^32`-byte bound of §3). Items are never freed individually: an abandoned item's bytes are
  added to the header's garbage count.
- **Growable items.** An element is appended in place while `count < capacity`; a full item is
  copied to a new item of at least twice the capacity (and at least one element), the referring
  slot is pointed at the copy, and the old item is abandoned.
- **Pages.** A page is taken from the free-page list, else at `pageCount`, else after relocating
  the index's extent to one of at least twice the page capacity.
- **Relocation.** A relocated extent is placed at the committed length, which advances past it;
  its directory entry is rewritten, and its old bytes are added to the garbage count. If the file
  is too short, the owner first extends it.

**Reserve.** `expand(n)` extends the file by `n` bytes of reserve beyond the committed length, and
re-maps it. An owner MAY call it implicitly when a relocation needs room, and an application MAY
call it explicitly to reserve space ahead of a burst of insertions; mapping a large, sparse
reserve up front makes re-mapping rare. Because a relocation changes absolute addresses, an
implementation MUST NOT hold a raw address into the mapping across a change; record indices, refs
and page numbers remain valid.

### 10.3 Changes

The changes an owner makes are the **semantic** machine operations of TEL §22.2, addressed as
there or by a record index, a site and a position. Each is applied as follows:

| Operation (TEL §22.2) | Effect on a RaTEL file |
| --- | --- |
| `update-value` | For an `INLINE` slot, the bytes are overwritten in place. For a `REF` slot or array element, a new value item is appended (or, for an interned scalar, an existing equal one MAY be reused), the slot or element is pointed at it, and the old item, if in the variable region, is abandoned. If the value is a key or lies on a secondary-index path (below), the affected index entries are deleted before and inserted after. |
| `set-flag`, `unset-flag` | The presence bit is set or cleared, or a `FLAG_COUNT` incremented or decremented. |
| `insert`, `insert-before`, `insert-after`, `insert-into-block` (with `construct`) | The new element's rows are allocated and filled, depth first, with their row headers' parent and link; its values are appended; it is spliced into its parent's child list, array or select list at its semantic position (or, for a non-repeatable member, written into its slot); and an entry is inserted into every index whose site it, or anything inside it, joins. |
| `delete` | The element's subtree is walked: every row in it is freed, every item it alone refers to is abandoned, and every index entry at every site within it is deleted. The element is then removed from its parent's list (or its slot cleared and its presence bit unset), and the entry for it deleted from the indexes at its own site. |
| `replace` | A `delete` followed by an `insert` at the same position, validated as one operation. A change of variant of a SelectRef is a `replace`. |
| `reorder-within-group` | Entries are moved within a child list, array or select list. Indexes at a `CHILDREN` site are unaffected; at an `ARRAY_REF` site, the entries of the elements whose positions changed are rewritten. |
| `reorder-groups`, `attach-remark`, `remove-remark`, `resize-tabulation` | Presentation only: across members the semantic model's order is member order (§8), and remarks and tabulation are not in the semantic model, so these leave a RaTEL file unchanged. |

**Index maintenance.** When a value in row `X` changes, the owner finds the affected index
entries by walking up from `X` through row-header parent links while the link is a `RECORD` slot
(a non-repeatable Struct member, through which a secondary-index path may pass), stopping at the
first `CHILDREN` link: if row `Y` is reached through a `CHILDREN` link at site `s` with host row `h`,
and an index at `s` has a key or path that leads from `Y`'s type to the changed member, the owner
recomputes `Y`'s entry for that index before and after the change. Paths never pass through
repeatable members or Select variants (§9.1), so no other entry can be affected.

**Validation before writing.** An operation MUST be validated in full — the preconditions of TEL
§22.2 (required members, `repeatable` and `required` constraints of flags), key uniqueness by a
probe of the primary index (↯/t.3.14), patterns (↯/t.3.15), codecs (↯/t.3.12), validators
(↯/t.3.10) and `width` (↯/t.5.22) — before the owner writes any byte. A rejected operation leaves
the file unchanged and is reported with the diagnostic that TEL §22.2 prescribes. A named
validator of a Struct (TEL §21.3) is given the element it validates; an owner MAY materialise that
element alone (§8) to call it. A failure *after* validation — exhausted reserve that cannot be
expanded, an I/O error — leaves the file dirty and the session's changes unspecified.

### 10.4 Creating a File

An owner creates a file from a valid semantic model and its composed schema exactly as `repack`
does (§10.5), with a slack factor of its choosing; the model may be as small as the schema
allows. It then holds the file dirty, as after step 2 of §10.1.

### 10.5 Repack and Seal

`repack` and `seal` both write a **new file** holding the same semantic model, walking the old one
from the root as materialisation does (§8) and allocating as it goes; the old file is not
changed, so an interruption leaves it intact. The procedure, with a **slack factor** `f ≥ 1`:

1. Compute all layouts and links. Keep a row cursor per table; allocate the root row.
2. `emit(element, T, r)`: write `r`'s row header, then fill `r`'s slots in slot order, appending
   items to the variable region (or interning into the string table, first occurrence first) as
   they are met. For a `CHILDREN(R)` slot with `n > 0` children, **reserve** rows
   `cursor[R] .. cursor[R] + n` and advance the cursor *before* descending, so that siblings are
   contiguous whatever the children allocate, and append their child list. An empty repeatable
   member gets the empty ref. For each `RECORD(R)` slot and Struct-variant payload, allocate one
   row. Then recurse into every allocated child row in slot order. What a slot refers to is
   appended before the item that refers to it: for a ref array, the value item of each element,
   in order, and then the array; for a select list, the value item or the row of each filling,
   in order, and then the list.
3. Build each index by computing its entries from the finished rows, sorting them, and bulk
   loading them (§9.7), the indexes in index order. The pages of an index are numbered from `0`
   in the order they are filled: its leaves from left to right, then each level of branches
   over the level below, likewise, so that the root is the last page. An entry or separator
   too long for its page (§9.4) is spilled as its page is filled, so the spilled items follow
   every item of step 2 in the variable region, in the order of the cells that refer to them.
4. Lay out the extents in this order: schema, strings, variable, the tables directory, the rows
   of each table in table order, the indexes directory, the pages of each index in index order,
   digests. Each begins at the next multiple of 8, and each growable extent, and each growable
   item, gets `f` times the capacity it needs, rounded up (a table and an index to whole rows and
   pages, a section to a multiple of 8).
5. Fill the header: zero garbage, the committed length after the last extent.

`repack(f)` leaves the new file unsealed and clean, with `f > 1` for room to grow. It reclaims all
garbage, free rows and free pages, and **renumbers rows**: handles into the old file are not
valid in the new one, and an implementation SHOULD offer the mapping from old to new record
indices to applications that hold handles. An owner SHOULD repack when the garbage count
exceeds a fraction of the committed length of its choosing (one half, say); since a repack costs
time linear in the live data, and that much garbage takes at least as much work to produce,
the amortised cost of repacking is constant per byte changed.

`seal()` is the procedure with `f = 1` and 4096-byte pages (`pageShift = 12`), and then sets flag
bit 2; it MAY compute the value hash
of the materialised document (BinTEL §3) into the header and set flag bit 1, and MAY append the
digests section (§5.7) and set flag bit 0, computing the header digest last. Seal is
**canonical**: for a given semantic model and schema its output is a function of the model alone,
whatever the history of the file it was read from, and this specification's test vectors are
sealed files. A publisher writes the sealed file beside the old version, makes it durable, and
renames it into place.

### 10.6 Costs (Informative)

| Operation | Cost |
| --- | --- |
| Field access | `O(1)`: a row address and a slot load |
| `k`-th child, element or select filling | `O(1)`: one more load, through the list |
| Keyed lookup | `O(log n)` page visits, plus `O(m)` for `m` matches |
| Append a row, a child, an element | amortised `O(1)` |
| Insert or delete before the end of a list of `n` | `O(n)` moves of 4- or 8-byte elements |
| Write a value of `b` bytes | `O(b)` |
| Insert or delete an index entry | `O(log n)` |
| Delete a subtree | linear in its rows, plus `O(log n)` per index entry within it |
| `repack`, `seal` | linear in the live data, plus `O(n log n)` to sort each index |

## 11. Security Considerations

A RaTEL reader works on memory it did not write and MUST treat every integer it loads as
adversarial.

**Bounds.** Every offset, length, capacity, count, ref, record index, page number, cell pointer
and tag is checked before use (§7.2, §9.5); arithmetic on `offset + capacity·rowSize`, `offset +
pageCapacity·pageSize` and `ref + 8 + capacity·w` is performed in 64 bits so that it cannot wrap.
A reader MUST NOT allocate memory proportional to a count before the count has been bounded by
the file.

**Termination.** Materialisation (§8) is bounded by the per-table visited bitmaps — each row is
visited at most once, so the work is linear in the number of rows — and by the depth limit of
256, matching BinTEL §11 and TEL §20.2; exceeding either is a resource error outside the ↯/t.5
taxonomy. An index descent visits exactly `height` pages, and a leaf walk is cut off after
`pageCount` pages (§9.5), so a cyclic leaf chain cannot hang a reader. Readers never follow free
lists.

**The file may change beneath the reader.** Version 1 provides no way for a reader to observe a
file while its owner changes it: the owner's exclusive lock (§10.1) and the dirty flag (§5.8)
exclude readers of unsealed files, and sealed files are never changed in place — a new version is
written beside a published one and renamed over it, and a reader keeps its mapping of the old
inode. But locks are advisory, and a reader cannot verify the discipline. A reader MUST copy the
header before acting on it (§7.1), MUST NOT assume that a value loaded twice is the same value,
and MUST tolerate a bounds check that passed once failing later.

**The owner trusts its own file.** An owner changes a file in place on the strength of its
structures — free lists, parent links, capacities — which a reader never follows. An owner MUST
NOT open for changing a file whose provenance it does not trust without first verifying it in
full (§7.2), and SHOULD instead repack such a file into one of its own.

**The schema is executable structure.** As in BinTEL §6.2, the embedded schema decides how every
other byte is interpreted and arrives from the same source. ↯/t.5.10 confirms only that the body is
consistent with its signature. A reader that requires a trusted schema MUST compare the verified
signature against the signatures it trusts, and MUST NOT infer trust from successful decoding.

**Digests and the value hash are not authenticity.** They detect accidental corruption and
identify content; a forger can recompute both. Authenticity, where needed, is a separate layer —
for a published file, the publisher's lock and the directory's ownership.

**Codecs run on attacker bytes.** As BinTEL §11: a codec's `decode` is invoked on value bytes
taken from the file and MUST handle malformed input safely; ↯/t.5.21 is a hardening check, not a
substitute.

## 12. Round-Trip and Conversion

**P5 (RaTEL losslessness).** For every valid semantic model `M` under composed schema `S` with
codec binding `B`, every conforming writer `ratel-encode`, and every sequence of changes `C`
(§10.3) that a conforming owner accepts,

```
materialise(open(ratel-encode(M, S, B)))   ≡ M
materialise(open(apply(C, ratel-encode(M, S, B))))   ≡ apply(C, M)
materialise(open(repack(F)))   ≡   materialise(open(seal(F)))   ≡   materialise(open(F))
```

where `≡` is semantic-model equality (TEL §22.4), `apply(C, M)` applies the same operations to the
semantic model under TEL §22.2, and `F` is any clean RaTEL file. Consequently
`value-hash(bintel-encode(materialise(F)))` is well defined for any valid RaTEL file `F`, is the
same for every RaTEL file holding `M`, and — when `F`'s header carries a value hash — MUST equal
it; a verifying reader MAY check this (**↯/t.5.27**). The value hash, not the file's bytes, is a RaTEL
document's identity.

**Conversion.** Text TEL, BinTEL and RaTEL are three encodings of one model. Converting between
any two is: obtain the semantic model (TEL §18.3 from text, BinTEL §7.8 from BinTEL, §8 from
RaTEL), then encode (TEL §22.3 canonical text, BinTEL §7, `seal` of §10.5 here). Every direction is
lossless in the semantic model; presentation detail — comments, atom forms, tabulation — is not in
any of the binary encodings (BinTEL §7.8). A tool that inspects TEL documents SHOULD inspect all
three: dump a RaTEL file's layout, rows and indexes; look up a record by key; convert among the
forms.

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
  info  First letter

entry beta
```

The two spaces after `info` are a hard space, which makes `First letter` one atom: with a
single space, `First` and `letter` would be two.

**Layouts.** Table 0 (root): one member, `entry`, repeatable Struct → `CHILDREN(Entry)`; no
presence bits; after the 8-byte row header, `entry` is at 8; `rowSize = round8(12) = 16`. Table 1
(`Entry`): `name` is a required variable-width scalar → `REF`, no bit; `info` is optional → `REF`,
presence bit 0. `bitmapBytes = 1`, at byte 8; after the align-4 pass `name` is at 12 and `info` at
16; `rowSize = round8(20) = 24`. **Links**: the only slot that refers to rows is the root's
`entry`, link 0. **Indexes**: one site (table 0, path `[0]`); `Entry` has a key field → `M = 1`, a
primary index on `name`.

The sealed file, as `seal` (§10.5) writes it, with a 4096-byte page and no value hash or digests,
follows. Let `L_s` be the schema section's length (below) and `A = 200 + L_s`.

**Variable region** (offset `A`, length 52, capacity 56):

```
00: 02 00 00 00  02 00 00 00  00 00 00 00  01 00 00 00     child list: capacity 2, count 2, rows 0, 1
10: 05 00 00 00  61 6C 70 68 61 00 00 00                   "alpha"         (pad 3)
1C: 0C 00 00 00  46 69 72 73 74 20 6C 65 74 74 65 72       "First letter"
2C: 04 00 00 00  62 65 74 61                               "beta"
34: 00 00 00 00                                            capacity pad
```

The root's child list is met first, when the root's slot is filled; then each child's values, in
row order.

**Tables section** (offset `A + 56`, length and capacity 64), then the rows of each table:

| Table | offset | count | capacity | rowSize | freeHead | layout |
| ----: | ------ | ----: | -------: | ------: | -------- | -----: |
| 0 | `A + 120` | 1 | 1 | 16 | NONE | 0 |
| 1 | `A + 136` | 2 | 2 | 24 | NONE | 0 |

```
A+120:  FF FF FF FF  FF FF  01 00  00 00 00 00  00 00 00 00
          root: parent NONE, link FFFF, live; entry → child list at 0x00; pad
A+136:  00 00 00 00  00 00  01 00  01 00 00 00  10 00 00 00  1C 00 00 00  00 00 00 00
          Entry 0: parent 0, link 0, live; bitmap 01 (info present); name → 0x10; info → 0x1C
A+160:  00 00 00 00  00 00  01 00  00 00 00 00  2C 00 00 00  00 00 00 00  00 00 00 00
          Entry 1: parent 0, link 0, live; bitmap 00; name → 0x2C; info absent (zero)
```

**Indexes section** (offset `A + 184`, length and capacity 32): index 0 has offset `A + 216`,
`pageCount = pageCapacity = 1`, `root = 0`, `freeHead = NONE`, `count = 2`, `kind = 0`,
`pageShift = 12`, `height = 1`.

**Index 0, page 0** (offset `A + 216`, 4096 bytes; all bytes not shown are zero): a leaf holding
both entries, bulk-loaded with cells placed from the end of the page downwards in key order.

```
000: 01 00 02 00  D8 0F 00 00  FF FF FF FF  00 00 00 00    leaf, 2 cells, cellStart 0xFD8, last leaf
010: EC 0F  D8 0F                                          cell pointers, in key order
FD8: 0F 00  00 00 00 00  01 62 65 74 61 00 00  00 00 00 01  00 00 00
                                                           15 bytes: host 0 | 01 "beta" 00 00 | child 1
FEC: 10 00  00 00 00 00  01 61 6C 70 68 61 00 00  00 00 00 00  00 00
                                                           16 bytes: host 0 | 01 "alpha" 00 00 | child 0
```

**Schema section**: the 33-byte signature of `catalogue` preceded by its length byte `21`, then
the varint length and bytes of the schema body under `tels`, padded to a multiple of 8 — `L_s`
bytes in all, whose exact value depends on the body's encoding and is pinned by the reference
implementation's vectors.

**Header**: `B1 C4 B5 BB 01 00 04 00` (flags: sealed); committed length `A + 4312 = 4512 + L_s`;
section directory as `(offset, length, capacity)`: schema `(200, L_s, L_s)`, strings `(A, 0, 0)`,
variable `(A, 52, 56)`, tables `(A + 56, 64, 64)`, indexes `(A + 184, 32, 32)`, digests
`(A + 4312, 0, 0)`; garbage `0`; value hash zeros (or flag bit 1 and the document's BLAKE3-256
value hash).

**Lookup of `beta`**: probe `00 00 00 00 01 62 65 74 61 00 00`. The index's height is 1, so the
root is the leaf. A binary search over its two cells finds the `alpha` entry below the probe and
the `beta` entry beginning with it, so `k = 1`; the entry's last four bytes give child row 1, the
next cell does not exist and the leaf's link is NONE, so the lookup returns `[1]`. Row 1 of table 1
is at `A + 136 + 1·24`, where `name`'s ref `0x2C` finds `beta` and the clear presence bit says
`info` is absent.

**A history of changes (informative).** An owner might reach the same document like this, from a
file created (§10.4) with the one entry it must have, `entry beta` — `entry` is required, so a
document with none is invalid (TEL ↯/t.3.7), and is never held:

1. The file is created with `entry beta`: Entry row 0, with parent 0 and link 0; the item
   `"beta"`; a child list of capacity 1 holding `[0]`, which the root's slot refers to; and a
   primary index of one page, a leaf, with the entry `host 0 | "beta" | child 0`.
2. `insert-before` beta, `entry alpha`: row 1; `"alpha"` appended; the full child list is copied
   to one of capacity 2 holding `[1, 0]`, and the old list (12 bytes) becomes garbage; the entry
   `host 0 | "alpha" | child 1` is inserted *before* beta's in the leaf, by key.
3. `update-value`, setting alpha's `info` to `First letter`: the item is appended, alpha's `info`
   slot pointed at it, and its presence bit set. `info` is not indexed, so no index changes.
4. `insert-after` beta, `entry gamma`, then `delete` it: gamma takes row 2 and a list of capacity
   4 holding `[1, 0, 2]`, and an index entry; the delete frees row 2 onto table 1's free list,
   leaves the list `[1, 0]` with capacity 4, removes the index entry, and abandons `"gamma"`.

The file now holds the example document, but with slack, a free row, garbage, and alpha at row 1.
`seal` walks it in semantic order — alpha first — and writes exactly the sealed bytes above, with
alpha at row 0.

## 14. Reader Error Handling

RaTEL's codes are the family **↯/t.5** of TEL's bolt codes (TEL §19.3), beside the ↯/t.1, ↯/t.2 and
↯/t.3 of the TEL Specification and the ↯/t.4 of BinTEL, though several restate the condition of a
↯/t.4 code.

| Code     | Condition                                                                                                                                                                                                                       | When                       |
| -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------- |
| ↯/t.5.1  | Magic number is not `B1 C4 B5 BB`                                                                                                                                                                                               | open                       |
| ↯/t.5.2  | Unsupported version byte                                                                                                                                                                                                        | open                       |
| ↯/t.5.3  | Reserved header byte or reserved flag bit non-zero                                                                                                                                                                              | open                       |
| ↯/t.5.4  | File shorter than 200 bytes; committed length `< 200`, greater than the file, or not a multiple of 8                                                                                                                            | open                       |
| ↯/t.5.5  | A section directory entry is misaligned, has `length > capacity` or a region `length` that is not a multiple of 4, has an offset `< 200`, extends beyond the committed length, or (for a fixed-size section) has `length ≠ capacity`                                            | open                       |
| ↯/t.5.6  | Two non-empty extents overlap, or one overlaps the header                                                                                                                                                                                                   | open                       |
| ↯/t.5.7  | A digest does not match; the digests section's length is not `64`, or it is not the last extent (flag bit 0 set); or flag bit 0 is clear and the digests section's capacity is not `0`                                          | open / verify              |
| ↯/t.5.8  | Schema signature structurally invalid — the ↯/t.4.3 conditions                                                                                                                                                                  | open                       |
| ↯/t.5.9  | Embedded schema body does not decode under `tels` — the ↯/t.4.12 condition                                                                                                                                                      | open                       |
| ↯/t.5.10 | Recomputed composed signature differs from the carried signature — the ↯/t.4.11 condition                                                                                                                                       | open                       |
| ↯/t.5.11 | The schema cannot be laid out: unresolvable `Reference`/`SelectRef` (the ↯/t.4.10 condition), `intern` on a fixed-width scalar, a secondary-index path that does not resolve per §9.1, more than 65535 links                   | open (configuration)       |
| ↯/t.5.12 | Table directory inconsistent with the layout or malformed: wrong section length or `rowSize`, unknown `layout` byte, non-zero reserved bytes, misaligned offset, `count > capacity`, bad `freeHead`, rows beyond the committed length | open                       |
| ↯/t.5.13 | Table 0 does not have exactly one row, has a free list, or its row is not live                                                                                                                                                  | open                       |
| ↯/t.5.14 | A ref is not a multiple of 4; an item's header or body extends beyond its region's length; a growable item has `count > capacity`                                                                                              | access / verify            |
| ↯/t.5.15 | A record index is `≥` its table's count, or names a free row                                                                                                                                                                    | access / verify            |
| ↯/t.5.16 | A row reached twice during materialisation — aliasing or a cycle                                                                                                                                                                | materialise                |
| ↯/t.5.17 | A select tag is not the keyword index of one of the member's variants                                                                                                                                                           | access / verify            |
| ↯/t.5.18 | A text scalar's bytes are not valid UTF-8 — the ↯/t.4.7 condition                                                                                                                                                               | access / verify            |
| ↯/t.5.19 | An encoding named by the schema is not resolved by the codec binding — the ↯/t.4.13 condition                                                                                                                                   | open                       |
| ↯/t.5.20 | A codec's decoder rejects a value's bytes — the ↯/t.4.14 condition                                                                                                                                                              | access / verify            |
| ↯/t.5.21 | OPTIONAL re-encode check found `encode(decode(b)) ≠ b` — the ↯/t.4.15 condition                                                                                                                                                 | access / verify            |
| ↯/t.5.22 | *Writer or owner:* a value of a `width w` scalar encodes to other than `w` bytes; the writer refuses. Never a reader condition                                                                                                  | write / change             |
| ↯/t.5.23 | Index directory, page or entry malformed: unknown kind, bad page size, entry count differing from `M`, non-zero reserved bytes, page fields out of range (§5.6, §9.4), an entry that does not parse                           | open / lookup / verify     |
| ↯/t.5.24 | An index violates the B+tree invariants of §9.4: entries or separators out of order, leaves at unequal depths, a page reached twice, a broken or cyclic leaf chain                                                             | lookup / verify            |
| ↯/t.5.25 | An index entry disagrees with the rows: host or child not live, child not in the host's list, recomputed components differ, or the count differs from the children at the site                                                 | verify                     |
| ↯/t.5.26 | Non-zero bytes in an absent member's slot or in padding                                                                                                                                                                         | verify (optional)          |
| ↯/t.5.27 | The header's value hash differs from the BLAKE3-256 value hash of the materialised document                                                                                                                                     | verify (optional)          |
| ↯/t.5.28 | Invalid state: flag bit 0 or 1 set without bit 2, or bits 2 and 3 both set; or a sealed file that is not compact (§5.8)                                                                                                         | open / verify              |
| ↯/t.5.29 | The file is dirty                                                                                                                                                                                                               | open                       |
| ↯/t.5.30 | A row header is inconsistent: reserved flag bits set, a live row's parent or link not naming the slot that reaches it, or a live row that is never reached                                                                     | verify                     |

↯/t.5.1–↯/t.5.13, ↯/t.5.19 and ↯/t.5.29 are **open-time** conditions, as are ↯/t.5.23 and ↯/t.5.28 when
`open` finds them: a reader MUST report them and MUST NOT return a reader. ↯/t.5.14–↯/t.5.18,
↯/t.5.20, ↯/t.5.21, ↯/t.5.23 and ↯/t.5.24 arise on access; they are fatal for the access that
encounters them, and a lazy reader MAY continue to serve other accesses while a verifying reader
rejects the file. ↯/t.5.25–↯/t.5.27 and ↯/t.5.30 arise only in verification. Resource limits (depth,
memory) are outside the taxonomy, as in BinTEL §11.

**Precedence.** Open-time checks are performed in the order of §7.1. Within one slot access, a
record index is checked (↯/t.5.15) before a ref (↯/t.5.14), a tag (↯/t.5.17) before its payload,
and the value's bytes (↯/t.5.18/↯/t.5.20) last; within one index access, a page's fields
(↯/t.5.23) before its order (↯/t.5.24). Where a verifying reader could report several codes for one
defect, it MUST report the first in this order.

## 15. Future Work (Informative)

The following are anticipated and provided for, but not specified:

- **Concurrent readers of a changing file**, by publishing each change behind a generation
  counter in the header and deferring the reuse of relocated extents, freed rows and freed pages
  until no reader can still see them.
- **Crash-safe changes**, by a write-ahead journal or by shadow paging, so that an interrupted
  session leaves the file at its last clean state rather than dirty.
- **Column-major tables**, under the reserved `layout` byte (§5.5), for scans over one field.
- **Hashed indexes**, under the reserved index `kind` (§5.6), for pure point lookups where a
  B+tree's `log n` page visits matter.
- **Wide refs** — `u64` refs for regions beyond 4 GiB — under a reserved `layout` value.
- **Indexes over Select-variant instances** (§9.1).
