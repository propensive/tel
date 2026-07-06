# tel — the TEL command-line tool (and Language Server)

The `tel` executable, built in Scala with the Soundness ecosystem and packaged in the same style as
[Flame](https://github.com/propensive/flame) (Mill + an Ethereal self-fetching native launcher).

It is organised around subcommands:

- **`tel lsp`** — run a Language Server for [TEL](../readme.md) documents over stdio (what an editor
  launches).
- **`tel lsp --log`** — stream, live, the messages a running server receives (a debugging aid; see
  below). More subcommands can be added later.

The language server is a proof-of-concept, intentionally minimal:

- **Diagnostics** — published on open and on change. It checks that the document begins with a valid
  pragma (`tel <major>.<minor>` optionally followed by a schema and/or sigil) and warns when a line is
  indented with tabs.
- **Hover** — over the pragma line, shows a short blurb naming the document and its pragma.

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

## Next steps

The validation here is a lightweight Scala stand-in. The reference TEL parser is the Rust crate `tel`
(`tel::parse -> ParseResult { document, errors }`); the intended evolution is to expose those richer,
spec-accurate diagnostics to the LSP (e.g. via a small JSON-emitting `tel-check` CLI the server shells
out to), mapping each `TelError`'s source span to an LSP range.
