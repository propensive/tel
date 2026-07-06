package tel

import scala.collection.mutable as scm

import soundness.*

// The mode givens that the inherited `LspServer.main`/`serve` rely on. They are imported inside
// `exegesis.LspServer` for its own `main`, but overriding `main`/`serve` here means providing them
// in scope. `charEncoders.utf8Encoder` is used by the (overridden) stdio transport.
import backstops.stackTraceBackstop
import charEncoders.utf8Encoder
import executives.completions
import interpreters.posixInterpreter
import probates.awaitProbate
import strategies.throwUnsafely
import threading.virtualThreading

// A minimal Language Server for TEL documents, built on Exegesis. `LspServer` supplies the JSON-RPC
// dispatch and the stdio transport, so this object mainly provides the handler hooks: it tracks open
// documents, publishes diagnostics when a document is opened or changed, and answers hover requests
// over the pragma line.
//
// The validation here is a deliberately lightweight, self-contained proof-of-concept: it checks the
// pragma line and flags tab indentation. The reference TEL parser lives in the Rust crate `tel`
// (`tel::parse`); wiring the LSP to that parser's richer diagnostics is the natural next step.
object TelServer extends LspServer():
  def name: Text = t"tel"
  override def version: Optional[Text] = t"0.1.0"

  def capabilities: Lsp.ServerCapabilities =
    Lsp.ServerCapabilities(textDocumentSync = Lsp.TextDocumentSyncKind.Full, hoverProvider = true)

  // ── Open-document store ─────────────────────────────────────────────────────────────────────
  //
  // The currently-open documents, keyed by URI, maintained from the `didOpen`/`didChange`/`didClose`
  // notifications. (Exegesis also keeps its own store; this is the server's own, as a basis for
  // features that need the live document set.) Guarded by its own monitor because, as an Ethereal
  // daemon, one JVM may host several editor sessions.

  private val documents: scm.HashMap[Text, Lsp.TextDocumentItem] = scm.HashMap()

  private def openDocument(uri: Text): Optional[Lsp.TextDocumentItem] =
    documents.synchronized(documents.get(uri).getOrElse(Unset))

  // ── Diagnostics ─────────────────────────────────────────────────────────────────────────────

  // A well-formed pragma: `tel <major>.<minor>` optionally followed by a schema and/or sigil.
  private val pragmaPattern = "tel [0-9]+\\.[0-9]+( .*)?"

  // The pragma is the first line, unless an interpreter directive (`#!…` shebang) precedes it.
  private def pragmaIndex(lines: List[String]): Int =
    if lines.headOption.exists(_.startsWith("#!")) then 1 else 0

  private def diagnose(text: Text): List[Lsp.Diagnostic] =
    val lines = text.s.linesIterator.toList
    val pragma = pragmaIndex(lines)

    val pragmaDiagnostic: List[Lsp.Diagnostic] =
      lines.lift(pragma) match
        case Some(line) if line.matches(pragmaPattern) => Nil
        case other =>
          val width = other.fold(1)(_.length.max(1))
          List
            ( Lsp.Diagnostic
                ( range    = Lsp.Range(Lsp.Position(pragma, 0), Lsp.Position(pragma, width)),
                  severity = Lsp.DiagnosticSeverity.Error,
                  code     = t"TEL001",
                  source   = t"tel",
                  message  = t"Expected a TEL pragma here, e.g. `tel 1.0` (optionally followed by a "+
                             t"schema identifier and/or a sigil character)." ) )

    val tabDiagnostics: List[Lsp.Diagnostic] =
      lines.zipWithIndex.collect:
        case (line, index) if line.startsWith("\t") =>
          val leadingTabs = line.takeWhile(_ == '\t').length
          Lsp.Diagnostic
            ( range    = Lsp.Range(Lsp.Position(index, 0), Lsp.Position(index, leadingTabs)),
              severity = Lsp.DiagnosticSeverity.Warning,
              code     = t"TEL002",
              source   = t"tel",
              message  = t"TEL indentation must use spaces, not tabs." )

    pragmaDiagnostic ::: tabDiagnostics

  // ── Document lifecycle ──────────────────────────────────────────────────────────────────────

  override def onOpen(document: Lsp.TextDocumentItem)(using LspClient): Unit =
    documents.synchronized(documents(document.uri) = document)
    summon[LspClient].publishDiagnostics(document.uri, diagnose(document.text))

  override def onChange
      ( textDocument: Lsp.VersionedTextDocumentIdentifier,
        changes:      List[Lsp.TextDocumentContentChangeEvent] )
      (using LspClient)
  :   Unit =
    // Full sync (see `capabilities`): the last change carries the whole new document text.
    changes.lastOption.foreach: change =>
      val updated = documents.synchronized:
        val item = documents.get(textDocument.uri) match
          case Some(existing) => existing.copy(version = textDocument.version, text = change.text)
          case None => Lsp.TextDocumentItem(textDocument.uri, t"", textDocument.version, change.text)

        documents(textDocument.uri) = item
        item

      summon[LspClient].publishDiagnostics(textDocument.uri, diagnose(updated.text))

  override def onClose(document: Lsp.TextDocumentIdentifier): Unit =
    documents.synchronized(documents.remove(document.uri))

  override def hover(uri: Text, position: Lsp.Position): Optional[Lsp.Hover] =
    openDocument(uri) match
      case document: Lsp.TextDocumentItem =>
        val lines = document.text.s.linesIterator.toList
        val pragma = pragmaIndex(lines)

        lines.lift(pragma) match
          case Some(line) if position.line == pragma && line.matches(pragmaPattern) =>
            val text: Text = line.tt
            Lsp.Hover(Lsp.MarkupContent(value = t"**TEL document** — pragma `$text`"))
          case _ =>
            Unset

      case _ =>
        Unset

  // ── Live message log ────────────────────────────────────────────────────────────────────────
  //
  // The tool runs as an Ethereal daemon: one JVM hosts every `tel` invocation, and `TelServer` is a
  // singleton loaded once in it. So `logSubscribers` is shared daemon-wide, and a `tel lsp --log`
  // session can observe — live — the messages that the editor's `tel lsp` session sends and receives.
  // Each `--log` subscriber gets its own `Spool`; the serving loop broadcasts every message, tagged
  // `recv` (client → server) or `send` (server → client), to all subscribers.

  private val logSubscribers: scm.HashSet[Spool[Text]] = scm.HashSet()

  private def broadcastLog(marker: Text, message: Text): Unit =
    logSubscribers.synchronized(logSubscribers.foreach(_.put(t"$marker $message")))

  // A logging variant of `LspServer.serve`: identical to the inherited stdio transport, but each
  // message is broadcast to any `--log` subscribers — outgoing ones as they are written, incoming
  // ones before they are dispatched.
  override def serve()(using Stdio, Monitor, Probate): Unit =
    val dispatch: Json => Optional[Json] = LspServer.dispatcher(this)

    val writer: Task[Unit] = async:
      outgoing.iterator.each: json =>
        val body: Text = json.encode
        broadcastLog(t"send", body)
        val payload: Data = body.data
        summon[Stdio].write(t"Content-Length: ${payload.length}\r\n\r\n".data)
        summon[Stdio].write(payload)
        summon[Stdio].out.flush()

    summon[Stdio].in.stream[Data].iterator.frames[ContentLength].each: frame =>
      val message: Text = frame.utf8
      broadcastLog(t"recv", message)
      try dispatch(message.decode[Json]).let(put)
      catch case error: Exception => put(JsonRpc.error(-32603, t"Internal error").json)

    writer.cancel()

  // Streams messages to stdout until interrupted (Ctrl-C); used by `tel lsp --log`.
  private def streamLog()(using Stdio): Unit =
    val spool = Spool[Text]()
    logSubscribers.synchronized(logSubscribers.add(spool))
    try
      Out.println(t"Streaming messages sent/received by the tel language server. Press Ctrl-C to stop.")
      spool.stream.iterator.each: message =>
        Out.println(message)
    catch case _: InterruptedException => ()
    finally logSubscribers.synchronized(logSubscribers.remove(spool))

  // ── Subcommands ─────────────────────────────────────────────────────────────────────────────
  //
  // `tel lsp` runs the language server over stdio (what an editor launches); `tel lsp --log` streams
  // the messages a running server sends and receives. Further subcommands can be added as new `case`
  // branches. This overrides the inherited `LspServer.main`, which ran the server unconditionally.

  private val LspCommand = Subcommand("lsp", "run the TEL language server over stdio (for editors)")

  override def main(args: IArray[Text]): Unit = cli:
    arguments match
      case LspCommand() :: rest if rest.exists(argument => argument() == t"--log") =>
        execute:
          streamLog()
          Exit.Ok

      case LspCommand() :: _ =>
        execute:
          supervise(serve())
          Exit.Ok

      case _ =>
        execute:
          Out.println(t"Usage:")
          Out.println(t"  tel lsp          run the TEL language server over stdio (for editors)")
          Out.println(t"  tel lsp --log    stream the messages a running tel server sends/receives")
          Exit.Fail(1)
