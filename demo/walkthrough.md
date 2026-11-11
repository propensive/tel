# Worked example: a single TEL document, end-to-end

This walkthrough takes one very small TEL document, shows its presentation
model (§18 of the TEL Specification), its semantic model after type
assignment against a schema (§20.2), and its BinTEL encoding (§7 of the
BinTEL Specification). The aim is to make each layer concrete in a way that
implementors can verify against their own tooling.

## 1. Schema

```tel
tel 1.0 specification.tel/tels:2.1.0

name greeting

document
  field text String
  field bold Flag optional
```

This schema declares a document with two members:

- `text`, a required scalar of the built-in type `String` (validated by the
  built-in `string` validator — any value is accepted). Required is the
  default for every Field; `optional` loosens it.
- `bold`, an optional flag (explicit `optional`).

A `field`'s type is its second inline atom, a `TypeName` (§20.5): `String`
and `Flag` are two of the five predefined names, alongside `Identifier`,
`Sigil` and `TypeName` itself.

## 2. The TEL document

```tel
tel 1.0

text  hello, world
bold
```

The pragma omits the schema identifier — in real use it would carry the
schema's BASE-256-encoded value hash. The body has two compounds.

## 3. Presentation model (§18)

The parser produces, roughly:

```
Document {
  pragma: Pragma { version: (1, 0), schema: None, sigil: None },
  children: [
    Block {
      compounds: [
        Compound { keyword: "text",
                   atoms: [Inline { text: "hello, world", preceding_spaces: 2 }],
                   children: [] },
        Compound { keyword: "bold",
                   atoms: [], children: [] },
      ],
    },
  ],
}
```

The hard-space (two preceding spaces) on the `text` line puts the value
phrase into hard-space mode, so `hello, world` is a single atom.

## 4. Semantic model (after type assignment, §20.2)

- The root compound is typed by `Schema.document`.
- The first child compound's keyword is `text`, which maps to keyword
  index 0, a `Field` whose type is `Scalar { validator: "string" }`. Its
  inline atom is the field's value.
- The second child's keyword is `bold`, keyword index 1, a `Field` whose
  type is `Flag`. The compound has no value content.

No errors arise; the document is valid.

## 5. BinTEL document root encoding (§7)

The document root has two children:

| Bytes (hex)                              | Meaning                                |
| ---------------------------------------- | -------------------------------------- |
| `02`                                     | child_count: 2 (varint)                |
| `00`                                     | child #1 keyword index: 0 (`text`)     |
| `0c`                                     | value length: 12 (varint)              |
| `68 65 6c 6c 6f 2c 20 77 6f 72 6c 64`    | UTF-8 of `hello, world`                |
| `01`                                     | child #2 keyword index: 1 (`bold`)     |

The `bold` Flag has no value bytes; the keyword index alone represents it.
Total: 16 bytes. (The reference Rust implementation has a regression test
that pins these exact bytes; see `walkthrough_example_encodes_as_expected`
in `ref/tel/src/lib.rs`.)

## 6. Value hash (§3)

The 256-bit BLAKE3 digest of the 16-byte sequence above is the **value hash**
of this semantic model. Two implementations that produce different
presentation encodings of the same semantic content (for example, putting
`bold` before `text`, or using a literal atom for the value) MUST produce
the same value hash.

## 7. Complete BinTEL document (§6)

The complete byte stream is:

```
B2 C4 B5 BB                     # magic number (external-schema mode, "βτελ")
32                              # document length: 50 bytes follow (varint)
21 <signature bytes…>           # signature: length 33 (varint 0x21) + bytes
02 00 0c …                      # document root (as above)
```

The magic number is four bytes and identifies the mode: `B2 C4 B5 BB` for
external-schema mode (§6.1), `B2 C4 B5 BC` for self-contained mode (§6.2),
which carries the schema body inline. Rendered as BASE-256 text the two read
`βτελ` and `βτεμ`.

The **document length** counts everything after itself — here 1 byte of
signature length, 33 of signature and 16 of document root, so 50 (`0x32`).
It is what makes the document self-framing: a reader knows where it ends
without decoding it, and therefore where the *continuation* begins (§6.3).
That matters because finding the end structurally would require the composed
schema — keyword indices determine each node's shape — so without the length
a reader that cannot resolve the schema could not even skip the document.
With it, framing costs four bytes plus a varint and needs no schema at all.

The signature for a no-layer schema is a 33-byte palimpsest: the schema's
32-byte BLAKE3-256 value hash followed by a one-byte cadence trailer
(see BinTEL §8). The pragma's schema identifier carries the same 33 bytes
encoded as 33 BASE-256 characters.

## Multi-document streams

A source may hold a sequence of independent documents separated by a
*document separator* — a line of exactly two sigil characters, `##` by default
(§6.1 of the TEL Specification). [`document-stream.tel`](document-stream.tel)
carries three such documents.

There are two ways to read it:

- **Single-document parsing** reads the first document and stops at the first
  `##`. Everything after the separator is left unparsed, which is how a TEL
  document can serve as a header for arbitrary trailing content.
- **Streaming parsing** yields all three documents in order, each parsed
  independently with its own pragma, sigil, and margin. A trailing separator
  yields no empty final document; a separator inside a literal-atom payload is
  preserved verbatim rather than splitting the document.

## See also

- [`document-stream.tel`](document-stream.tel) — three independent documents
  in one source, separated by `##`.
- [`contact-schema.tel`](contact-schema.tel) — a larger example showing
  `record` definitions, references to them by `TypeName`, and a `select`
  whose variants are all `Flag`.
- [`contact-document.tel`](contact-document.tel) — a document conforming
  to that schema, with hard-space multi-token values.
- [`tels.bintel.hex`](tels.bintel.hex) — the BinTEL document
  root encoding of `tels.tel`, whose BLAKE3-256 hash is normatively
  pinned in §20.5 of the TEL Specification.
