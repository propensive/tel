<p align="center"><img src="logo.svg" height="300"></p>

# TEL, the Typed Element Language

TEL is a tree-structured data format designed to be edited by humans, agents, and processors
alike. Structure is carried by indentation and a single sigil character (`#` by default);
everything else is content. The result is a notation that reads like the data it represents,
with no escaping rules to learn and no punctuation to balance.

```tel
tel 1.0

project alpha
  description    Demo of TEL features
  contributor    Alice
  contributor    Bob   # was Robert
```

## Features

- **Minimal markup.** Indentation and a configurable sigil are all that distinguish structure
  from content; everything else is data.
- **Hosts other languages without escaping.** A scalar value may be carried as an indented
  block (a *source atom*) or as a delimited payload (a *literal atom*) — JSON, XML, Markdown,
  shell scripts, and the like embed verbatim.
- **Schemas and types.** A schema names records, sums, scalars, and validators. Definition
  names are PascalCase (`Contact`, `PhoneNumber`); field and variant keywords are kebab-case.
  Documents are checked against their schema during parsing.
- **User-extensible validation.** A schema attaches named validators to scalars and structs;
  the parser calls back to the application to run them.
- **Layered schemas with safe evolution.** A base schema may be refined by ordered layers;
  every permitted layer operation produces a *subtype* of the base, so older readers can
  still consume newer documents.
- **Concise binary wire format.** Every TEL document has an unambiguous **BinTEL** encoding
  (typically ~2× smaller than the text) for hashing, transmission, or storage.
- **BASE-256 textual carrier.** When the wire format must travel in a text channel, BASE-256
  encodes one byte as one Unicode letter — half the length of hex, copy-paste-safe, no
  escaping required.
- **Faithful round-trips.** Programmatic edits preserve comments, blank lines, atom form,
  and tabulation alignment wherever they aren't directly changed.

## Quick tour

### Pragma

Every TEL document begins with a pragma identifying the version, optional schema, and
optional sigil:

```tel
tel 1.0
tel 1.0 https://example.org/schema/contact#sigḅHrïЖqẍḱăL
tel 1.0 contact %
```

The schema identifier is either a URL, a URL with a BASE-256 signature fragment, or a bare
signature. The sigil overrides the default `#`.

### Compounds and atoms

A non-blank line is a **compound**: a keyword followed by zero or more inline atoms, and
optionally child blocks at one greater indent.

```tel
contact alice
  email          alice@example.org
  phone   work   +44 20 7946 0958
  phone   home   +44 117 496 0123
```

### Hosting other languages

A *source atom* is an indented block whose payload is captured verbatim. A *literal atom*
uses an arbitrary delimiter line and preserves every byte of its payload — including
trailing spaces, leading whitespace, and the sigil character.

```tel
fixture sample-payload
  description
      A JSON document carried inside TEL,
      with no escaping or fence wrapping.
  json
      { "name": "Fido", "kind": "dog" }

  shell
        ---
        #!/usr/bin/env bash
        echo "Greetings from $(hostname)"
        ---
```

### Schemas

A schema is itself a TEL document describing the shape of conforming documents. Three kinds
of named Definition coexist in one namespace: `record` (a product type), `scalar` (a leaf
value with validators), and `select` (a sum type — a named alternation of variants). At a
member position, `field` declares a single-keyword slot and `select` references a named
sum. Cardinality defaults to "exactly one"; `optional` loosens to "zero or one",
`repeatable` loosens to "zero or more". Layers may *tighten* these defaults in later
versions but never loosen them.

```tel
tel 1.0

name contact

record PhoneNumber
  field country-code String
  field number String

select Status
  variant active Flag
  variant archived Flag

document
  field name String
  field email String optional
  field phone PhoneNumber optional repeatable
  select Status optional
```

A document under this schema:

```tel
tel 1.0 contact

name alice
email alice@example.org
phone
  country-code 44
  number       2079460958
active
```

### Validators

Each scalar may declare one or more named **validators** (applied in AND-conjunction). A
record or sum may carry its own validators for cross-field or cross-variant constraints.
Validator names live in a single shared namespace and are resolved at parse time by a
host-language callback. Four built-in validators are guaranteed by every conforming parser:
`identifier` (kebab-case), `type-name` (PascalCase), `sigil` (a single sigil character),
and `string` (unconstrained).

```tel
scalar Hostname
  validate non-empty
  validate dns-label

record Event
  field start-date String
  field end-date String
  validate start-precedes-end
```

## Binary form (BinTEL)

Every well-typed TEL document has a deterministic **BinTEL** encoding (see
[`spec/bintel.md`](spec/bintel.md)). BinTEL is type-tag-free — the schema supplies all
typing, so the byte stream encodes only keyword indices and scalar values. A BinTEL stream
begins with the four bytes `B2 C4 B5 BB`, which render as the Greek letters `βτελ` in
BASE-256 textual form.

The BLAKE3-256 hash of a BinTEL document root is the document's **value hash**: a stable,
schema-aware identifier suitable for content addressing. Composed schemas (base + layers)
are identified by a **palimpsest** of component hashes, encoded as a single BASE-256 token
on the pragma line.

## BASE-256

[`spec/base256.md`](spec/base256.md) describes a binary-to-text encoding that maps every
byte to one Unicode letter (or ASCII digit) drawn from a fixed 256-character alphabet. A
BASE-256-encoded string is one word under Unicode word-segmentation (double-click selects
the whole token), contains no whitespace or punctuation, and decodes losslessly via a
single modulo operation.

## Where to go next

- [`spec/tel.md`](spec/tel.md) — the full TEL specification (25 sections, formal type system,
  error taxonomy, machine operations, round-trip properties).
- [`spec/bintel.md`](spec/bintel.md) — the BinTEL wire format.
- [`spec/palimpsest.md`](spec/palimpsest.md) — the palimpsest construction used in composed
  schema signatures.
- [`spec/base256.md`](spec/base256.md) — the BASE-256 textual encoding.
- [`demo/`](demo/) — worked schemas and documents covering inline/source/literal
  atoms, layered schemas, struct validators, and the canonical `tel-schema` self-bootstrap.
