# tel — the TEL command-line tool (and Language Server)

The `tel` executable, built in Scala with the Soundness ecosystem and packaged in the same style as
[Flame](https://github.com/propensive/flame) (Mill + an Ethereal self-fetching native launcher).

> **Build note:** Soundness now publishes only *bundles*, under the `dev.propensive` group, so this
> build depends on four of them — `soundness-base`, `soundness-data` (Stratiform), `soundness-cli`
> and `soundness-tool` (Exegesis) — rather than one artifact per library. The pinned version is
> **0.0.1-TEST**, a local build of Soundness's `stratiform/staged-codecs` branch (the one carrying
> TEL `key` fields, TELP paths and scalar codecs), resolved from `~/.ivy2/local`; override with
> `$SOUNDNESS_VERSION`. Reproduce it with
> `SOUNDNESS_RELEASE_VERSION=0.0.1-TEST ./mill 'soundness.{base,cli,data,tool}.publishLocal'`
> in that worktree (the version must be given explicitly; the git-describe fallback picks up a stray
> tag).
>
> Soundness is built with the propensive Scala fork, and its TASTy is only readable by that
> compiler, so `scalaVersion`/`scalaRelease` here must match the values the Soundness build used —
> currently the coordinate `3.9.0-RC5-p11` and the release tag `3.9.0-RC5-p13`. The build downloads
> that release from [proscala](https://github.com/propensive/proscala) into a shared cache
> (`~/.cache/soundness/proscala/<tag>/lib`) — the same one the Soundness build uses — so no local
> compiler build is needed; set `$SOUNDNESS_SCALA_HOME` to a `make`-built `release` directory to
> override.

It is organised around subcommands:

- **`tel lsp`** — run a Language Server for [TEL](../readme.md) documents over stdio (what an editor
  launches).
- **`tel lsp --log`** — stream, live, the messages a running server sends/receives (a debugging aid).
- **`tel schema list`** — list registered schemas as a table (name, BASE-256 id, layers).
- **`tel schema add <file>`** — validate a schema against the TELS meta-schema and add it to the
  registry (relative paths resolve against the invoking shell's directory).
- **`tel schema signature <name> [layer…]`** — print the BASE-256 palimpsest for a schema composed
  with the named layers (in order; none = the base schema).

The schema **registry** lives at `$XDG_CACHE_HOME/tel/schemas` (`~/.cache/tel/schemas`), shared by the
CLI and the LSP: `tel schema add` populates it, and the LSP resolves a document's pragma schema against
it to validate ordinary documents (see below). The built-in **TELS** meta-schema is always
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
  position-tracked document with `tel.locate`. A *schema* document (one whose pragma names the `tels` meta-schema) is
  additionally validated against the built-in meta-schema (`Tels.Axiom.tels`), surfacing
  malformed-schema errors such as `E306` (unrecognised keyword), plus a local `E210` check for
  duplicate (or built-in-colliding) definition names. Diagnostics are cleared when a document
  closes. A pragma naming an unregistered schema gets an `Information` diagnostic on the
  identifier (the document is valid, just unvalidated); the known-spurious `E306` on scalar
  `encoding` is downgraded to a `Warning` with an explanation.
- **Outline / document symbols**, **folding ranges**, **selection ranges**, and **document
  highlights** — derived from an indentation scan of the source. (Stratiform positions are looked up
  by keyword *path*, which can't disambiguate same-keyword siblings, so the scan stays authoritative
  for ordered structure.) The scan recognises source-atom and literal-atom payloads (§14/§15)
  lexically, so payload lines never appear as compounds, and a compound's fold covers its payload.
- **Go-to-definition**, **find references**, and **hover** for named types — a `record`/`scalar`/
  `select` compound defines a type; a `field`/`variant` references one by its inline atom. Hover over
  the pragma reports the schema-resolution status: the resolved schema's name, signature and
  registry path (or the meta-schema, or why resolution failed).
- **Cross-file link-to-definition into the schema** — from a document that resolves to a registered
  schema, go-to-definition on a compound keyword jumps *across* into the schema file, at the
  `field`/`variant` that declares it (descending through record references for nested compounds), and
  go-to-definition on the pragma opens the schema file at its head. The target is the registry's copy,
  which is stored **read-only**, so an editor that honours filesystem permissions presents it read-only.
- **Schema-aware hover and completion** — when a document resolves to a registered schema, the server
  navigates the schema alongside the document's compound tree (descending into `record` references and
  flattening `select` variants):
  - **hover** is column-accurate: over a compound *keyword* it shows the member's type, cardinality
    (`optional`/`repeatable`), default, and **description**; over a *value atom* it shows what the
    schema says about that slot — the expected scalar type and validators (with the field's
    default), the matched `select` variant (or the admissible variants), or a record's
    atom-assignable flag members; over a built-in validator name on a `validate` line, its §21.5
    blurb. Every hover carries the hovered token's range.
  - **completion** is driven by the schema at the cursor's position (a space after a keyword
    triggers it):
    - at a **keyword** slot — the members valid for the enclosing struct (`field`s and flattened
      `select` variants), each with its type as the detail and its **description** as the
      documentation (atom-taking fields insert a trailing space);
    - at a **value** slot — the inline atoms the member's type admits: a select reference's
      variants, or a record's atom-assignable members (flag fields and flag-typed variants);
    - at the **pragma's identifier slot** — the registered schemas, labelled by name, inserting the
      BASE-256 signature;
    - and, because a schema document is itself checked against the built-in **meta-schema**, editing a
      schema completes meta-keywords (`record`, `field`, `validate`, …) at a keyword slot, the
      available **type names** (the document's own `record`/`scalar`/`select` definitions plus the
      built-ins `String`, `Identifier`, `TypeName`, `Sigil`, `Flag`) at a `field`/`variant` type
      slot, the **flags** (`optional`, `required`, `repeatable`, `irrepeatable`, derived from the
      meta-schema) after a member declaration, and the four built-in **validator names** on a
      `validate` line.

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

The LSP matches that signature against each cached schema's base or fully-composed signature
(memoized per identifier, invalidated when the registry directory changes). Schema *documents*
(pragma names `tels`) are still validated against the built-in meta-schema. When the pragma
names a schema that matches nothing in the registry, the LSP says so: an `Information` diagnostic
(`schema-unresolved`) underlines the identifier, and hovering the pragma explains the resolution
status (resolved schema name, signature and registry path; meta-schema; or the failure). The
pragma's identifier slot also tab-completes to the registered schemas, inserting the signature.

## Testing

`mill tel.test.run` runs a Probably suite ([`tel.Tests`](src/test/tel.Tests.scala)) over the
server's pure handler functions — diagnostics, schema resolution, the structure scan (including
§14/§15 payload handling), hover and completion — against a throwaway schema registry. No JSON-RPC
transport is involved; for a live smoke test, drive `tel lsp` over stdio and watch with
`tel lsp --log`.

## Known gaps / next steps

- **Deeper schema validity** — `assign`/`fromTel` catch malformed schema *syntax*, and the LSP
  checks duplicate/built-in-colliding definition names itself (E210), but the remaining semantic
  E2xx (layer-merge errors, empty selects) need Stratiform's §20.1 schema-validity pass.
- **Resolution by name / URL** — only signature (and all-alphanumeric name) lookups work; mapping a
  URL or kebab-case name to a cached schema would need a stored URL↔schema/name index.
- **Formatting** (`tel.show`) and **rename**. Formatting is now within reach: Stratiform's §22.2
  machine-operation set is exposed through `open[Tel]`, including §22.3 canonical presentation.
- **Per-node structure ranges from positions** — outline/folding use a source scan because
  `tel.locate` resolves a keyword *path* (ambiguous for same-keyword siblings). The scan recognises
  source-atom/literal-atom payloads lexically, so payload lines are not mistaken for compounds.
- **Snippet completions** — Exegesis's `CompletionItem` has `insertText` but no `insertTextFormat`,
  so completions cannot yet carry `$1`-placeholder snippets.
- **TELP paths are not yet used.** Stratiform now implements the TELP path language
  ([`../spec/telp.md`](../spec/telp.md)) as `stratiform.Telp`, which addresses elements by keyword
  and by key value rather than positionally. Outline/folding still use the source scan (see above),
  and nothing yet exposes TELP to the editor — a `key`-aware go-to-definition or a "copy path here"
  command would be the natural first uses.
