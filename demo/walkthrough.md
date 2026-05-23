# Worked example: a single TEL document, end-to-end

This walkthrough takes one very small TEL document, shows its presentation
model (§18 of the TEL Specification), its semantic model after type
assignment against a schema (§20.2), and its BinTEL encoding (§7 of the
BinTEL Specification). The aim is to make each layer concrete in a way that
implementors can verify against their own tooling.

## 1. Schema

```tel
name greeting

document
  field text
    scalar string
  field bold optional
    flag
```

This schema declares a document with two members:

- `text`, a required scalar (validated by the built-in `string` validator —
  any value is accepted). Required is the default for every Field; `optional`
  loosens it.
- `bold`, an optional flag (explicit `optional`).

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
in `src/lib.rs`.)

## 6. Value hash (§3)

The SHA-256 digest of the 16-byte sequence above is the **value hash** of
this semantic model. Two implementations that produce different presentation
encodings of the same semantic content (for example, putting `bold` before
`text`, or using a literal atom for the value) MUST produce the same value
hash.

## 7. Complete BinTEL document (§6)

The complete byte stream is:

```
C0 D1                           # magic number
20 <signature bytes…>           # signature: length (varint) + bytes
02 00 0c …                      # document root (as above)
```

The signature for a no-layer schema is the schema's own 32-byte value hash
(see BinTEL §8). The pragma's schema identifier carries the same 32 bytes
encoded as 32 BASE-256 characters.

## See also

- [`contact-schema.tel`](contact-schema.tel) — a larger example showing
  `define`s, `Reference` types, and a `select` with all-`flag` variants.
- [`contact-document.tel`](contact-document.tel) — a document conforming
  to that schema, with hard-space multi-token values.
- [`tel-schema.bintel.hex`](tel-schema.bintel.hex) — the BinTEL document
  root encoding of `tel-schema.tel`, whose SHA-256 is normatively pinned
  in §20.5 of the TEL Specification.
