# tel — the TEL command-line tool (and Language Server)

The `tel` executable, built in Scala with the Soundness ecosystem and packaged in the same style as
[Flame](https://github.com/propensive/flame) (Mill + an Ethereal self-fetching native launcher).

It is organised around subcommands:

- **`tel lsp`** — run a Language Server for [TEL](../readme.md) documents over stdio (what an editor
  launches).
- **`tel lsp --log`** — stream, live, the messages a running server receives (a debugging aid; see
  below). More subcommands can be added later.

Features so far:

- **Diagnostics** — published on open and on change. The document is parsed with
  [Stratiform](https://github.com/propensive/stratiform)'s TEL parser (`read[Tel]`) under an accrual
  boundary, and every `TelError` is reported with its spec E-code (e.g. `E104`, `E107`), its message,
  and a **source range**. Ranges come from Stratiform's position tracking (`import
  parsing.trackPositions`, Soundness 0.63.0): parse errors carry the parser's position, and
  schema/validation errors carry the position `Tel.Type.assign` fills into their `Tel.Focus` — so a
  *schema* error points at the offending compound (its keyword span) rather than the document root. A
  *schema* document (one whose pragma names the `tel-schema` meta-schema) is additionally validated
  against the built-in meta-schema (`Tels.Axiom.tels`), surfacing malformed-schema errors such as
  `E306` (unrecognised keyword).
- **Outline / document symbols**, **folding ranges**, **selection ranges**, and **document
  highlights** — derived from an indentation scan of the source. (Stratiform positions are looked up
  by keyword *path*, which can't disambiguate same-keyword siblings, so the scan stays authoritative
  for ordered structure.)
- **Go-to-definition**, **find references**, and **hover** for named types — a `record`/`scalar`/
  `select` compound defines a type; a `field`/`variant` references one by its inline atom. Hover over
  the pragma still shows the document's version.

The whole tool is a single object, `tel.TelServer`, in
[`src/core/tel.TelServer.scala`](src/core/tel.TelServer.scala): it extends `exegesis.LspServer` (which
supplies the JSON-RPC dispatch, the document store and the stdio transport) and overrides `main` to
dispatch on the subcommand.

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

## Known gaps / next steps

- **No schema resolver.** Documents reference schemas by URL, and Stratiform does not dereference
  them. Only *schema* documents (validated against the built-in meta-schema) get semantic validation;
  validating a regular document against its external schema needs a resolution mechanism (workspace
  registry or fetch) that is a deliberate design decision, not yet built.

Natural extensions:

- **Deeper schema validity** — `assign`/`fromTel` catch malformed schema *syntax* but not semantic
  E2xx (unresolved references, duplicate definitions, empty selects); surfacing those needs
  Stratiform's §20.1 schema-validity pass.
- **Formatting** wired to Stratiform's canonical printer (`tel.show`).
- **Completion** and **rename**.
- **External-schema resolution** so regular documents can be validated against their schema.
- **Per-node structure ranges from positions** — the outline/folding currently use a source scan
  because `tel.locate` resolves a keyword *path* (ambiguous for same-keyword siblings); an
  index/`Pointer`-based lookup would let structure features use real spans too.
