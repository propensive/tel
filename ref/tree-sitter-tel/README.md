# tree-sitter-tel

A [tree-sitter](https://tree-sitter.github.io/tree-sitter/) grammar for
**TEL** — the Typed Element Language defined in
[`spec/tel.md`](../../spec/tel.md).

The grammar parses TEL's presentation model:

- pragma (with version, schema id, optional sigil)
- shebang
- comments and remarks
- compounds with inline atoms, source atoms, and literal atoms
- child blocks via indentation
- tabulation lines and tabulated rows

It is **not** a schema-driven semantic parser. Schema resolution,
typing, BinTEL encoding, palimpsest verification, and the E2xx / E3xx
error families remain the concern of the reference parser at
[`ref/tel/`](../tel/).

## Layout

- `grammar.js` — top-level structural rules
- `src/scanner.c` — external scanner (indent / dedent / dynamic sigil /
  hard-space mode / source-atom and literal-atom capture / tabulation
  detection)
- `src/parser.c`, `src/grammar.json`, `src/node-types.json`,
  `src/tree_sitter/` — generated artefacts (committed so the crate
  builds without a tree-sitter CLI)
- `queries/highlights.scm` — syntax highlighting
- `queries/injections.scm` — payload injection hooks for embedded
  languages inside source/literal atoms
- `test/corpus/` — tree-sitter native tests
- `bindings/rust/` — Rust binding (re-exportable as `LANGUAGE`)

## Build

```
npm install
npx tree-sitter generate    # regenerates src/parser.c
npx tree-sitter test        # runs corpus tests
npx tree-sitter parse path/to/file.tel
```

For the Rust crate:

```
cargo build
cargo test
```

## Coverage

- All 10 `demo/*.tel` files parse without `ERROR` or `MISSING` nodes.
- All 118 positive test cases under `ref/tel/test/pos/` parse without
  `ERROR` or `MISSING` nodes.
- Most negative test cases under `ref/tel/test/neg/` still produce a
  structural tree (tree-sitter recovers locally); the scanner emits
  `_error_sentinel` only for runaway literal atoms.

## Soft and hard gaps

The grammar exposes the spec's hard- vs. soft-space distinction (§10.3)
as two atom node types and two gap node types:

- A 1-space gap between phrases produces `soft_atom` with a `soft_gap`
  child.
- A 2-or-more-space gap produces `hard_atom` with a `hard_gap` child,
  and switches the rest of the line into hard-space mode (so that
  subsequent soft spaces become part of the atom's content — e.g.
  `name  Alice Anderson` yields one `hard_atom` whose content is
  `Alice Anderson`).
- `remark` carries the same pair: it is `seq(choice(soft_gap, hard_gap),
  _remark_text)`, so the gap that introduces the remark is visible too.

The exact gap byte count is recoverable from the gap node's range.

## Known Limitations

- Tabulated rows are opaque tokens — column-aware sub-parsing is left to
  downstream tooling.
- The pragma sigil is captured as soon as the third pragma atom is a
  single-byte valid sigil character; a sigil overridden by the schema
  body (rather than the pragma) is not honoured.
- Parsing the demo `contact-document.tel` treats the leading comments
  before the pragma line as ordinary comments and the `tel 1.0 …` line
  as a compound with keyword `tel`. The strict spec (§8, E102) would
  flag the missing pragma; tree-sitter produces a usable tree either
  way.

## Spec reference

The authoritative grammar is [`spec/tel.md`](../../spec/tel.md). When
spec and grammar diverge, the spec wins.
