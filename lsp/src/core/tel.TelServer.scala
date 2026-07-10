package tel

import scala.collection.mutable as scm

import soundness.*

// The mode givens that the inherited `LspServer.main`/`serve` rely on. They are imported inside
// `exegesis.LspServer` for its own `main`, but overriding `main`/`serve` here means providing them
// in scope. `charEncoders.utf8Encoder` is used by the (overridden) stdio transport.
import backstops.stackTraceBackstop
import charEncoders.utf8Encoder
import errorDiagnostics.emptyDiagnostics
import executives.completions
import interpreters.posixInterpreter
import parsing.trackPositions
import probates.awaitProbate
import strategies.throwUnsafely
import threading.virtualThreading

// A Language Server for TEL documents, built on Exegesis. `LspServer` supplies the JSON-RPC dispatch
// and the stdio transport, so this object provides the handler hooks: it tracks open documents,
// publishes diagnostics (from Stratiform's TEL parser) when a document is opened or changed, and
// answers hover requests over the pragma line.
object TelServer extends LspServer():
  def name: Text = t"tel"
  override def version: Optional[Text] = t"0.1.0"

  def capabilities: Lsp.ServerCapabilities =
    Lsp.ServerCapabilities
      ( textDocumentSync        = Lsp.TextDocumentSyncKind.Full,
        hoverProvider           = true,
        documentSymbolProvider  = true,
        foldingRangeProvider    = true,
        selectionRangeProvider  = true,
        documentHighlightProvider = true,
        definitionProvider      = true,
        referencesProvider      = true )

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
  //
  // Parse the document with Stratiform's TEL parser and surface every error it reports as an LSP
  // diagnostic. Parsing runs under a `validate` accrual boundary so that recoverable defects (§19.5)
  // are collected rather than aborting on the first; `read[Tel]` yields the recovered document (which
  // we discard here — diagnostics only need the errors) while each `TelError` folds into `TelErrors`.

  // Accrual accumulator: each surfaced error with its focus. For schema validation, `Focus.position`
  // is filled by `Tel.Type.assign` (via `Focus.withPosition`) against the position-tracked document
  // (`import parsing.trackPositions`), so the diagnostic can point at the offending compound.
  private case class Accrued(items: List[(Optional[Tel.Focus], TelError)] = Nil)(using Diagnostics)
  extends Error(m"${items.length} TEL errors"):
    def add(focus: Optional[Tel.Focus], error: TelError): Accrued = Accrued(items :+ (focus, error))

  private def diagnose(text: Text): List[Lsp.Diagnostic] =
    val lines = text.s.linesIterator.toIndexedSeq
    val parse = parseErrors(text)

    // A schema document (declares `name` + `document`) is additionally validated against the built-in
    // tel-schema meta-schema, surfacing E2xx schema-validity errors. Documents that reference an
    // external schema can't be resolved (there is no schema resolver), so they get parse errors only.
    val schema =
      if parse.isEmpty && isSchemaDocument(lines)
      then safely(text.read[Tel]).lay(Nil)(schemaErrors)
      else Nil

    (parse ::: schema).map(diagnostic(_, lines))

  private def parseErrors(text: Text): List[(Optional[Tel.Focus], TelError)] =
    validate[Tel.Focus](Accrued()):
      case error: TelError => accrual.add(prior, error)
    . protect(text.read[Tel])
    . items

  // Validate a schema document two ways: (1) conformance to the built-in tel-schema meta-schema
  // (`assign`), catching malformed schema syntax; (2) constructing the `Tels` from it
  // (`Reconstructor.fromTel`), catching schema-validity errors (E2xx). Errors are de-duplicated by
  // code and message.
  private def schemaErrors(tel: Tel): List[(Optional[Tel.Focus], TelError)] =
    val conformance =
      validate[Tel.Focus](Accrued()):
        case error: TelError => accrual.add(prior, error)
      . protect(Tel.Type.assign(tel, Tels.Axiom.tels))
      . items

    val construction =
      validate[Tel.Focus](Accrued()):
        case error: TelError => accrual.add(prior, error)
      . protect(Tels.Reconstructor.fromTel(tel))
      . items

    (conformance ::: construction).distinctBy((_, error) => (error.reason.number, error.reason))

  // A schema document declares that it conforms to the tel-schema meta-schema in its pragma, e.g.
  // `tel 1.0 https://tel-lang.org/schema/tel-schema`. Only such documents are validated against the
  // built-in meta-schema (regular documents reference an external schema we can't resolve).
  private def isSchemaDocument(lines: IndexedSeq[String]): Boolean =
    val index = if lines.headOption.exists(_.startsWith("#!")) then 1 else 0
    lines.lift(index).exists(_.contains("tel-schema"))

  private def diagnostic(entry: (Optional[Tel.Focus], TelError), lines: IndexedSeq[String])
  :   Lsp.Diagnostic =
    val (focus, error) = entry
    Lsp.Diagnostic
      ( range    = errorRange(focus, error, lines),
        severity = Lsp.DiagnosticSeverity.Error,
        code     = t"E${error.reason.number}",
        source   = t"tel",
        message  = m"${error.reason}".text )

  // Prefer the error's own position (set for parse errors), else the focus position filled by
  // `assign` (schema/validation errors); both expose a 0-based `.span` that maps to an LSP range via
  // Exegesis's `Lsp.Range.from`. Fall back to the first line only when no span is available.
  private def errorRange(focus: Optional[Tel.Focus], error: TelError, lines: IndexedSeq[String])
  :   Lsp.Range =
    val position: Optional[TelError.Position] =
      if error.position.absent then focus.lay(Unset)(_.position) else error.position

    val fallback =
      val end = lines.headOption.fold(1)(_.length.max(1))
      Lsp.Range(Lsp.Position(0, 0), Lsp.Position(0, end))

    val range = Lsp.Range.from(position.lay(Span.empty)(_.span)).or(fallback)

    // Parse errors report a point (zero-width span); widen those to the end of the line so the
    // diagnostic is visible. Located errors with a length (e.g. a schema keyword) keep their span.
    if range.start == range.end then
      val end = lineLength(lines, range.start.line).max(range.start.character + 1)
      range.copy(end = Lsp.Position(range.start.line, end))
    else range

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

  // A well-formed pragma: `tel <major>.<minor>` optionally followed by a schema and/or sigil. The
  // pragma is the first line, unless an interpreter directive (`#!…` shebang) precedes it.
  private val pragmaPattern = "tel [0-9]+\\.[0-9]+( .*)?"

  private def pragmaIndex(lines: List[String]): Int =
    if lines.headOption.exists(_.startsWith("#!")) then 1 else 0

  override def hover(uri: Text, position: Lsp.Position): Optional[Lsp.Hover] =
    openDocument(uri).lay(Unset: Optional[Lsp.Hover]): document =>
      val (lines, tree) = structure(document.text)
      val pragma = pragmaIndex(lines.to(List))

      if position.line == pragma && lines.lift(pragma).exists(_.matches(pragmaPattern)) then
        Lsp.Hover(Lsp.MarkupContent(value = t"**TEL document** — pragma `${lines(pragma).tt}`"))
      else
        // Over a named type (definition or reference), show that definition and its members.
        wordAt(position, lines) match
          case Some((word, _, _)) => definitions(tree).get(word) match
            case Some(node) => Lsp.Hover(Lsp.MarkupContent(value = describe(node)))
            case None       => Unset
          case None => Unset

  private def describe(node: Node): Text =
    val members = node.children.map(_.keyword).distinct
    val head = t"**${node.keyword} ${node.atoms.headOption.getOrElse(t"")}**"
    if members.isEmpty then head else t"$head — ${members.join(t", ")}"

  // ── Structure (source scan) ───────────────────────────────────────────────────────────────────
  //
  // Stratiform's parse tree carries no source spans, so the position-based features (outline,
  // folding, selection ranges, highlights, go-to-definition) are derived from a lightweight
  // indentation scan of the source text here. Each non-blank, non-comment line is a compound —
  // `<indent><keyword> <atoms…>` — and nesting follows indentation. (Embedded source/literal-atom
  // payloads are not specially recognised, so an outline of a document that uses them may include
  // payload lines; ordinary documents and all schema documents are handled exactly.)

  private case class Node
      ( line:       Int,
        indent:     Int,
        keyword:    Text,
        keywordEnd: Int,
        detail:     Text,
        atoms:      List[Text],
        endLine:    Int,
        children:   List[Node] )

  private def lineLength(lines: IndexedSeq[String], line: Int): Int =
    lines.lift(line).fold(0)(_.length)

  private def leadingSpaces(line: String): Int =
    var i = 0
    while i < line.length && line.charAt(i) == ' ' do i += 1
    i

  private def documentSigil(lines: IndexedSeq[String]): Char =
    val index = if lines.headOption.exists(_.startsWith("#!")) then 1 else 0
    lines.lift(index) match
      case Some(line) =>
        var end = line.length
        while end > 0 && line.charAt(end - 1) == ' ' do end -= 1
        var start = end
        while start > 0 && line.charAt(start - 1) != ' ' do start -= 1
        val token = line.substring(start, end).nn
        if token.length == 1 && !Character.isLetterOrDigit(token.charAt(0)) then token.charAt(0)
        else '#'
      case None =>
        '#'

  // The full document as `lines` plus a nested tree of compound `Node`s.
  private def structure(text: Text): (IndexedSeq[String], List[Node]) =
    val lines = text.s.linesIterator.toIndexedSeq
    val sigil = documentSigil(lines)
    val pragmaIndex = if lines.headOption.exists(_.startsWith("#!")) then 1 else 0

    // A compound line, keeping its source line index, indentation, keyword and inline atoms.
    final case class Raw(index: Int, indent: Int, keyword: Text, keywordEnd: Int, detail: Text)

    val raws: List[Raw] = lines.zipWithIndex.toList.flatMap: (line, index) =>
      val indent = leadingSpaces(line)
      // Skip blank/whitespace-only lines, the pragma line, and comment/separator lines (leading sigil).
      if indent >= line.length || index == pragmaIndex || line.charAt(indent) == sigil then None
      else
        var end = indent
        while end < line.length && line.charAt(end) != ' ' do end += 1
        var detailStart = end
        while detailStart < line.length && line.charAt(detailStart) == ' ' do detailStart += 1
        Some(Raw(index, indent, line.substring(indent, end).nn.tt, end, line.substring(detailStart).nn.tt))

    def build(items: List[Raw]): List[Node] = items match
      case Nil => Nil
      case head :: tail =>
        val (descendants, rest) = tail.span(_.indent > head.indent)
        val endLine = descendants.lastOption.fold(head.index)(_.index)
        val atoms = head.detail.cut(t" ").filter(_ != t"")
        Node(head.index, head.indent, head.keyword, head.keywordEnd, head.detail, atoms, endLine,
            build(descendants))
        :: build(rest)

    (lines, build(raws))

  private def flatten(nodes: List[Node]): List[Node] =
    nodes.flatMap(node => node :: flatten(node.children))

  // ── Structure features (Phase 1) ──────────────────────────────────────────────────────────────

  override def documentSymbols(uri: Text): List[Lsp.DocumentSymbol] =
    openDocument(uri).let: document =>
      val (lines, tree) = structure(document.text)
      tree.map(symbol(_, lines))
    . or(Nil)

  private def symbol(node: Node, lines: IndexedSeq[String]): Lsp.DocumentSymbol =
    Lsp.DocumentSymbol
      ( name           = node.keyword,
        detail         = if node.detail.s.isEmpty then Unset else node.detail,
        kind           = Lsp.SymbolKind.Field,
        range          = Lsp.Range
                           ( Lsp.Position(node.line, node.indent),
                             Lsp.Position(node.endLine, lineLength(lines, node.endLine)) ),
        selectionRange = Lsp.Range
                           ( Lsp.Position(node.line, node.indent),
                             Lsp.Position(node.line, node.keywordEnd) ),
        children       = if node.children.isEmpty then Unset else node.children.map(symbol(_, lines)) )

  override def foldingRanges(uri: Text): List[Lsp.FoldingRange] =
    openDocument(uri).let: document =>
      def fold(node: Node): List[Lsp.FoldingRange] =
        val self =
          if node.endLine > node.line
          then List(Lsp.FoldingRange(startLine = node.line, endLine = node.endLine, kind = t"region"))
          else Nil

        self ::: node.children.flatMap(fold)

      structure(document.text)._2.flatMap(fold)
    . or(Nil)

  override def selectionRanges(uri: Text, positions: List[Lsp.Position]): List[Lsp.SelectionRange] =
    openDocument(uri).let: document =>
      val (lines, tree) = structure(document.text)
      positions.map(selectionRange(_, tree, lines))
    . or(Nil)

  private def selectionRange(position: Lsp.Position, tree: List[Node], lines: IndexedSeq[String])
  :   Lsp.SelectionRange =
    def path(nodes: List[Node]): List[Node] =
      nodes.find(n => position.line >= n.line && position.line <= n.endLine) match
        case Some(node) => node :: path(node.children)
        case None       => Nil

    val ranges = path(tree).map: node =>
      Lsp.Range(Lsp.Position(node.line, node.indent), Lsp.Position(node.endLine, lineLength(lines, node.endLine)))

    val nested = ranges.foldLeft(Unset: Optional[Lsp.SelectionRange]): (parent, range) =>
      Lsp.SelectionRange(range, parent)

    nested.lay(Lsp.SelectionRange(Lsp.Range(position, position)))(identity)

  override def documentHighlights(uri: Text, position: Lsp.Position): List[Lsp.DocumentHighlight] =
    openDocument(uri).let: document =>
      val nodes = flatten(structure(document.text)._2)
      nodes.find(_.line == position.line) match
        case Some(target) =>
          nodes.filter(_.keyword == target.keyword).map: node =>
            Lsp.DocumentHighlight
              ( Lsp.Range(Lsp.Position(node.line, node.indent), Lsp.Position(node.line, node.keywordEnd)),
                Lsp.DocumentHighlightKind.Text )
        case None =>
          Nil
    . or(Nil)

  // ── Navigation (Phase 3) ──────────────────────────────────────────────────────────────────────
  //
  // Type-name navigation for schema documents: a `record`/`scalar`/`select` compound *defines* a
  // named type; a `field`/`variant`/`select`/`record` compound *references* one by its inline atom.
  // These are resolved textually against the source scan, so they work for any document that carries
  // such definitions (i.e. schemas).

  private val definitionKeywords = Set(t"record", t"scalar", t"select")

  // The whitespace-delimited tokens of a line, each with its start/end column.
  private def tokens(line: String): List[(String, Int, Int)] =
    val out = scm.ListBuffer[(String, Int, Int)]()
    var i = 0
    while i < line.length do
      if line.charAt(i) == ' ' then i += 1
      else
        val start = i
        while i < line.length && line.charAt(i) != ' ' do i += 1
        out += ((line.substring(start, i).nn, start, i))

    out.to(List)

  // The token under a cursor position, with its column span.
  private def wordAt(position: Lsp.Position, lines: IndexedSeq[String]): Option[(Text, Int, Int)] =
    val found = lines.lift(position.line).flatMap: line =>
      tokens(line).find((_, start, end) => position.character >= start && position.character <= end)

    found.map((token, start, end) => (token.tt, start, end))

  // Named-type definitions in the document, keyed by name (the first atom of a `record`/`scalar`/
  // `select` compound).
  private def definitions(nodes: List[Node]): Map[Text, Node] =
    flatten(nodes).flatMap: node =>
      if definitionKeywords.contains(node.keyword) then node.atoms.headOption.map(_ -> node) else None
    . to(Map)

  // Every whitespace-delimited occurrence of `word` as a whole token, with line and column span.
  private def occurrences(word: Text, lines: IndexedSeq[String]): List[(Int, Int, Int)] =
    lines.zipWithIndex.to(List).flatMap: (line, index) =>
      tokens(line).collect { case (token, start, end) if token.tt == word => (index, start, end) }

  private def location(uri: Text, line: Int, start: Int, end: Int): Lsp.Location =
    Lsp.Location(uri, Lsp.Range(Lsp.Position(line, start), Lsp.Position(line, end)))

  override def definition(uri: Text, position: Lsp.Position): List[Lsp.Location] =
    openDocument(uri).let: document =>
      val (lines, tree) = structure(document.text)

      wordAt(position, lines) match
        case Some((word, _, _)) => definitions(tree).get(word) match
          case Some(node) =>
            // Jump to the definition's name token on its own line.
            tokens(lines(node.line)).drop(1).headOption match
              case Some((_, start, end)) => List(location(uri, node.line, start, end))
              case None                  => Nil
          case None => Nil
        case None => Nil
    . or(Nil)

  override def references(uri: Text, position: Lsp.Position, includeDeclaration: Boolean)
  :   List[Lsp.Location] =
    openDocument(uri).let: document =>
      val (lines, _) = structure(document.text)

      wordAt(position, lines) match
        case Some((word, _, _)) =>
          occurrences(word, lines).map((line, start, end) => location(uri, line, start, end))
        case None =>
          Nil
    . or(Nil)

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
