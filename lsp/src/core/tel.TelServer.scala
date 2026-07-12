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
import systems.javaSystem
import interfaces.paths.pathOnLinux
import textMetrics.uniformMetric
import tableStyles.thinRoundedTableStyle
import columnAttenuation.ignoreAttenuation

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
        completionProvider      = Lsp.CompletionOptions(),
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

  // The schema-registry directory, resolved once at LSP start-up (where the invoker's `Environment`
  // is in scope) and read on demand during diagnostics.
  private var schemaDirectory: Optional[Path on Linux] = Unset

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

    // When the document parses, validate it against its schema and surface the schema/validation
    // errors. A schema *document* (its pragma names `tel-schema`) is checked against the built-in
    // meta-schema; any other pragma schema — a bare name or BASE-256 signature — is resolved against
    // the local registry populated by `tel schema add`.
    val schema =
      if parse.nonEmpty then Nil
      else safely(text.read[Tel]).lay(Nil): tel =>
        pragmaSchema(lines) match
          case identifier: Text if identifier.s.contains("tel-schema") => schemaErrors(tel)
          case identifier: Text => resolveSchema(identifier).lay(Nil)(assignErrors(tel, _))
          case _                => Nil

    (parse ::: schema).map(diagnostic(_, lines))

  private def parseErrors(text: Text): List[(Optional[Tel.Focus], TelError)] =
    validate[Tel.Focus](Accrued()):
      case error: TelError => accrual.add(prior, error)
    . protect(text.read[Tel])
    . items

  // Validate a document against a schema (`Tel.Type.assign`), accruing every violation with the focus
  // whose source position `assign` fills in.
  private def assignErrors(tel: Tel, schema: Tels): List[(Optional[Tel.Focus], TelError)] =
    validate[Tel.Focus](Accrued()):
      case error: TelError => accrual.add(prior, error)
    . protect(Tel.Type.assign(tel, schema))
    . items

  // Validate a schema document two ways: (1) conformance to the built-in tel-schema meta-schema
  // (`assign`), catching malformed schema syntax; (2) constructing the `Tels` from it
  // (`Reconstructor.fromTel`), catching schema-validity errors (E2xx). De-duplicated by code + reason.
  private def schemaErrors(tel: Tel): List[(Optional[Tel.Focus], TelError)] =
    val construction =
      validate[Tel.Focus](Accrued()):
        case error: TelError => accrual.add(prior, error)
      . protect(Tels.Reconstructor.fromTel(tel))
      . items

    (assignErrors(tel, Tels.Axiom.tels) ::: construction)
    . distinctBy((_, error) => (error.reason.number, error.reason))

  // The pragma's schema identifier: `tel <version> [schema] [sigil]` — the first token after the
  // version that is not a single symbolic sigil character.
  private def pragmaSchema(lines: IndexedSeq[String]): Optional[Text] =
    val index = if lines.headOption.exists(_.startsWith("#!")) then 1 else 0
    lines.lift(index) match
      case Some(line) =>
        tokens(line).map((token, _, _) => token).drop(2)
        . filterNot(token => token.length == 1 && !Character.isLetterOrDigit(token.charAt(0)))
        . headOption.map(_.tt).getOrElse(Unset)
      case None =>
        Unset

  // Resolve a pragma schema identifier against the registry (loaded once at start-up).
  private def resolveSchema(identifier: Text): Optional[Tels] =
    schemaDirectory match
      case directory: (Path on Linux) => SchemaCache.resolve(directory, identifier)
      case _                          => Unset

  // Resolve a pragma schema identifier to its cache file (for cross-file go-to-definition).
  private def resolveSchemaFile(identifier: Text): Optional[Path on Linux] =
    schemaDirectory match
      case directory: (Path on Linux) => SchemaCache.resolveFile(directory, identifier)
      case _                          => Unset

  // ── Schema-aware information (hover + completion) ──────────────────────────────────────────────
  //
  // When a document resolves to a registered schema, the compound keywords correspond to schema
  // members. These helpers navigate the schema alongside the document's compound tree to surface each
  // member's type, cardinality, default and description.

  // The chain of compound nodes whose block contains `line`, outermost first.
  private def nodeChain(nodes: List[Node], line: Int): List[Node] =
    nodes.find(node => line >= node.line && line <= node.endLine) match
      case Some(node) => node :: nodeChain(node.children, line)
      case None       => Nil

  // The struct a `Reference`/`Struct` type denotes (a record reference resolves to its struct).
  private def structOf(fieldType: Tels.Type, schema: Tels): Optional[Tels.Struct] =
    fieldType match
      case struct: Tels.Struct  => struct
      case Tels.Reference(name) => schema.records.find(_.name == name) match
        case Some(record) => Tels.Struct(record.members, record.validators)
        case None         => Unset
      case _ => Unset

  // The struct that a field's members live in, one keyword deeper.
  private def memberStruct(struct: Tels.Struct, keyword: Text, schema: Tels): Optional[Tels.Struct] =
    struct.members.to(List).collectFirst { case f: Tels.Field if f.keyword == keyword => f.fieldType }
    match
      case Some(fieldType) => structOf(fieldType, schema)
      case None            => Unset

  // The struct reached by descending `schema.document` along a keyword path.
  private def structAt(schema: Tels, path: List[Text]): Optional[Tels.Struct] =
    path.foldLeft(schema.document: Optional[Tels.Struct]): (current, keyword) =>
      current match
        case struct: Tels.Struct => memberStruct(struct, keyword, schema)
        case _                   => Unset

  private def typeLabel(fieldType: Tels.Type, schema: Tels): Text =
    fieldType match
      case Tels.Reference(name)    => name
      case Tels.Flag               => t"Flag"
      case Tels.Scalar(validators) => if validators.length == 0 then t"scalar" else validators.to(List).join(t"+")
      case Tels.Struct(_, _)       => t"record"

  private def cardinality(required: Tels.Polarity, repeatable: Tels.Polarity): Text =
    val flags =
      List
        ( Option.when(required == Tels.Polarity.Loose)(t"optional"),
          Option.when(repeatable == Tels.Polarity.Loose)(t"repeatable") )
      . flatten

    if flags.isEmpty then t"" else t" (${flags.join(t", ")})"

  private def fieldMarkdown(field: Tels.Field, schema: Tels): Text =
    val header =
      t"**${field.keyword}** — ${typeLabel(field.fieldType, schema)}${cardinality(field.required, field.repeatable)}"

    val default = field.default.let(value => t"\n\nDefault: `$value`").or(t"")
    val description = field.description.let(value => t"\n\n$value").or(t"")
    t"$header$default$description"

  private def variantMarkdown(variant: Tels.Variant, reference: Text, schema: Tels): Text =
    val header =
      t"**${variant.keyword}** — variant of `$reference` (${typeLabel(variant.variantType, schema)})"

    variant.description.let(value => t"$header\n\n$value").or(header)

  // Hover markup for the member `keyword` of `struct` (a `field`, or a `select` variant).
  private def fieldHover(struct: Tels.Struct, keyword: Text, schema: Tels): Optional[Lsp.Hover] =
    val markup = struct.members.to(List).flatMap:
      case field: Tels.Field if field.keyword == keyword =>
        List(fieldMarkdown(field, schema))
      case reference: Tels.SelectRef =>
        schema.selects.find(_.name == reference.reference).to(List)
        . flatMap(_.variants.to(List)).filter(_.keyword == keyword)
        . map(variantMarkdown(_, reference.reference, schema))
      case _ =>
        Nil

    markup.headOption match
      case Some(text) => Lsp.Hover(Lsp.MarkupContent(value = text))
      case None       => Unset

  // Hover over a compound keyword in a document validated against a resolved schema.
  private def schemaFieldHover(lines: IndexedSeq[String], tree: List[Node], position: Lsp.Position)
  :   Optional[Lsp.Hover] =
    pragmaSchema(lines) match
      case identifier: Text => resolveSchema(identifier) match
        case schema: Tels => nodeChain(tree, position.line).reverse match
          case node :: ancestors if node.line == position.line =>
            structAt(schema, ancestors.reverse.map(_.keyword)) match
              case struct: Tels.Struct => fieldHover(struct, node.keyword, schema)
              case _                   => Unset
          case _ => Unset
        case _ => Unset
      case _ => Unset

  private def keywordCompletions(struct: Tels.Struct, schema: Tels): List[Lsp.CompletionItem] =
    struct.members.to(List).flatMap:
      case field: Tels.Field =>
        List
          ( Lsp.CompletionItem
              ( label         = field.keyword,
                kind          = Lsp.CompletionItemKind.Field,
                detail        = typeLabel(field.fieldType, schema),
                documentation = field.description.let(text => Lsp.MarkupContent(value = text)) ) )
      case reference: Tels.SelectRef =>
        schema.selects.find(_.name == reference.reference).to(List).flatMap(_.variants.to(List)).map: variant =>
          Lsp.CompletionItem
            ( label         = variant.keyword,
              kind          = Lsp.CompletionItemKind.EnumMember,
              detail        = t"variant of ${reference.reference}",
              documentation = variant.description.let(text => Lsp.MarkupContent(value = text)) )
      case _ =>
        Nil

  // The predefined TEL type names, always available in a schema document.
  private val builtinTypeNames = List(t"String", t"Identifier", t"TypeName", t"Sigil", t"Flag")

  // The schema a document is checked against: the built-in meta-schema for a schema document,
  // otherwise the registered schema its pragma resolves to.
  private def documentSchema(lines: IndexedSeq[String]): Optional[Tels] =
    if isSchemaDocument(lines) then Tels.Axiom.tels
    else pragmaSchema(lines) match
      case identifier: Text => resolveSchema(identifier)
      case _                => Unset

  private def isSchemaDocument(lines: IndexedSeq[String]): Boolean =
    pragmaSchema(lines) match
      case identifier: Text => identifier.s.contains("tel-schema")
      case _                => false

  // The line's indentation, its keyword (the first token, if any), and how many whole atoms precede
  // the cursor: 0 = keyword position, 1 = first value/atom, 2 = second atom, and so on.
  private def completionContext(line: String, character: Int): (Int, Optional[Text], Int) =
    val indent = leadingSpaces(line)
    val lineTokens = tokens(line)
    val keyword = lineTokens.headOption.map((token, _, _) => token.tt).getOrElse(Unset)
    val atomsBefore = lineTokens.count((_, _, end) => character > end)
    (indent, keyword, atomsBefore)

  private def fieldType(struct: Tels.Struct, keyword: Text): Optional[Tels.Type] =
    struct.members.to(List).collectFirst { case f: Tels.Field if f.keyword == keyword => f.fieldType }
    . getOrElse(Unset)

  // Type-name completions for a schema document: the document's own definitions plus the built-ins.
  private def typeNameCompletions(tree: List[Node]): List[Lsp.CompletionItem] =
    (definitions(tree).keys.to(List) ::: builtinTypeNames).distinct.sorted.map: name =>
      Lsp.CompletionItem(label = name, kind = Lsp.CompletionItemKind.Class)

  // Value completions in a data document: a `select`-typed field's value is one of its variants.
  private def atomValueCompletions
      (lines: IndexedSeq[String], tree: List[Node], line: Int, indent: Int, keyword: Text)
  :   Lsp.CompletionList =
    documentSchema(lines) match
      case schema: Tels =>
        val parents = nodeChain(tree, line).filter(_.indent < indent)
        val struct = structAt(schema, parents.map(_.keyword)).or(schema.document)

        fieldType(struct, keyword) match
          case Tels.Reference(name) => schema.selects.find(_.name == name) match
            case Some(select) =>
              Lsp.CompletionList(items = select.variants.to(List).map: variant =>
                Lsp.CompletionItem
                  ( label         = variant.keyword,
                    kind          = Lsp.CompletionItemKind.EnumMember,
                    detail        = t"variant of $name",
                    documentation = variant.description.let(text => Lsp.MarkupContent(value = text)) ))
            case None => Lsp.CompletionList()
          case _ => Lsp.CompletionList()
      case _ => Lsp.CompletionList()

  override def complete(uri: Text, position: Lsp.Position): Lsp.CompletionList =
    openDocument(uri).lay(Lsp.CompletionList()): document =>
      val (lines, tree) = structure(document.text)
      val line = lines.lift(position.line).getOrElse("")
      val (indent, keyword, atomsBefore) = completionContext(line, position.character)

      (keyword, atomsBefore) match
        // Keyword position — the members valid for the enclosing struct (of the resolved schema, or
        // the meta-schema for a schema document).
        case (_, 0) => documentSchema(lines) match
          case schema: Tels =>
            val parents = nodeChain(tree, position.line).filter(_.indent < indent)
            val struct = structAt(schema, parents.map(_.keyword)).or(schema.document)
            Lsp.CompletionList(items = keywordCompletions(struct, schema))
          case _ => Lsp.CompletionList()

        // Type-name slot in a schema document — `field <name> <type>`, `variant <name> <type>`.
        case (kw: Text, 2) if isSchemaDocument(lines) && Set(t"field", t"variant").contains(kw) =>
          Lsp.CompletionList(items = typeNameCompletions(tree))

        // Value slot in a data document — a `select`-typed field completes to its variants.
        case (kw: Text, 1) =>
          atomValueCompletions(lines, tree, position.line, indent, kw)

        case _ =>
          Lsp.CompletionList()

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
            case None       => schemaFieldHover(lines, tree, position)
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

  // ── Cross-file navigation into the schema (link-to-definition) ─────────────────────────────────
  //
  // When a document resolves to a registered schema, go-to-definition on a compound keyword jumps
  // across into the schema *file* — at the `field`/`variant` that declares it — and go-to-definition
  // on the pragma jumps to the schema file's head. The schema file is the read-only registry copy, so
  // an editor that honours filesystem permissions presents it read-only. Resolution walks the schema
  // file's own source scan alongside the document's compound tree.

  private val memberKeywords = Set(t"field", t"variant")

  // Top-level named-type definitions of a schema document, keyed by name. Unlike `definitions`, this
  // considers only the top level, so a nested *reference* (e.g. `select Status` inside `document`)
  // never shadows the real, child-bearing definition.
  private def topLevelDefinitions(nodes: List[Node]): Map[Text, Node] =
    nodes.flatMap: node =>
      if definitionKeywords.contains(node.keyword) then node.atoms.headOption.map(_ -> node) else None
    . to(Map)

  private def fieldNode(nodes: List[Node], name: Text): Optional[Node] =
    nodes.find(node => node.keyword == t"field" && node.atoms.headOption.contains(name)).getOrElse(Unset)

  // Descend a schema document from its `document` block along the ancestor keywords of a compound
  // (each a field whose type names a record), yielding the child nodes of the enclosing struct.
  private def descend(schemaTree: List[Node], context: List[Node], ancestors: List[Text])
  :   Optional[List[Node]] =
    ancestors match
      case Nil => context
      case keyword :: rest => fieldNode(context, keyword) match
        case field: Node => field.atoms.lift(1) match
          case Some(typeName) => topLevelDefinitions(schemaTree).get(typeName) match
            case Some(definition) => descend(schemaTree, definition.children, rest)
            case None             => Unset
          case None => Unset
        case _ => Unset

  // The schema-document node that declares `keyword`: a `field`/`variant` in `context` named
  // `keyword`, or a `variant` of a `select` referenced from `context`.
  private def locateMember(schemaTree: List[Node], context: List[Node], keyword: Text): Optional[Node] =
    context.find(node => memberKeywords.contains(node.keyword) && node.atoms.headOption.contains(keyword))
    . orElse:
        context.filter(_.keyword == t"select").flatMap: reference =>
          reference.atoms.headOption.to(List)
          . flatMap(name => topLevelDefinitions(schemaTree).get(name).to(List))
          . flatMap(_.children)
          . filter(node => node.keyword == t"variant" && node.atoms.headOption.contains(keyword))
        . headOption
    . getOrElse(Unset)

  private def schemaDefinition(lines: IndexedSeq[String], tree: List[Node], position: Lsp.Position)
  :   List[Lsp.Location] =
    val pragmaLine = if lines.headOption.exists(_.startsWith("#!")) then 1 else 0
    pragmaSchema(lines) match
      case identifier: Text => resolveSchemaFile(identifier) match
        case file: (Path on Linux) => SchemaCache.readText(file).lay(Nil): text =>
          val (schemaLines, schemaTree) = structure(text)
          val uri = t"file://${file.encode}"

          if position.line == pragmaLine then List(location(uri, 0, 0, 0))
          else nodeChain(tree, position.line).reverse match
            case node :: ancestors if node.line == position.line =>
              val documentBlock = schemaTree.find(_.keyword == t"document").map(_.children).getOrElse(Nil)
              descend(schemaTree, documentBlock, ancestors.reverse.map(_.keyword)).lay(Nil): context =>
                locateMember(schemaTree, context, node.keyword).lay(Nil): target =>
                  tokens(schemaLines(target.line)).drop(1).headOption match
                    case Some((_, start, end)) => List(location(uri, target.line, start, end))
                    case None                  => List(location(uri, target.line, target.indent, target.keywordEnd))
            case _ => Nil
        case _ => Nil
      case _ => Nil

  override def definition(uri: Text, position: Lsp.Position): List[Lsp.Location] =
    openDocument(uri).let: document =>
      val (lines, tree) = structure(document.text)

      wordAt(position, lines) match
        // A local named-type reference (schema documents) jumps within the file.
        case Some((word, _, _)) => definitions(tree).get(word) match
          case Some(node) => tokens(lines(node.line)).drop(1).headOption match
            case Some((_, start, end)) => List(location(uri, node.line, start, end))
            case None                  => Nil
          // Otherwise, try to jump across into the registered schema file.
          case None => schemaDefinition(lines, tree, position)
        case None => schemaDefinition(lines, tree, position)
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

  private val LspCommand       = Subcommand("lsp", "run the TEL language server over stdio (for editors)")
  private val SchemaCommand    = Subcommand("schema", "manage the schema registry")
  private val AddCommand       = Subcommand("add", "add a schema file to the registry")
  private val ListCommand      = Subcommand("list", "list registered schemas")
  private val SignatureCommand = Subcommand("signature", "show a schema's palimpsest signature")

  override def main(args: IArray[Text]): Unit = cli:
    arguments match
      case LspCommand() :: rest if rest.exists(argument => argument() == t"--log") =>
        execute:
          streamLog()
          Exit.Ok

      case LspCommand() :: _ =>
        execute:
          schemaDirectory = safely(SchemaCache.directory)
          supervise(serve())
          Exit.Ok

      case SchemaCommand() :: ListCommand() :: _ =>
        execute(schemaList())

      case SchemaCommand() :: AddCommand() :: Argument(file) :: _ =>
        execute(schemaAdd(file))

      case SchemaCommand() :: SignatureCommand() :: Argument(name) :: layers =>
        execute(schemaSignature(name, layers.map(_())))

      case _ =>
        execute:
          Out.println(t"Usage:")
          Out.println(t"  tel lsp                              run the language server over stdio")
          Out.println(t"  tel lsp --log                        stream the server's message traffic")
          Out.println(t"  tel schema list                      list registered schemas")
          Out.println(t"  tel schema add <file>                add a schema to the registry")
          Out.println(t"  tel schema signature <name> [layer…] show a schema's palimpsest signature")
          Exit.Fail(1)

  private def schemaList()(using Stdio, Environment, System): Exit =
    try
      val entries = SchemaCache.entries(SchemaCache.directory)
      if entries.isEmpty then Out.println(t"No schemas registered. Add one with `tel schema add`.")
      else
        val table = Scaffold[SchemaCache.Entry]
          ( Column(t"Name")(_.name),
            Column(t"BASE-256 id")(_.id),
            Column(t"Layers")(_.layers) )

        Out.println(table.tabulate(entries).grid(120).render.join(t"\n"))
      Exit.Ok
    catch case error: Error =>
      Out.println(t"tel: could not list schemas: ${error.message.text}")
      Exit.Fail(1)

  private def schemaAdd(file: Text)(using Stdio, Environment, System): Exit =
    try
      val entry = SchemaCache.add(SchemaCache.directory, file.decode[Path on Linux])
      Out.println(t"Added schema `${entry.name}` (id ${entry.id}).")
      Exit.Ok
    catch case error: Error =>
      Out.println(t"tel: could not add schema: ${error.message.text}")
      Exit.Fail(1)

  private def schemaSignature(name: Text, layers: List[Text])(using Stdio, Environment, System): Exit =
    try
      SchemaCache.load(SchemaCache.directory, name) match
        case tel: Tel =>
          Out.println(SchemaCache.signature(tel, layers))
          Exit.Ok
        case _ =>
          Out.println(t"tel: no schema named `$name` in the registry")
          Exit.Fail(1)
    catch case error: Error =>
      Out.println(t"tel: could not compute signature: ${error.message.text}")
      Exit.Fail(1)
