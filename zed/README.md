# TEL — a Zed extension

A [Zed](https://zed.dev) extension that adds support for [TEL](../readme.md) documents (`.tel`):

- **Syntax highlighting** via the existing [`tree-sitter-tel`](../ref/tree-sitter-tel) grammar.
- **Diagnostics and hover** via the [`tel`](../lsp) Language Server (built with Soundness/Exegesis).

The extension itself is tiny: [`src/lib.rs`](src/lib.rs) simply tells Zed to launch `tel lsp` (the
`tel` binary it finds on your `PATH`, with the `lsp` subcommand). Everything else is configuration:

- [`extension.toml`](extension.toml) — registers the `TEL` language, the `tel` language server, and the
  `tree-sitter-tel` grammar (fetched from `github.com/propensive/tel`, subdirectory `ref/tree-sitter-tel`).
- [`languages/tel/config.toml`](languages/tel/config.toml) — associates the `.tel` suffix with the language.
- [`languages/tel/highlights.scm`](languages/tel/highlights.scm), `injections.scm` — the highlight queries.

## Testing it

### 1. Prerequisites

- **Rust via rustup** (a Homebrew Rust will not work for Zed dev extensions). Zed compiles the
  extension to WebAssembly; if it reports a missing target, run:
  ```sh
  rustup target add wasm32-wasip2   # older Zed: wasm32-wasip1
  ```
- Make sure `~/.local/bin` is on the `PATH` that Zed inherits (Zed uses your login-shell environment).

### 2. Build and install the language server

```sh
cd ../lsp
make install    # builds the `tel` launcher and copies it to ~/.local/bin
which tel        # sanity check
```

### 3. Install this extension as a dev extension

1. In Zed, open the command palette (`Cmd-Shift-P`) and run **`zed: install dev extension`**
   (equivalently: Extensions page → *Install Dev Extension*).
2. Select this `zed/` directory. Zed compiles `src/lib.rs` to WASM and loads the extension.

### 4. Verify

Open a `.tel` file (e.g. [`../tel-schema.tel`](../tel-schema.tel) or anything in [`../demo`](../demo)):

- **Highlighting** — the pragma, atoms, comments etc. should be coloured.
- **Diagnostics** — change the pragma `tel 1.0` to `tel abc`; a red error (`TEL001`) should appear on that
  line and clear when you fix it. Indent a line with a tab to see a `TEL002` warning.
- **Hover** — hover over the pragma line; a small popup names the document and its pragma.

### Troubleshooting

- Command palette → search **"language server logs"** to watch the JSON-RPC traffic; `zed: open log`
  shows `Zed.log`. Launching `zed --foreground` from a terminal surfaces extension `stderr` and verbose logs.
- `tel` runs as an [Ethereal](https://github.com/propensive/ethereal) resident **daemon** — the first
  file you open spawns a background JVM; later opens reconnect (fast). After rebuilding `tel`, kill the
  stray daemon JVM (`pkill -f ethereal.name=tel`) or restart Zed so the new binary is picked up.
- If Zed can't find the server, confirm `~/.local/bin` is on its `PATH`, or hardcode the absolute path to
  `tel` in `language_server_command` (in `src/lib.rs`) as a fallback.
