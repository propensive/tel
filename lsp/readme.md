# tel — the TEL command-line tool (and Language Server)

The `tel` executable, built in Scala with the Soundness ecosystem and packaged in the same style as
[Flame](https://github.com/propensive/flame) (Mill + an Ethereal self-fetching native launcher).

> **Build note:** the LSP depends on the locally-published Soundness **0.64.0**, which is built with
> the propensive Scala fork (`3.9.0-RC4-p8`). The build (`build.mill`) downloads that fork release
> from [proscala](https://github.com/propensive/proscala) into a shared cache
> (`~/.cache/soundness/proscala/<tag>/lib`) — the same one the Soundness build uses — so no local
> compiler build is needed; set `$SOUNDNESS_SCALA_HOME` to a `make`-built `release` directory to
> override. `SOUNDNESS_RELEASE_VERSION=0.64.0 ./mill soundness.all.publishLocal --transitive true`
> in `~/work/soundness` produces the 0.64.0 artifacts (the version must be given explicitly; the
> git-describe fallback picks up a stray tag).

It is organised around subcommands:

- **`tel lsp`** — run a Language Server for [TEL](../readme.md) documents over stdio (what an editor
  launches).
- **`tel lsp --log`** — stream, live, the messages a running server sends/receives (a debugging aid).
- **`tel schema list`** — list registered schemas as a table (name, BASE-256 id, layers).
- **`tel schema add <file>`** — validate a schema against the tel-schema meta-schema and add it to the
  registry (an *absolute* path for now).
- **`tel schema signature <name> [layer…]`** — print the BASE-256 palimpsest for a schema composed
  with the named layers (in order; none = the base schema).

The schema **registry** lives at `$XDG_CACHE_HOME/tel/schemas` (`~/.cache/tel/schemas`), shared by the
CLI and the LSP: `tel schema add` populates it, and the LSP resolves a document's pragma schema against
it to validate ordinary documents (see below). The built-in **tel-schema** meta-schema is always
preloaded, so it appears in `list` (and is resolvable) even on a fresh cache.

Features so far:

- **Diagnostics** — published on open and on change (the editor's incremental edits are applied by
  `Lsp.listen`'s document store, so each change re-parses the spliced text). The document is parsed
  with [Stratiform](https://github.com/propensive/stratiform)'s TEL parser (`read[Tel]`) under an
  accrual boundary, and every `TelError` is reported with its spec E-code (e.g. `E104`, `E107`), its
  message, and a **source range**. Ranges come from Stratiform's position tracking (`import
  parsing.trackPositions`), and are *exact*: every located `TelError` carries a `Span` giving both
  the start and the extent of the offending text, so a diagnostic underlines the token itself — the
  bad pragma phrase, the trailing-space run, the misaligned indent, the offending compound's
  keyword — rather than a single character or a whole line. A parse error carries its own `span`; a
  schema/validation error's span is filled onto its `Tel.Focus` by `Tel.Type.assign` (via
  `Tel.supplementPositions`), and, failing that, the focus's keyword path is resolved against the
  position-tracked document with `tel.locate`. A *schema* document (one whose pragma names the `tel-schema` meta-schema) is
  additionally validated against the built-in meta-schema (`Tels.Axiom.tels`), surfacing
  malformed-schema errors such as `E306` (unrecognised keyword).
- **Outline / document symbols**, **folding ranges**, **selection ranges**, and **document
  highlights** — derived from an indentation scan of the source. (Stratiform positions are looked up
  by keyword *path*, which can't disambiguate same-keyword siblings, so the scan stays authoritative
  for ordered structure.)
- **Go-to-definition**, **find references**, and **hover** for named types — a `record`/`scalar`/
  `select` compound defines a type; a `field`/`variant` references one by its inline atom. Hover over
  the pragma still shows the document's version.
- **Cross-file link-to-definition into the schema** — from a document that resolves to a registered
  schema, go-to-definition on a compound keyword jumps *across* into the schema file, at the
  `field`/`variant` that declares it (descending through record references for nested compounds), and
  go-to-definition on the pragma opens the schema file at its head. The target is the registry's copy,
  which is stored **read-only**, so an editor that honours filesystem permissions presents it read-only.
- **Schema-aware hover and completion** — when a document resolves to a registered schema, the server
  navigates the schema alongside the document's compound tree (descending into `record` references and
  flattening `select` variants):
  - **hover** over a compound keyword shows the member's type, cardinality (`optional`/`repeatable`),
    default, and **description**;
  - **completion** is driven by the schema at the cursor's position:
    - at a **keyword** slot — the members valid for the enclosing struct (`field`s and flattened
      `select` variants), each with its type as the detail and its **description** as the documentation;
    - at a **value** slot of a `select`-typed field — that select's variant keywords;
    - and, because a schema document is itself checked against the built-in **meta-schema**, editing a
      schema completes meta-keywords (`record`, `field`, `validate`, …) at a keyword slot and the
      available **type names** (the document's own `record`/`scalar`/`select` definitions plus the
      built-ins `String`, `Identifier`, `TypeName`, `Sigil`, `Flag`) at a `field`/`variant` type slot.

The whole tool is a single object, `tel.TelServer`, in
[`src/core/tel.TelServer.scala`](src/core/tel.TelServer.scala). Its `main` dispatches on the
subcommand; `tel lsp` calls `exegesis.Lsp.listen`, which supplies the JSON-RPC dispatch, the
open-document store (applying the editor's incremental edits) and the stdio transport. Each feature
is a **registration** in that call — `opened`, `hover`, `complete()`, `definition`, … — and the
capabilities the server advertises are derived from exactly those registrations, so there is no
capabilities record to keep in step. Within a handler the current `document`, the `workspace`, the
`client` and the request's payload (`position`, `positions`, …) are ambient.

## Building and installing

Requires JDK 25 (Mill fetches it via `temurin:25`) on the path used by Mill.

```sh
make install    # builds the `tel` launcher and copies it to ~/.local/bin
which tel        # sanity check that it is on your PATH
tel lsp          # run the language server on stdio (Ctrl-C to stop)
```

Other targets: `make assembly` (just the JAR), `make run` (runs `tel lsp` via the launcher for a
manual JSON-RPC smoke test), `make dev` (watch-compile).

The tool runs as an [Ethereal](https://github.com/propensive/ethereal) resident daemon: the first
launch starts a background JVM and later launches reconnect to it, so editor restarts are fast. After
rebuilding `tel`, kill the stray daemon JVM (`pkill -f ethereal.name=tel`) or restart your editor so
the new binary takes effect.

## Watching the traffic: `tel lsp --log`

Because every `tel` invocation shares one daemon JVM, you can watch the messages a running server
receives from a second terminal:

```sh
tel lsp --log
```

Leave that running while your editor (or another `tel lsp`) drives the server; each JSON-RPC message
is printed as it arrives, one per line, tagged `recv` (client → server) or `send` (server → client):

```
recv {"jsonrpc":"2.0","id":1,"method":"initialize",...}
send {"jsonrpc":"2.0","result":{"capabilities":...},"id":1}
recv {"jsonrpc":"2.0","method":"textDocument/didOpen",...}
send {"jsonrpc":"2.0","method":"textDocument/publishDiagnostics",...}
```

Press Ctrl-C to stop. It attaches to whichever daemon is running (starting one if necessary), so the
order you launch the editor and the logger doesn't matter. Note that stdout of the serving process is
reserved for the LSP wire protocol, which is why the log is exposed through this separate observer
rather than printed by the server itself.

## Using it from Zed

See [`../zed`](../zed) for the companion Zed extension (which launches `tel lsp`) and step-by-step
testing instructions.

## Schema resolution (how the LSP validates ordinary documents)

The LSP resolves a document's pragma schema against the registry and validates the document with
`Tel.Type.assign`. Note that TEL's pragma grammar admits a schema identifier that is a **URL** or a
**bare BASE-256 signature** — not a kebab-case name (a hyphenated name is a parse error, `E122`). So a
document references a *registered* schema by its **signature** (from `tel schema signature`):

```tel
tel 1.0 ḡǼJûĿΫęôқδfΊzžμȑωûĺǑЬǨỵξϋ4SṽζẄǽOḁ
…
```

The LSP matches that signature against each cached schema's base or fully-composed signature. Schema
*documents* (pragma names `tel-schema`) are still validated against the built-in meta-schema.

## Known gaps / next steps

- **`tel schema add` needs an absolute path** (relative-path resolution wants the invoker's
  `WorkingDirectory` threaded through the daemon's CLI context — deferred).
- **Deeper schema validity** — `assign`/`fromTel` catch malformed schema *syntax* but not semantic
  E2xx (unresolved references, duplicate definitions, empty selects); surfacing those needs
  Stratiform's §20.1 schema-validity pass.
- **Resolution by name / URL** — only signature (and all-alphanumeric name) lookups work; mapping a
  URL or kebab-case name to a cached schema would need a stored URL↔schema/name index.
- **Formatting** (`tel.show`) and **rename**. Formatting is now within reach: Stratiform's §22.2
  machine-operation set is exposed through `open[Tel]`, including §22.3 canonical presentation.
- **Per-node structure ranges from positions** — outline/folding use a source scan because
  `tel.locate` resolves a keyword *path* (ambiguous for same-keyword siblings).
- **Scalar `encoding` is unsupported upstream.** `stratiform.Tels` has no `encoding` anywhere — not
  on `ScalarDefinition`, not in `Tels.Axiom`, not in the BinTEL codecs — although the language has
  it (tel.md §21.7, BinTEL B13–B15) and the in-repo meta-schema
  ([`tel.MetaSchema`](src/core/tel.MetaSchema.scala)) declares `field encoding Identifier optional`.
  So a schema document using `encoding` reports a spurious `E306`, and LSP-computed tel-schema
  signatures do not match the pinned normative vector.
