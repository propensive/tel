package tel

import scala.collection.mutable as scm

import soundness.*

// `soundness` re-exports Proscenium's collections, but an *exported* opaque type does not carry its
// companion's extensions into implicit scope — `.stdlib`, the `::` cons and the `.to(List)` factory
// would all be unavailable — so the collection types come straight from Proscenium, as the Soundness
// modules themselves do. A named import outranks the `soundness` wildcard.
import proscenium.{List, Nil, Chain, Map, Set, `::`}
import proscenium.compat.*

// `Accrued` appends to, and takes the length of, a `List` — both O(n) on a linked structure, and
// both gated behind this acknowledgement. The lists are the errors in one document, so linear cost
// is not a concern here.
import asymptotics.linearSizeComplexity
import backstops.stackTraceBackstop
import charEncoders.utf8Encoder
import errorDiagnostics.emptyDiagnostics
import executives.completions
import interpreters.posixInterpreter
import parsing.trackPositions
import probates.awaitProbate
import threading.virtualThreading
import systems.javaSystem
import interfaces.paths.pathOnLinux
// `Pathname` resolves a command-line path argument against the *invoking shell's* directory, not the
// daemon JVM's, so the working directory has to come from the `Cli` the Ethereal client supplies.
import workingDirectories.daemonClientWorkingDirectory
import textMetrics.uniformMetric
import tableStyles.thinRoundedTableStyle
import columnAttenuation.ignoreAttenuation

// A Language Server for TEL documents, built on Exegesis. `Lsp.listen` supplies the JSON-RPC
// dispatch, the open-document store (with incremental edits already applied) and the stdio
// transport, so this object provides the handler registrations: it publishes diagnostics from
// Stratiform's TEL parser when a document is opened or changed, and answers hover, completion,
// outline, folding, highlight and navigation requests.
//
// Every handler is registered inside the `Lsp.listen` block, and the current document, the
// workspace and the request's payload are ambient within it — so a handler reads `document.text`
// and `position` directly rather than resolving a URI against a store of its own.
object TelServer:
  // ── Diagnostics ─────────────────────────────────────────────────────────────────────────────
  //
  // Parse the document with Stratiform's TEL parser and surface every error it reports as an LSP
  // diagnostic. Parsing runs under a `validate` accrual boundary so that recoverable defects (§19.5)
  // are collected rather than aborting on the first: each `Tel.Error` folds into `Accrued`, while
  // `read[Tel]` yields the recovered document, which is kept both to locate the diagnostics and to
  // run the schema passes against.

  // Accrual accumulator: each surfaced error with its focus. For schema validation, `Focus.span` is
  // filled by `Tel.Type.assign` (via `Focus.withSpan`) against the position-tracked document
  // (`import parsing.trackPositions`), so the diagnostic can span the offending compound's keyword.
  private case class Accrued(items: List[(Optional[Tel.Focus], Tel.Error)] = Nil)(using Diagnostics)
  extends Error(m"${items.length} TEL errors"):
    def add(focus: Optional[Tel.Focus], error: Tel.Error): Accrued = Accrued(items :+ (focus, error))

  // The schema registry directory is resolved once, where the invoker's `Environment` is in scope,
  // and threaded to the handlers that need it: nothing capability-carrying or stateful lives in this
  // object, which the capture-checked API requires and the Ethereal daemon (one JVM, many editor
  // sessions) makes desirable anyway.
  private[tel] type Registry = Optional[Path on Linux]

  // ── Pragma and schema resolution ────────────────────────────────────────────────────────────
  //
  // The pragma line — `tel <major>.<minor> [<lira-ref>] [+<layer>…] [<signature>] [<sigil>]` (§8),
  // on line 0 or on line 1 after a `#!` interpreter directive — is parsed once into a `Pragma`,
  // shared by diagnostics, hover, completion and the structure scan. Phrases after the version
  // are classified by form, exactly as the specification's parser does: a `+`-prefixed phrase is
  // a layer selection, a phrase containing `/` is a LIRA schema reference, a well-formed BASE-256
  // string of signature length is a schema signature, and a final single sigil character is the
  // sigil.

  private[tel] case class PragmaId(name: Text, start: Int, end: Int)

  private[tel] case class Pragma
     ( line:       Int,
       wellFormed: Boolean,
       reference:  Optional[PragmaId],
       layers:     List[PragmaId],
       signature:  Optional[PragmaId],
       sigil:      Char ):

    // The phrase resolution keys on: the signature when present (it is authoritative, §8.1),
    // else the reference. Also the span diagnostics and hover anchor to.
    def identifier: Optional[PragmaId] = signature.or(reference)
    def layerNames: List[Text] = layers.map(_.name)

  // A well-formed pragma: `tel <major>.<minor>` optionally followed by schema phrases.
  private val pragmaPattern = "tel [0-9]+\\.[0-9]+( .*)?"

  // A well-formed BASE-256 schema signature phrase (§8.1): 33 characters for one component,
  // `37 + 2·(n − 2)` for `n ≥ 2`, every character a Unicode letter or ASCII digit.
  private def isSignatureToken(token: String): Boolean =
    val length = token.codePointCount(0, token.length)
    (length == 33 || (length >= 37 && (length - 37) % 2 == 0))
    && token.codePoints.nn.allMatch(Character.isLetterOrDigit(_))

  private[tel] def pragmaOf(lines: IndexedSeq[String]): Pragma =
    val index = if lines.headOption.exists(_.startsWith("#!")) then 1 else 0

    lines.lift(index) match
      case Some(line) =>
        val rest = tokens(line).stdlib.drop(2)

        var reference: Optional[PragmaId] = Unset
        var signature: Optional[PragmaId] = Unset
        var sigil: Optional[Char] = Unset
        val layers = scm.ListBuffer[PragmaId]()
        val count = rest.length

        rest.zipWithIndex.foreach: (entry, position) =>
          val (token, start, end) = entry

          if token.startsWith("+") && token.length > 1 then
            layers += PragmaId(token.substring(1).nn.tt, start, end)
          else if token.contains("/") && token.length > 1 then
            if reference.absent then reference = PragmaId(token.tt, start, end)
          else if isSignatureToken(token) then
            if signature.absent then signature = PragmaId(token.tt, start, end)
          else if token.length == 1 && !Character.isLetterOrDigit(token.charAt(0))
                  && token.charAt(0) != '+' && position == count - 1
          then sigil = token.charAt(0)

        Pragma
          ( index, line.matches(pragmaPattern), reference, layers.to(scala.List).to(List),
            signature, sigil.or('#') )

      case None =>
        Pragma(index, false, Unset, Nil, Unset, '#')

  // Does this pragma schema identifier name TELS, the schema-of-schemas (§20.5)? The meta-schema's
  // canonical coordinate is `specification.tel/tels`, pinned in the specification to `:1.0.0`;
  // Stratiform's `Reference.isTels` accepts the coordinate with or without its version pin.
  private[tel] def namesTels(identifier: Text): Boolean =
    Tel.Pragma.Reference.parse(identifier).lay(false)(_.isTels)

  // The outcome of resolving a document's pragma schema identification. `Unresolved` is
  // distinguished from `NoSchema` so that an identifier which matches nothing in the registry can
  // be reported to the user, rather than validation being skipped invisibly; `BadLayers` reports
  // a resolved schema whose `+layer` selection does not match its declared layers (§8.1).
  private[tel] enum Resolution:
    case NoSchema
    case Meta(meta: Tels, file: Optional[Path on Linux])
    case Resolved(entry: SchemaCache.Entry, file: Path on Linux, schema: Tels)
    case Unresolved(identifier: Text)
    case BadLayers(identifier: Text, detail: Text)

    // The schema to validate and navigate with, if resolution succeeded.
    def tels: Optional[Tels] = this match
      case Meta(meta, _)          => meta
      case Resolved(_, _, schema) => schema
      case _                      => Unset

    // The registry file backing the schema (for cross-file go-to-definition).
    def schemaFile: Optional[Path on Linux] = this match
      case Meta(_, file)        => file
      case Resolved(_, file, _) => file
      case _                    => Unset

  // The registry lookup name for a pragma schema identifier: a LIRA reference resolves by its
  // module-name tail (the local tel cache stores schemas as `<name>.tel`, and §2.7 of the design
  // binds the published name to the declared name), with any `:version`/`:tag` selector stripped;
  // a signature (or a bare name, for direct calls) is used verbatim.
  private def lookupName(identifier: Text): Text =
    if identifier.s.contains("/") then
      val coordinate = identifier.s.takeWhile(_ != ':')
      coordinate.substring(coordinate.lastIndexOf('/') + 1).nn.tt
    else identifier

  // Resolves pragma schema identifications against the registry, memoized per identification:
  // matching a BASE-256 signature re-reads and re-hashes every cached schema, which is too slow
  // to repeat on every keystroke. The memo is dropped whenever the registry directory's
  // modification time changes (i.e. `tel schema add` ran).
  private[tel] class PragmaResolver(registry: Registry):
    private val cache = scm.HashMap[Text, Resolution]()
    private var stamp: Long = 0L

    private def registryStamp: Long = registry match
      case directory: (Path on Linux) => java.io.File(directory.encode.s).lastModified
      case _                          => 0L

    def apply(pragma: Pragma): Resolution =
      pragma.identifier.lay(Resolution.NoSchema)(id => apply(id.name, pragma.layerNames))

    def apply(identifier: Text): Resolution = apply(identifier, Nil)

    def apply(identifier: Text, layers: List[Text]): Resolution = synchronized:
      val now = registryStamp
      if now != stamp then
        cache.clear()
        stamp = now

      val key = (identifier :: layers).join(t" +")
      cache.getOrElseUpdate(key, resolve(identifier, layers))

    def entries: List[SchemaCache.Entry] = registry match
      case directory: (Path on Linux) => SchemaCache.entries(directory)
      case _                          => Nil

    private def resolve(identifier: Text, layers: List[Text]): Resolution = registry match
      case directory: (Path on Linux) =>
        if namesTels(identifier)
        then Resolution.Meta(Tels.Axiom.tels, SchemaCache.resolveFile(directory, t"tels"))
        else
          val name = lookupName(identifier)
          SchemaCache.resolveFile(directory, name, layers) match
            case file: (Path on Linux) => SchemaCache.resolve(directory, name, layers) match
              case schema: Tels =>
                val entry =
                  SchemaCache.describe(file).or(SchemaCache.Entry(identifier, identifier, t""))
                Resolution.Resolved(entry, file, schema)

              case _ =>
                // The schema itself is present — only the layer selection can have failed. An
                // empty selection failing means the file no longer parses, which `describe`
                // above would also have caught; report the layers, the actionable case.
                if layers.isEmpty then Resolution.Unresolved(identifier)
                else Resolution.BadLayers
                  ( identifier,
                    t"the `+` layer selection does not match the schema's declared layers "
                    + t"(unknown name, or not in declaration order)" )
            case _ => Resolution.Unresolved(identifier)

      case _ =>
        if namesTels(identifier) then Resolution.Meta(Tels.Axiom.tels, Unset)
        else Resolution.NoSchema

  private[tel] def diagnose(text: Text, resolver: PragmaResolver): List[Lsp.Diagnostic] =
    val lines = text.s.linesIterator.toIndexedSeq
    val pragma = pragmaOf(lines)
    val resolution = resolver(pragma)

    // The parsed document, kept for the diagnostic ranges: when a focus arrives without a span of
    // its own, its keyword path is located against this position-tracked tree. It is assigned
    // inside the `guard` below, so it stays `Unset` exactly when the parse failed.
    var document: Optional[Tel] = Unset

    // The composed schema of a schema document, set only when reconstruction AND the §20.1
    // validity battery both succeed; the reference-coherence pass below runs against it.
    var composed: Optional[Tels] = Unset

    // Parsing and the schema passes that depend on it share ONE accrual boundary, so the document
    // is parsed once and the dependency between the two is carried by the `Venture` rather than by
    // re-testing whether the parse produced errors. Forcing a failed venture skips the enclosing
    // `guard`, which is what keeps the schema passes from running on a half-recovered tree and
    // reporting cascade errors.
    val accrued =
      validate[Tel.Focus](Accrued()):
        case error: Tel.Error => accrual.add(prior, error)
      . protect:
          val parsed = venture(text.read[Tel])

          // When the document parses, validate it against its schema. A schema *document* (its
          // pragma names TELS) is checked two ways: conformance to the built-in TELS meta-schema
          // (`assign`), catching malformed schema syntax, and construction of the `Tels` from it
          // (`Reconstructor.fromTel`), catching schema-validity errors (E2xx). Any other pragma
          // schema — a LIRA reference or BASE-256 signature — is resolved against the local registry
          // populated by `tel schema add`. The two meta-schema passes are ventured separately so
          // that a failure in the first still lets the second contribute its errors.
          guard:
            val tel = parsed()
            document = tel

            resolution match
              case Resolution.Meta(_, _) =>
                venture(Tel.Type.assign(tel, Tels.Axiom.tels))

                // Reconstruction alone checks only the document's shape; `Validation.validate`
                // composes the layers and runs the §20.1 schema-validity battery (E201-E221)
                // over the result. The composed schema feeds the reference-coherence pass.
                val validated = venture(Tels.Validation.validate(Tels.Reconstructor.fromTel(tel)))
                guard:
                  composed = validated()

              case Resolution.Resolved(_, _, schema) =>
                venture(Tel.Type.assign(tel, schema))

              case _ =>
                ()

    // The two meta-schema passes often report the same defect, so schema errors are collapsed by
    // reason. This is deliberately not applied when the parse failed: parse errors legitimately
    // repeat one reason at several positions, and the `guard` guarantees the two sets are never
    // both present.
    val collapsed =
      if document.absent then accrued.items
      else accrued.items.stdlib.distinctBy((_, error) => (error.reason.number, error.reason)).to(List)

    // Stratiform's validity battery now checks reference coherence itself (E209/E217), but it
    // aborts at the first defect without a source span. Those entries are dropped for a schema
    // document and re-located precisely by the incoherences pass below, against the composed
    // schema — recovered here, since the failed battery never assigned it.
    val coherenceReasons = scala.List(Tel.Error.Reason.UnresolvedReference,
        Tel.Error.Reason.ReferenceKindMismatch)

    val entries = resolution match
      case Resolution.Meta(_, _) if !document.absent =>
        if composed.absent then composed = document.let: tel =>
          safely(Tels.Layers.compose(Tels.Reconstructor.fromTel(tel)))

        collapsed.filter((_, error) => !coherenceReasons.contains(error.reason))

      case _ =>
        collapsed

    // Stratiform's `Reconstructor` leaves §20.1 schema-validity checking out of scope, so the one
    // cheap, high-value case — duplicate or built-in-colliding definition names (E210) — is
    // checked here against the source scan, on the offending name atom.
    val duplicates = resolution match
      case Resolution.Meta(_, _) if !document.absent => duplicateDefinitions(lines)
      case _                                         => Nil

    // Reference-coherence findings (E209/E217), located on the offending TypeName atom in the
    // source scan. Computed only when the whole validity battery passed, so they are never
    // cascade noise from a malformed schema.
    val incoherent = composed.lay(scala.List[Lsp.Diagnostic]()): schema =>
      SchemaCache.incoherences(schema).map: (name, reason) =>
        incoherenceDiagnostic(name, reason, lines)

    // An identifier that matches nothing in the registry gets a diagnostic of its own: the document
    // is valid TEL, but it is not being validated, which would otherwise be invisible to the user.
    // A bad `+` layer selection against a resolved schema is an error (§8.1: unknown layer names,
    // and selections out of declaration order — E124 — fail resolution).
    val unresolved = resolution match
      case Resolution.Unresolved(identifier) =>
        val range = pragma.identifier
          . lay(Lsp.Range(Lsp.Position(pragma.line, 0), Lsp.Position(pragma.line, 1))): id =>
              Lsp.Range(Lsp.Position(pragma.line, id.start), Lsp.Position(pragma.line, id.end))

        List(Lsp.Diagnostic
          ( range    = range,
            severity = Lsp.DiagnosticSeverity.Information,
            code     = t"schema-unresolved",
            source   = t"tel",
            message  = t"Schema `$identifier` is not registered, so this document is parsed but "
                       + t"not validated. Register the schema with `tel schema add <file>`." ))

      case Resolution.BadLayers(identifier, detail) =>
        val start = pragma.layers.headOption.map(_.start)
          . getOrElse(pragma.identifier.let(_.start).or(0))
        val end = pragma.layers.lastOption.map(_.end)
          . getOrElse(pragma.identifier.let(_.end).or(1))

        List(Lsp.Diagnostic
          ( range    = Lsp.Range(Lsp.Position(pragma.line, start), Lsp.Position(pragma.line, end)),
            severity = Lsp.DiagnosticSeverity.Error,
            code     = t"E124",
            source   = t"tel",
            message  = t"Schema `$identifier` resolves, but $detail." ))

      case _ => Nil

    (entries.map(diagnostic(_, lines, document)) ::: duplicates ::: incoherent.to(List)
     ::: unresolved).stdlib
    . distinctBy(entry => (entry.range, entry.code, entry.message))
    . to(List)

  // E210 (§20.1): two base Definitions sharing a name, or a Definition using a predefined built-in
  // name. Located on the second (or built-in-colliding) definition's name atom.
  private def duplicateDefinitions(lines: IndexedSeq[String]): List[Lsp.Diagnostic] =
    def diagnosticAt(line: Int, message: Text): Optional[Lsp.Diagnostic] =
      lines.lift(line).flatMap(candidate => tokens(candidate).stdlib.drop(1).headOption)
      . map: (_, start, end) =>
          Lsp.Diagnostic
            ( range    = Lsp.Range(Lsp.Position(line, start), Lsp.Position(line, end)),
              severity = Lsp.DiagnosticSeverity.Error,
              code     = t"E210",
              source   = t"tel",
              message  = message )
      . getOrElse(Unset)

    val builtins = builtinTypeNames.stdlib.to(scala.collection.immutable.Set)
    var seen = scala.collection.immutable.Set[Text]()
    val out = scm.ListBuffer[Lsp.Diagnostic]()

    lines.zipWithIndex.foreach: (line, index) =>
      val lineTokens = tokens(line).stdlib
      if leadingSpaces(line) == 0 && lineTokens.length >= 2
         && definitionKeywords.stdlib.contains(lineTokens(0)(0).tt)
      then
        val name = lineTokens(1)(0).tt
        if builtins.contains(name) then
          diagnosticAt(index, t"`$name` is a predefined built-in type name").let(out += _)
        else if seen.contains(name) then
          diagnosticAt(index, t"a definition named `$name` is already declared").let(out += _)
        else seen += name

    out.to(scala.List).to(List)

  // A reference-coherence finding, located on the first occurrence of the offending TypeName in a
  // type slot: the third token of a `field`/`variant` line, or the second token of an indented
  // `select` line (a `SelectRef`; the definition form is unindented). Falls back to the first
  // line when the scan cannot find it, which only a scan/parse disagreement would cause.
  private def incoherenceDiagnostic
     ( name: Text, reason: Tel.Error.Reason, lines: IndexedSeq[String] )
  :   Lsp.Diagnostic =

    val located = lines.zipWithIndex.collectFirst(scala.Function.unlift { (line, index) =>
      val lineTokens = tokens(line).stdlib

      val slot =
        if lineTokens.length >= 3 && (lineTokens(0)(0) == "field" || lineTokens(0)(0) == "variant")
           && lineTokens(2)(0) == name.s
        then Some(lineTokens(2))
        else if lineTokens.length >= 2 && lineTokens(0)(0) == "select" && leadingSpaces(line) > 0
                && lineTokens(1)(0) == name.s
        then Some(lineTokens(1))
        else None

      slot.map((_, start, end) => (index, start, end))
    })

    val (line, start, end) = located.getOrElse((0, 0, 1))

    Lsp.Diagnostic
      ( range    = Lsp.Range(Lsp.Position(line, start), Lsp.Position(line, end)),
        severity = Lsp.DiagnosticSeverity.Error,
        code     = t"E${reason.number}",
        source   = t"tel",
        message  = m"$reason".text )

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

  // The type that the member `keyword` of `struct` denotes: a field's declared type, or — because
  // a `select` reference is flattened so its variants appear as sibling keywords — the variant
  // type of a matching variant of a referenced select.
  private def memberType(struct: Tels.Struct, keyword: Text, schema: Tels): Optional[Tels.Type] =
    struct.members.readable.collectFirst { case f: Tels.Field if f.keyword == keyword => f.fieldType }
    . orElse:
        struct.members.readable.to(scala.List).collect { case ref: Tels.SelectRef => ref }
        . flatMap(ref => schema.selects.find(_.name == ref.reference).to(scala.List))
        . flatMap(_.variants.readable.to(scala.List))
        . collectFirst { case variant if variant.keyword == keyword => variant.variantType }
    . getOrElse(Unset)

  // The struct that a member's own members live in, one keyword deeper.
  private def memberStruct(struct: Tels.Struct, keyword: Text, schema: Tels): Optional[Tels.Struct] =
    memberType(struct, keyword, schema).let(structOf(_, schema))

  // The struct reached by descending `schema.document` along a keyword path.
  private def structAt(schema: Tels, path: List[Text]): Optional[Tels.Struct] =
    path.stdlib.foldLeft(schema.document: Optional[Tels.Struct]): (current, keyword) =>
      current match
        case struct: Tels.Struct => memberStruct(struct, keyword, schema)
        case _                   => Unset

  private def typeLabel(fieldType: Tels.Type, schema: Tels): Text =
    fieldType match
      case Tels.Reference(name) => name
      case Tels.Flag            => t"Flag"
      case Tels.Struct(_, _)    => t"record"

      // A declared `encoding` (§21.7) is part of what the scalar accepts — the codec's encoder is
      // one further validity constraint — so it belongs in the label beside the validators.
      case Tels.Scalar(validators, encoding) =>
        val base = if validators.length == 0 then t"scalar" else validators.readable.to(List).join(t"+")
        encoding.let(codec => t"$base [$codec]").or(base)

  // The declaration flags shown after a member's type in hover markup. `key` (§20) is not a
  // polarity — it marks the field as its record's identifier — but it reads naturally in the same
  // list, and it is the one flag a reader most needs to see.
  private def cardinality(required: Tels.Polarity, repeatable: Tels.Polarity, key: Boolean): Text =
    val flags: scala.List[Text] =
      (if required == Tels.Polarity.Loose then scala.List(t"optional") else scala.Nil)
      ::: (if repeatable == Tels.Polarity.Loose then scala.List(t"repeatable") else scala.Nil)
      ::: (if key then scala.List(t"key") else scala.Nil)

    if flags.isEmpty then t"" else t" (${flags.to(List).join(t", ")})"

  private def fieldMarkdown(field: Tels.Field, schema: Tels): Text =
    val header =
      t"**${field.keyword}** — ${typeLabel(field.fieldType, schema)}${cardinality(field.required, field.repeatable, field.key)}"

    val default = field.default.let(value => t"\n\nDefault: `$value`").or(t"")
    val description = field.description.let(value => t"\n\n$value").or(t"")
    t"$header$default$description"

  private def variantMarkdown(variant: Tels.Variant, reference: Text, schema: Tels): Text =
    val header =
      t"**${variant.keyword}** — variant of `$reference` (${typeLabel(variant.variantType, schema)})"

    variant.description.let(value => t"$header\n\n$value").or(header)

  // Hover markup for the member `keyword` of `struct` (a `field`, or a `select` variant).
  private def fieldMarkup(struct: Tels.Struct, keyword: Text, schema: Tels): Optional[Text] =
    val markup = struct.members.readable.to(List).flatMap:
      case field: Tels.Field if field.keyword == keyword =>
        List(fieldMarkdown(field, schema))

      case reference: Tels.SelectRef =>
        schema.selects.find(_.name == reference.reference).toList
        . flatMap(_.variants.readable.to(scala.List)).filter(_.keyword == keyword)
        . map(variantMarkdown(_, reference.reference, schema)).to(List)

      case _ =>
        Nil

    markup.headOption.getOrElse(Unset)

  // The built-in scalar type names (§20.5) with the §21.5 validator each one denotes.
  private val builtinScalars: List[(Text, Text)] =
    List
      ( t"String"     -> t"string",
        t"Identifier" -> t"identifier",
        t"TypeName"   -> t"type-name",
        t"Sigil"      -> t"sigil" )

  // The built-in validators of §21.5, with hover/completion blurbs.
  private val builtinValidators: List[(Text, Text)] =
    List
      ( t"string"     -> t"accepts any value (no constraint)",
        t"identifier" -> t"a kebab-case identifier (§20.7)",
        t"type-name"  -> t"a PascalCase type name (§20.7)",
        t"sigil"      -> t"a single sigil character" )

  private def scalarValueMarkup
     ( keyword:     Text,
       name:        Optional[Text],
       validators:  List[Text],
       default:     Optional[Text],
       description: Optional[Text],
       encoding:    Optional[Text] )
  :   Text =

    val checks =
      if validators.isEmpty then t""
      else t", validated as ${validators.map(validator => t"`$validator`").join(t", ")}"

    // A declared `encoding` (§21.7) is a further constraint on the value — the codec's encoder
    // rejects what it cannot represent (E312) — as well as its BinTEL representation.
    val codec = encoding.let(name => t", encoded as `$name`").or(t"")
    val header = t"**$keyword** value — `${name.or(t"scalar")}`$checks$codec"
    val fallback = default.let(value => t"\n\nDefault: `$value`").or(t"")
    val about = description.let(value => t"\n\n$value").or(t"")
    t"$header$fallback$about"

  // Hover markup for a value atom: what the schema says about the slot the atom fills — its
  // expected type and validators, the field's default, or the admissible select variants.
  private def valueMarkup
     ( keyword: Text, memberType: Tels.Type, default: Optional[Text], atom: Text, schema: Tels )
  :   Optional[Text] =

    memberType match
      case Tels.Reference(name) =>
        schema.selects.find(_.name == name) match
          case Some(select) =>
            select.variants.readable.to(scala.List).find(_.keyword == atom) match
              case Some(variant) => variantMarkdown(variant, name, schema)
              case None =>
                val names = select.variants.readable.to(List).map: (variant: Tels.Variant) =>
                  t"`${variant.keyword}`"

                t"**$keyword** value — one of ${names.join(t", ")}"

          case None => schema.scalars.find(_.name == name) match
            case Some(scalar) =>
              scalarValueMarkup
                ( keyword, name, scalar.validators.readable.to(List), default,
                  scalar.description, scalar.encoding )

            case None => builtinScalars.stdlib.find(_(0) == name) match
              case Some((_, validator)) =>
                scalarValueMarkup(keyword, name, List(validator), default, Unset, Unset)

              // A record-typed member: an inline atom fills one of the record's atom-assignable
              // members (§21) — a flag field or a flag-typed select variant — so describe that.
              case None => structOf(Tels.Reference(name), schema)
                . let(fieldMarkup(_, atom, schema))

      case Tels.Scalar(validators, encoding) =>
        scalarValueMarkup(keyword, Unset, validators.readable.to(List), default, Unset, encoding)

      case struct: Tels.Struct =>
        fieldMarkup(struct, atom, schema)

      case _ =>
        Unset

  // The innermost compound at `position` (if the cursor is on its own line) with the schema struct
  // its members belong to.
  private case class Slot(struct: Tels.Struct, node: Node)

  private def enclosing(tree: List[Node], position: Lsp.Position, schema: Tels): Optional[Slot] =
    nodeChain(tree, position.line).reverse match
      case node :: ancestors if node.line == position.line =>
        structAt(schema, ancestors.reverse.map(_.keyword)).let(Slot(_, node))

      case _ => Unset

  // A scalar- or reference-typed field takes an inline atom, so completing its keyword appends the
  // separating space; a flag or struct keyword stands alone.
  private def keywordInsertText(keyword: Text, fieldType: Tels.Type): Optional[Text] =
    fieldType match
      case Tels.Reference(_) | Tels.Scalar(_, _) => t"$keyword "
      case _                                     => Unset

  private def keywordCompletions(struct: Tels.Struct, schema: Tels): List[Lsp.CompletionItem] =
    struct.members.readable.to(List).flatMap:
      case field: Tels.Field =>
        List
          ( Lsp.CompletionItem
              ( label         = field.keyword,
                kind          = Lsp.CompletionItemKind.Field,
                detail        = typeLabel(field.fieldType, schema),
                documentation = field.description.let(text => Lsp.MarkupContent(value = text)),
                insertText    = keywordInsertText(field.keyword, field.fieldType) ) )

      case reference: Tels.SelectRef =>
        schema.selects.find(_.name == reference.reference).toList
        . flatMap(_.variants.readable.to(scala.List)).map: variant =>
            Lsp.CompletionItem
              ( label         = variant.keyword,
                kind          = Lsp.CompletionItemKind.EnumMember,
                detail        = t"variant of ${reference.reference}",
                documentation = variant.description.let(text => Lsp.MarkupContent(value = text)) )
        . to(List)

      case _ =>
        Nil

  // The predefined TEL type names, always available in a schema document.
  private val builtinTypeNames = List(t"String", t"Identifier", t"TypeName", t"Sigil", t"Flag")

  // The schema a document is checked against: the built-in meta-schema for a schema document,
  // otherwise the registered schema its pragma resolves to.
  private def documentSchema(lines: IndexedSeq[String], resolver: PragmaResolver): Optional[Tels] =
    resolver(pragmaOf(lines)).tels

  private def isSchemaDocument(lines: IndexedSeq[String]): Boolean =
    pragmaOf(lines).identifier.lay(false)(id => namesTels(id.name))

  // The line's indentation, its keyword (the first token, if any), and how many whole atoms precede
  // the cursor: 0 = keyword position, 1 = first value/atom, 2 = second atom, and so on.
  private def completionContext(line: String, character: Int): (Int, Optional[Text], Int) =
    val indent = leadingSpaces(line)
    val lineTokens = tokens(line)
    val keyword = lineTokens.headOption.map((token, _, _) => token.tt).getOrElse(Unset)
    val atomsBefore = lineTokens.count((_, _, end) => character > end)
    (indent, keyword, atomsBefore)

  // Type-name completions for a schema document: the document's own definitions plus the built-ins.
  private def typeNameCompletions(tree: List[Node]): List[Lsp.CompletionItem] =
    (definitions(tree).keys.to(scala.List) ::: builtinTypeNames.stdlib).distinct.sorted
    . map: name =>
        Lsp.CompletionItem(label = name, kind = Lsp.CompletionItemKind.Class)
    . to(List)

  // The schema struct enclosing `line`: the compound tree's ancestors above `indent`, descended
  // from the schema's document root.
  private def enclosingStruct(tree: List[Node], line: Int, indent: Int, schema: Tels): Tels.Struct =
    val parents = nodeChain(tree, line).filter(_.indent < indent)
    structAt(schema, parents.map(_.keyword)).or(schema.document)

  private def variantCompletion(variant: Tels.Variant, reference: Text): Lsp.CompletionItem =
    Lsp.CompletionItem
      ( label         = variant.keyword,
        kind          = Lsp.CompletionItemKind.EnumMember,
        detail        = t"variant of $reference",
        documentation = variant.description.let(text => Lsp.MarkupContent(value = text)) )

  // Completions for an inline atom of type `atomType`: a select reference completes to its
  // variants; a record completes to its atom-assignable members (§21) — flag fields and flag-typed
  // variants of its select references.
  private def atomCompletions(atomType: Tels.Type, schema: Tels): List[Lsp.CompletionItem] =
    atomType match
      case Tels.Reference(name) if schema.selects.exists(_.name == name) =>
        schema.selects.find(_.name == name).map(_.variants.readable.to(List)).getOrElse(Nil)
        . map(variantCompletion(_, name))

      case other => structOf(other, schema) match
        case struct: Tels.Struct =>
          struct.members.readable.to(List).flatMap:
            case field: Tels.Field => field.fieldType match
              case Tels.Flag =>
                List(Lsp.CompletionItem
                  ( label         = field.keyword,
                    kind          = Lsp.CompletionItemKind.Field,
                    detail        = t"Flag",
                    documentation =
                      field.description.let(text => Lsp.MarkupContent(value = text)) ))

              case _ => Nil

            case reference: Tels.SelectRef =>
              schema.selects.find(_.name == reference.reference).toList
              . flatMap(_.variants.readable.to(scala.List))
              . filter: variant =>
                  variant.variantType match
                    case Tels.Flag => true
                    case _         => false
              . map(variantCompletion(_, reference.reference)).to(List)

            case _ => Nil

        case _ => Nil

  // Value completions: what may fill the inline-atom slot after `keyword`.
  private def atomValueCompletions
     ( lines:    IndexedSeq[String],
       tree:     List[Node],
       line:     Int,
       indent:   Int,
       keyword:  Text,
       resolver: PragmaResolver )
  :   Lsp.CompletionList =

    documentSchema(lines, resolver) match
      case schema: Tels =>
        memberType(enclosingStruct(tree, line, indent, schema), keyword, schema) match
          case memberType: Tels.Type =>
            Lsp.CompletionList(items = atomCompletions(memberType, schema))

          case _ => Lsp.CompletionList()
      case _ => Lsp.CompletionList()

  // Flag completions after a member declaration in a schema document: the Flag-typed members of
  // the meta-schema record that describes the declaration (`optional`, `required`, `repeatable`,
  // `irrepeatable`), derived from the meta-schema rather than hardcoded, minus any already given.
  private def flagCompletions
     ( lines:    IndexedSeq[String],
       tree:     List[Node],
       line:     Int,
       indent:   Int,
       keyword:  Text,
       content:  String,
       resolver: PragmaResolver )
  :   List[Lsp.CompletionItem] =

    documentSchema(lines, resolver) match
      case schema: Tels =>
        val struct = enclosingStruct(tree, line, indent, schema)
        val present = tokens(content).stdlib.map((token, _, _) => token.tt)

        memberStruct(struct, keyword, schema).lay(Nil): member =>
          member.members.readable.to(List).flatMap:
            case field: Tels.Field if !present.contains(field.keyword) => field.fieldType match
              case Tels.Flag =>
                List(Lsp.CompletionItem
                  ( label         = field.keyword,
                    kind          = Lsp.CompletionItemKind.Keyword,
                    detail        = t"Flag",
                    documentation = field.description.let(text => Lsp.MarkupContent(value = text)) ))

              case _ => Nil
            case _ => Nil

      case _ => Nil

  // Pragma schema-identifier completions: each registered schema, inserted as its BASE-256
  // signature — the only bare identifier form the pragma grammar admits (§8) that the registry
  // can resolve.
  private def pragmaCompletions(resolver: PragmaResolver): List[Lsp.CompletionItem] =
    resolver.entries.map: (entry: SchemaCache.Entry) =>
      Lsp.CompletionItem
        ( label      = entry.name,
          kind       = Lsp.CompletionItemKind.Module,
          detail     = if entry.layers.s.isEmpty then t"BASE-256 signature"
                       else t"BASE-256 signature; layers: ${entry.layers}",
          insertText = entry.id )

  // Validator-name completions on a `validate` line: the four built-in validators of §21.5 (the
  // only ones the validator registry is guaranteed to know).
  private def validatorCompletions: List[Lsp.CompletionItem] =
    builtinValidators.map: (name, blurb) =>
      Lsp.CompletionItem
        ( label         = name,
          kind          = Lsp.CompletionItemKind.Function,
          documentation = Lsp.MarkupContent(value = blurb) )

  private[tel] def completions(text: Text, position: Lsp.Position, resolver: PragmaResolver)
  :   Lsp.CompletionList =

    val (lines, tree) = structure(text)
    val pragma = pragmaOf(lines)
    val line = lines.lift(position.line).getOrElse("")
    val (indent, keyword, atomsBefore) = completionContext(line, position.character)

    // The pragma line's schema-identifier slot completes to the registered schemas.
    if position.line == pragma.line then
      if atomsBefore == 2 then Lsp.CompletionList(items = pragmaCompletions(resolver))
      else Lsp.CompletionList()
    else (keyword, atomsBefore) match
      // Keyword position — the members valid for the enclosing struct (of the resolved schema, or
      // the meta-schema for a schema document).
      case (_, 0) => documentSchema(lines, resolver) match
        case schema: Tels =>
          val struct = enclosingStruct(tree, position.line, indent, schema)
          Lsp.CompletionList(items = keywordCompletions(struct, schema))

        case _ => Lsp.CompletionList()

      // Type-name slot in a schema document — `field <name> <type>`, `variant <name> <type>`.
      case (kw: Text, 2) if isSchemaDocument(lines) && memberKeywords.stdlib.contains(kw) =>
        Lsp.CompletionList(items = typeNameCompletions(tree))

      // Validator slot in a schema document — `validate <name>`.
      case (kw: Text, 1) if isSchemaDocument(lines) && kw == t"validate" =>
        Lsp.CompletionList(items = validatorCompletions)

      // Flag slot in a schema document — after `field <name> <type>` or `select <TypeName>`.
      case (kw: Text, n) if isSchemaDocument(lines)
                            && ((kw == t"field" && n >= 3) || (kw == t"select" && n >= 2)) =>
        Lsp.CompletionList
          ( items = flagCompletions(lines, tree, position.line, indent, kw, line, resolver) )

      // Value slot — a `select`-typed field completes to its variants.
      case (kw: Text, 1) =>
        atomValueCompletions(lines, tree, position.line, indent, kw, resolver)

      case _ =>
        Lsp.CompletionList()

  // ── Code actions (refactoring) ─────────────────────────────────────────────
  //
  // Refactorings between the inline-atom and compound-child presentations of a member (§20.2).
  // Every action is offered if and only if it is verified meaning-preserving: the candidate text is
  // generated, then both the original and the candidate are parsed and type-assigned against the
  // resolved schema, and the action is offered only when both assign without error AND yield the
  // same §18.2 semantic model. Validity is therefore decided by the same machinery as the
  // diagnostics — the positional walk below chooses the member keyword, but never the verdict.

  private[tel] def codeActionsAt(uri: Text, text: Text, range: Lsp.Range, resolver: PragmaResolver)
  :   List[Lsp.CodeAction] =

    val (lines, tree) = structure(text)

    documentSchema(lines, resolver) match
      case schema: Tels =>
        expandAtomAction(uri, text, lines, tree, range.start.line, schema).lay(Nil)(List(_))
        ::: inlineChildAction(uri, text, lines, tree, range.start.line, schema).lay(Nil)(List(_))

      case _ => Nil

  // The inline-atom spans of a compound line after `keywordEnd`, per §10.3 phrase separation:
  // initially a single space terminates a phrase, but from the start of the first hard-space run
  // (two or more spaces) onward only hard runs terminate phrases, and single spaces become phrase
  // content. Returns (text, start, end) column spans. Remarks are NOT recognised here: the code
  // actions decline any line whose space-split tokens include a bare sigil before tokenising —
  // conservative, since a sigil can legitimately sit inside a hard-mode phrase, but never wrong.
  // If this tokenisation ever disagrees with Stratiform's, the generated candidate fails semantic
  // verification and the action is declined, so correctness does not rest on it.
  private def inlineAtomSpans(line: String, keywordEnd: Int): scala.List[(String, Int, Int)] =
    val out = scm.ListBuffer[(String, Int, Int)]()
    var i = keywordEnd
    var hard = false

    while i < line.length do
      val gapStart = i
      while i < line.length && line.charAt(i) == ' ' do i += 1
      if i - gapStart >= 2 then hard = true

      if i < line.length then
        val start = i
        var end = i

        if !hard then
          while i < line.length && line.charAt(i) != ' ' do i += 1
          end = i
        else
          // `end` tracks the last non-space seen, so a trailing single space is left out of the
          // phrase (it would be E108 anyway, and verification catches the invalid document).
          var scanning = true
          while scanning && i < line.length do
            if line.charAt(i) == ' ' then
              var j = i
              while j < line.length && line.charAt(j) == ' ' do j += 1
              if j - i >= 2 || j >= line.length then scanning = false else i = j
            else
              i += 1
              end = i

        out += ((line.substring(start, end).nn, start, end))

    out.to(scala.List)

  // The separator to place before `atom` when writing it after `keyword` (or appending it to a
  // line): a hard gap whenever the atom contains a space — so §10.3 keeps it one phrase — or when
  // the line is already in hard mode, where a single space would merge the atom into the phrase
  // before it.
  private def gapFor(atom: Text, hardMode: Boolean): Text =
    if hardMode || atom.s.contains(" ") then t"  " else t" "

  // Move the LAST inline atom of the compound at `line` onto a child compound line. Restricted to
  // the last atom so that no other atom's position — and hence its §20.2 positional binding —
  // shifts; the moved atom re-binds to the same member by keyword, and the new child is inserted
  // as the compound's first child, so the semantic model's element order (atoms, then children,
  // each in source order) is also preserved. Anything more exotic — a remark on the line, a hard
  // gap (which locks the rest of the line into one atom, §10.3), or a source/literal-atom payload
  // (which must immediately follow its compound, §14/§15) — is declined rather than handled.
  private def expandAtomAction
     ( uri:    Text,
       text:   Text,
       lines:  IndexedSeq[String],
       tree:   List[Node],
       line:   Int,
       schema: Tels )
  :   Optional[Lsp.CodeAction] =

    flatten(tree).stdlib.find(_.line == line).map: node =>
      val lineText = lines.lift(line).getOrElse("")
      val payload = lines.lift(line + 1).exists: next =>
        leadingSpaces(next) < next.length && leadingSpaces(next) >= node.indent + 4

      val sigil = pragmaOf(lines).sigil.toString
      val remark = tokens(lineText).stdlib.exists(_(0) == sigil)
      val spans = inlineAtomSpans(lineText, node.keywordEnd)

      // The last phrase must run to the end of the line: with remarks declined, anything after it
      // would be trailing space (E108), and verification would reject the document anyway.
      if spans.isEmpty || payload || remark || spans.last(2) != lineText.length then Unset
      else
        val struct = enclosingStruct(tree, node.line, node.indent, schema)

        memberStruct(struct, node.keyword, schema).let: owner =>
          val atoms = spans.map((atom, _, _) => atom.tt).to(List)

          expansionOf(owner, atoms, schema).let: childText =>
            val atomText = spans.last(0).tt
            val previousEnd =
              if spans.length >= 2 then spans(spans.length - 2)(2) else node.keywordEnd
            val indent = lineText.substring(0, node.indent).nn + "  "

            val patched =
              (lines.take(line)
               :+ lineText.substring(0, previousEnd).nn
               :+ s"$indent$childText")
              ++ lines.drop(line + 1)

            verifiedAction(text, patched.mkString("\n").tt, schema):
              Lsp.CodeAction
                ( title = t"Move `$atomText` to a child compound line",
                  kind  = t"refactor.rewrite",
                  edit  = Lsp.WorkspaceEdit(changes = Map.of(scala.Predef.Map(uri -> List(
                    Lsp.TextEdit
                      ( Lsp.Range
                          ( Lsp.Position(line, previousEnd),
                            Lsp.Position(line, lineText.length) ),
                        t"\n$indent$childText" ))))) )
    . getOrElse(Unset)

  // The inverse of `expandAtomAction`: fold the child compound at `line` back onto its parent's
  // line as an inline atom. The child must be the parent's FIRST child and on the line immediately
  // below it — appending its atom at the end of the parent's line then preserves the semantic
  // model's element order exactly as the expansion did, and adjacency means no comment or blank
  // line is orphaned by deleting the child's line. A Scalar-typed child of one atom inlines as its
  // value; a Flag-typed field or select variant inlines as its keyword. Everything else — children
  // of its own, a payload, a remark or hard gap on either line, an empty Scalar (its empty-string
  // value has no inline spelling, §18.3) — is declined.
  private def inlineChildAction
     ( uri:    Text,
       text:   Text,
       lines:  IndexedSeq[String],
       tree:   List[Node],
       line:   Int,
       schema: Tels )
  :   Optional[Lsp.CodeAction] =

    val sigil = pragmaOf(lines).sigil.toString

    def declined(node: Node): Boolean =
      val nodeLine = lines.lift(node.line).getOrElse("")
      val payload = lines.lift(node.line + 1).exists: next =>
        leadingSpaces(next) < next.length && leadingSpaces(next) >= node.indent + 4

      payload || tokens(nodeLine).stdlib.exists(_(0) == sigil)

    nodeChain(tree, line).reverse match
      case child :: parent :: _
          if child.line == line && child.line == parent.line + 1 && child.children.isEmpty
             && parent.children.stdlib.headOption.exists(_.line == line)
             && !declined(child) && !declined(parent) =>

        val owner = enclosingStruct(tree, parent.line, parent.indent, schema)

        memberStruct(owner, parent.keyword, schema).let: parentStruct =>
          val childLine = lines.lift(child.line).getOrElse("")
          val childSpans = inlineAtomSpans(childLine, child.keywordEnd)

          val atom: Optional[Text] =
            memberType(parentStruct, child.keyword, schema).let(resolvedType(_, schema)).let:
              case _: Tels.Scalar =>
                if childSpans.length == 1 then childSpans.head(0).tt else Unset

              case Tels.Flag => if childSpans.isEmpty then child.keyword else Unset
              case _         => Unset

          atom.let: atom =>
            val parentLine = lines.lift(parent.line).getOrElse("")

            // A hard gap is needed when the parent line is already in hard mode — a single space
            // would merge the atom into the phrase before it — or when the atom itself contains a
            // space and must stay one phrase.
            val parentHard = parentLine.substring(parent.keywordEnd).nn.contains("  ")
            val gap = gapFor(atom, parentHard)

            val patched =
              (lines.take(parent.line) :+ s"$parentLine$gap$atom") ++ lines.drop(child.line + 1)

            verifiedAction(text, patched.mkString("\n").tt, schema):
              Lsp.CodeAction
                ( title = t"Inline `$atom` onto the `${parent.keyword}` line",
                  kind  = t"refactor.inline",
                  edit  = Lsp.WorkspaceEdit(changes = Map.of(scala.Predef.Map(uri -> List
                    ( Lsp.TextEdit
                        ( Lsp.Range
                            ( Lsp.Position(parent.line, parentLine.length),
                              Lsp.Position(parent.line, parentLine.length) ),
                          t"$gap$atom" ),
                      Lsp.TextEdit
                        ( Lsp.Range
                            ( Lsp.Position(child.line, 0),
                              Lsp.Position(child.line + 1, 0) ),
                          t"" ))))) )

      case _ => Unset

  // The action, if and only if `candidate` is verified equivalent to `text`: both must parse and
  // type-assign against `schema` without a single accrued error, and produce identical semantic
  // models.
  private def verifiedAction(text: Text, candidate: Text, schema: Tels)(action: => Lsp.CodeAction)
  :   Optional[Lsp.CodeAction] =

    assignedModel(text, schema).let: before =>
      assignedModel(candidate, schema).let: after =>
        if sameModel(before, after) then action else Unset

  // The §18.2 semantic model of `text` under `schema`, or `Unset` if parsing or type assignment
  // reports any error at all. The same accrual boundary as `diagnose`, so "no errors" here means
  // exactly "no diagnostics there".
  private def assignedModel(text: Text, schema: Tels): Optional[Tel.Element] =
    var model: Optional[Tel.Element] = Unset

    val accrued =
      validate[Tel.Focus](Accrued()):
        case error: Tel.Error => accrual.add(prior, error)
      . protect:
          val parsed = venture(text.read[Tel])

          guard:
            model = Tel.Type.assign(parsed(), schema)

    if accrued.items.isEmpty then model else Unset

  // Structural equality of two semantic models produced against the SAME schema: the flat keyword
  // index fully identifies the member (and, through the schema, its type), so index, tree shape and
  // scalar text suffice. Element order is compared too — order is part of the model — which makes
  // the check conservative: a refactoring that merely reorders equivalent elements is declined.
  private def sameModel(a: Tel.Element, b: Tel.Element): Boolean = (a, b) match
    case (Tel.Element.Value(i, _, s), Tel.Element.Value(j, _, t)) => i == j && s == t

    case (Tel.Element.Node(i, _, as), Tel.Element.Node(j, _, bs)) =>
      i == j && as.length == bs.length
      && as.readable.to(scala.List).zip(bs.readable.to(scala.List)).forall(sameModel(_, _))

    case _ => false

  // The child-compound line that the LAST atom of `atoms` expands to — `keyword value` for a
  // Scalar-typed field, the bare atom text for a Flag field or select variant — or `Unset` when
  // any atom fails to assign. This mirrors the §20.2 atom phase (the skip rule of step 3a, and
  // step 3d's rule that a repeatable member's position is not advanced), but only to CHOOSE the
  // keyword: a wrong choice yields a candidate that fails semantic verification, so correctness
  // never depends on this walk.
  // A member type with any `Reference` resolved through the schema's records and scalars (which
  // include the built-ins, prepended at reconstruction). A reference to a select — or to nothing —
  // resolves to `Unset`: the callers here treat both as "not this shape", and the genuine error is
  // the validator's to report.
  private def resolvedType(fieldType: Tels.Type, schema: Tels): Optional[Tels.Type] =
    fieldType match
      case Tels.Reference(name) =>
        schema.records.find(_.name == name)
        . map(record => Tels.Struct(record.members, record.validators): Tels.Type)
        . orElse:
            schema.scalars.find(_.name == name)
            . map(definition => Tels.Scalar(definition.validators, definition.encoding): Tels.Type)
        . getOrElse(Unset)

      case other => other

  private def expansionOf(struct: Tels.Struct, atoms: List[Text], schema: Tels): Optional[Text] =
    val members = struct.members.readable.to(scala.List).toVector

    def resolved(fieldType: Tels.Type): Optional[Tels.Type] = resolvedType(fieldType, schema)

    def required(member: Tels.Member): Boolean = member match
      case field: Tels.Field    => field.required != Tels.Polarity.Loose
      case select: Tels.SelectRef => select.required != Tels.Polarity.Loose
      case _: Tels.Exclude      => false

    def repeatable(member: Tels.Member): Boolean = member match
      case field: Tels.Field    => field.repeatable == Tels.Polarity.Loose
      case select: Tels.SelectRef => select.repeatable == Tels.Polarity.Loose
      case _: Tels.Exclude      => false

    def variantsOf(select: Tels.SelectRef): scala.List[Tels.Variant] =
      schema.selects.find(_.name == select.reference)
      . map(_.variants.readable.to(scala.List)).getOrElse(scala.Nil)

    def atomAssignable(member: Tels.Member): Boolean = member match
      case field: Tels.Field => resolved(field.fieldType) match
        case _: Tels.Scalar => true
        case Tels.Flag      => true
        case _              => false

      case select: Tels.SelectRef =>
        val variants = variantsOf(select)
        variants.nonEmpty && variants.forall: variant =>
          resolved(variant.variantType) match
            case Tels.Flag => true
            case _         => false

      case _: Tels.Exclude => false

    def flagShaped(member: Tels.Member): Boolean = member match
      case field: Tels.Field => resolved(field.fieldType) match
        case Tels.Flag => true
        case _         => false

      case select: Tels.SelectRef => atomAssignable(select)
      case _: Tels.Exclude        => false

    def matches(member: Tels.Member, atom: Text): Boolean = member match
      case field: Tels.Field      => field.keyword == atom
      case select: Tels.SelectRef => variantsOf(select).exists(_.keyword == atom)
      case _: Tels.Exclude        => false

    def skippable(member: Tels.Member, atom: Text): Boolean = member match
      case _: Tels.Exclude => true
      case _ =>
        !required(member)
        && (!atomAssignable(member) || (flagShaped(member) && !matches(member, atom)))

    var pos = 0
    var ok = true
    var result: Optional[Text] = Unset

    atoms.stdlib.foreach: atom =>
      if ok then
        while pos < members.length && skippable(members(pos), atom) do pos += 1

        if pos >= members.length then ok = false
        else members(pos) match
          case field: Tels.Field => resolved(field.fieldType) match
            case _: Tels.Scalar =>
              result = t"${field.keyword}${gapFor(atom, false)}$atom"
              if !repeatable(field) then pos += 1

            case Tels.Flag =>
              if atom == field.keyword then
                result = atom
                if !repeatable(field) then pos += 1
              else ok = false

            case _ => ok = false

          case select: Tels.SelectRef =>
            if matches(select, atom) then
              result = atom
              if !repeatable(select) then pos += 1
            else ok = false

          case _: Tels.Exclude => ok = false

    if ok then result else Unset

  private def diagnostic
     ( entry:    (Optional[Tel.Focus], Tel.Error),
       lines:    IndexedSeq[String],
       document: Optional[Tel] )
  :   Lsp.Diagnostic =

    val (focus, error) = entry
    val range = errorRange(focus, error, lines, document)

    // Every TEL E-code is a hard error. (Scalar `encoding` used to be special-cased here as a
    // warning, because Stratiform's schema reconstruction did not yet understand it and raised a
    // spurious E306; it now does, so the exception is gone.)
    Lsp.Diagnostic
      ( range    = range,
        severity = Lsp.DiagnosticSeverity.Error,
        code     = t"E${error.reason.number}",
        source   = t"tel",
        message  = m"${error.reason}".text )

  // Every located TEL error carries a `Span` — 0-based, `Line`-mode, and carrying the *extent* of
  // the offending text, not merely its first character — which is exactly the shape of an LSP
  // range, so Exegesis's `Lsp.Range.from` converts it directly. Three sources, in order of
  // precision:
  //
  //   1. `error.span`, which a parse error carries: the parser spans the token it rejected (a
  //      pragma phrase, a trailing-space run, a literal's opening delimiter, an indent run…), so
  //      the diagnostic underlines that token and nothing else.
  //   2. `focus.span`, which `Tel.Type.assign` fills in (via `Tel.supplementPositions`) for every
  //      accrued schema-validation focus, from the document's `PositionIndex` — hence the
  //      load-bearing `import parsing.trackPositions`. It spans the offending compound's keyword.
  //   3. Failing both — an error accrued outside `assign`, e.g. from `Tels.Reconstructor.fromTel` —
  //      the focus's keyword path is located against the position-tracked document here.
  //
  // An error with no span at all (nothing in the source to point at) falls back to the first line.
  private def errorRange
     ( focus:    Optional[Tel.Focus],
       error:    Tel.Error,
       lines:    IndexedSeq[String],
       document: Optional[Tel] )
  :   Lsp.Range =

    val span: Span =
      if error.span.exists then error.span
      else focus.lay(Span.empty): focus =>
        if focus.span.exists then focus.span
        else document.lay(Span.empty)(_.locate(focus.pointer).lay(Span.empty)(_.span))

    val fallback =
      val end = lines.headOption.fold(1)(_.length.max(1))
      Lsp.Range(Lsp.Position(0, 0), Lsp.Position(0, end))

    Lsp.Range.from(span).lay(fallback)(fit(_, lines))

  // Reconcile a span-derived range with the document as the client sees it. A span's columns are
  // the parser's, so a column just past the last character of a line (an error at end-of-input, or
  // a saturated span) has to be pulled back within the line, and a range left with no width — a
  // located compound whose length was not recorded — is widened to one character, so that the
  // client has something to underline. A range that spans lines is left alone.
  private def fit(range: Lsp.Range, lines: IndexedSeq[String]): Lsp.Range =
    if range.start.line != range.end.line then range else
      val length = lineLength(lines, range.start.line)
      val start = range.start.character.min(length).max(0)
      val end = range.end.character.max(start).min(length)

      val (from, to) =
        if end > start then (start, end)
        else if start < length then (start, start + 1) // widen forwards, within the line
        else if length > 0 then (length - 1, length)   // at end of line: underline its last character
        else (0, 0)                                    // an empty line: nothing to underline

      Lsp.Range(Lsp.Position(range.start.line, from), Lsp.Position(range.start.line, to))

  private[tel] def hoverAt(text: Text, position: Lsp.Position, resolver: PragmaResolver)
  :   Optional[Lsp.Hover] =

    val (lines, tree) = structure(text)
    val pragma = pragmaOf(lines)

    if position.line == pragma.line && pragma.wellFormed then pragmaHover(pragma, lines, resolver)
    else wordAt(position, lines) match
      case Some((word, start, end)) =>
        val range = Lsp.Range(Lsp.Position(position.line, start), Lsp.Position(position.line, end))
        val index = lines.lift(position.line).fold(0):
          line => tokens(line).stdlib.indexWhere((_, tokenStart, _) => tokenStart == start).max(0)

        hoverMarkup(lines, tree, position, index, word, resolver)
        . let(value => Lsp.Hover(Lsp.MarkupContent(value = value), range))

      case None => Unset

  // Hover on the pragma line reports the schema-resolution status.
  private def pragmaHover(pragma: Pragma, lines: IndexedSeq[String], resolver: PragmaResolver)
  :   Lsp.Hover =

    val range = pragma.identifier.let: id =>
      Lsp.Range(Lsp.Position(pragma.line, id.start), Lsp.Position(pragma.line, id.end))

    val markup = resolver(pragma) match
      case Resolution.Resolved(entry, file, _) =>
        val layers = if entry.layers.s.isEmpty then t"" else t"; layers: ${entry.layers}"
        t"**TEL document** — schema `${entry.name}` (`${entry.id}`$layers), registered at "
        + t"`${file.encode}`"

      case Resolution.Meta(_, _) =>
        t"**TEL schema document** — validated against the built-in `tels` meta-schema"

      case Resolution.Unresolved(identifier) =>
        t"**TEL document** — schema `$identifier` is not registered, so the document is parsed "
        + t"but not validated. Register it with `tel schema add <file>`."

      case Resolution.BadLayers(identifier, detail) =>
        t"**TEL document** — schema `$identifier` resolves, but $detail."

      case Resolution.NoSchema =>
        t"**TEL document** — pragma `${lines.lift(pragma.line).getOrElse("").tt}`"

    Lsp.Hover(Lsp.MarkupContent(value = markup), range)

  // Hover markup for the token under the cursor. The keyword slot (token 0) shows the schema
  // member's declaration; a value slot shows what the schema says about the atom; named-type
  // references and built-in validator names show their own descriptions.
  private def hoverMarkup
     ( lines:    IndexedSeq[String],
       tree:     List[Node],
       position: Lsp.Position,
       index:    Int,
       word:     Text,
       resolver: PragmaResolver )
  :   Optional[Text] =

    if index == 0 then
      keywordMarkup(lines, tree, position, resolver)
      . or(definitions(tree).get(word).map(describe).getOrElse(Unset))
    else definitions(tree).get(word) match
      case Some(node) => describe(node)
      case None =>
        builtinValidatorMarkup(lines, position, word)
        . or(valueSlotMarkup(lines, tree, position, word, resolver))

  private def keywordMarkup
     ( lines: IndexedSeq[String], tree: List[Node], position: Lsp.Position,
       resolver: PragmaResolver )
  :   Optional[Text] =

    documentSchema(lines, resolver) match
      case schema: Tels => enclosing(tree, position, schema) match
        case Slot(struct, node) => fieldMarkup(struct, node.keyword, schema)
        case _                  => Unset
      case _ => Unset

  private def valueSlotMarkup
     ( lines: IndexedSeq[String], tree: List[Node], position: Lsp.Position, atom: Text,
       resolver: PragmaResolver )
  :   Optional[Text] =

    documentSchema(lines, resolver) match
      case schema: Tels => enclosing(tree, position, schema) match
        case Slot(struct, node) =>
          val default = struct.members.readable.collectFirst:
            case field: Tels.Field if field.keyword == node.keyword => field.default
          . getOrElse(Unset)

          memberType(struct, node.keyword, schema)
          . let(valueMarkup(node.keyword, _, default, atom, schema))

        case _ => Unset
      case _ => Unset

  // A validator name on a `validate` line gets the §21.5 blurb for the built-in validators.
  private def builtinValidatorMarkup
     ( lines: IndexedSeq[String], position: Lsp.Position, word: Text )
  :   Optional[Text] =

    val onValidate = lines.lift(position.line).exists: line =>
      tokens(line).stdlib.headOption.exists(_(0) == "validate")

    if onValidate then
      builtinValidators.stdlib.find(_(0) == word)
      . map((name, blurb) => t"**$name** — built-in validator: $blurb")
      . getOrElse(Unset)
    else Unset

  private def describe(node: Node): Text =
    val members = node.children.stdlib.map(_.keyword).distinct.to(List)
    val head = t"**${node.keyword} ${node.atoms.headOption.getOrElse(t"")}**"
    if members.isEmpty then head else t"$head — ${members.join(t", ")}"

  // ── Structure (source scan) ───────────────────────────────────────────────────────────────────
  //
  // Stratiform's parse tree carries no source spans that can disambiguate same-keyword siblings (its
  // position index is internal, and `tel.locate` resolves a keyword *path*), so the position-based
  // features (outline, folding, selection ranges, highlights, go-to-definition) are derived from a
  // lightweight indentation scan of the source text here. Each non-blank, non-comment line is a
  // compound — `<indent><keyword> <atoms…>` — and nesting follows indentation. Source-atom (§14)
  // and literal-atom (§15) payloads are recognised lexically and excluded, so payload lines are
  // never mistaken for compounds; a compound's payload extends its `endLine`, so folding covers it.

  private[tel] case class Node
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

  // A compound line, keeping its source line index, indentation, keyword and inline atoms.
  // `payloadEnd` is the last line of the compound's source- or literal-atom payload (§14, §15), or
  // the compound's own line if it has none.
  private case class Raw
      ( index: Int, indent: Int, keyword: Text, keywordEnd: Int, detail: Text, payloadEnd: Int )

  // The full document as `lines` plus a nested tree of compound `Node`s.
  private[tel] def structure(text: Text): (IndexedSeq[String], List[Node]) =
    val lines = text.s.linesIterator.toIndexedSeq
    val pragma = pragmaOf(lines)
    val sigil = pragma.sigil

    val raws: scm.ListBuffer[Raw] = scm.ListBuffer()
    var index = 0

    // The compound on the immediately preceding line, if any: only such a compound can introduce a
    // source-atom payload (+2 indent levels = +4 spaces, §14) or a literal-atom payload (+3 levels
    // = +6 spaces, §15) on the current line. The payload trigger is checked before the comment
    // rule, because the sigil has no special meaning on the first payload line.
    var adjacent: Optional[Raw] = Unset

    while index < lines.length do
      val line = lines(index)
      val indent = leadingSpaces(line)

      if indent >= line.length || index == pragma.line then
        // A blank line (or the pragma) is no compound, and ends payload adjacency.
        adjacent = Unset
        index += 1
      else adjacent match
        case previous: Raw if indent == previous.indent + 4 =>
          // Source atom (§14): captured lines run to the first non-blank line with fewer leading
          // spaces than the first payload line; interior blank lines are permitted.
          var last = index
          while index < lines.length && {
            val candidate = lines(index)
            val leading = leadingSpaces(candidate)
            leading >= candidate.length || leading >= indent
          } do
            if leadingSpaces(lines(index)) < lines(index).length then last = index
            index += 1

          raws(raws.length - 1) = previous.copy(payloadEnd = last)
          adjacent = Unset

        case previous: Raw if indent == previous.indent + 6 =>
          // Literal atom (§15): the current line is the delimiter line; the payload runs verbatim
          // to the first line whose content matches it exactly (inclusive), or to end of input.
          var end = index + 1
          while end < lines.length && lines(end) != line do end += 1
          val last = if end < lines.length then end else lines.length - 1
          raws(raws.length - 1) = previous.copy(payloadEnd = last)
          adjacent = Unset
          index = last + 1

        case _ =>
          if line.charAt(indent) == sigil then
            // A comment/separator line; it also ends payload adjacency.
            adjacent = Unset
          else
            var end = indent
            while end < line.length && line.charAt(end) != ' ' do end += 1
            var detailStart = end
            while detailStart < line.length && line.charAt(detailStart) == ' ' do detailStart += 1
            val raw = Raw(index, indent, line.substring(indent, end).nn.tt, end,
                line.substring(detailStart).nn.tt, index)
            raws += raw
            adjacent = raw

          index += 1

    // Proscenium's `List` is opaque, so the compiler cannot see that `Nil` and `::` exhaust it;
    // matching the stdlib view instead would collide with the imported cons extractor.
    @scala.annotation.nowarn("msg=match may not be exhaustive")
    def build(items: List[Raw]): List[Node] = items match
      case Nil => Nil
      case head :: tail =>
        val (descendants, rest) = tail.span(_.indent > head.indent)
        val endLine = descendants.lastOption.fold(head.payloadEnd)(_.payloadEnd)
        val atoms = head.detail.cut(t" ").filter(_ != t"")
        Node(head.index, head.indent, head.keyword, head.keywordEnd, head.detail, atoms, endLine,
            build(descendants))
        :: build(rest)

    (lines, build(raws.to(scala.List).to(List)))

  private[tel] def flatten(nodes: List[Node]): List[Node] =
    nodes.stdlib.flatMap(node => node +: flatten(node.children).stdlib).to(List)

  // ── Structure features ────────────────────────────────────────────────────────────────────────

  // The outline follows the compound structure of the document. When the document resolves to a
  // schema, each entry is classified by what the schema says its keyword denotes — a record-typed
  // member is an Object, a scalar-typed member a String, a Flag a Boolean, a select variant an
  // EnumMember; a schema document's own declaration keywords get their natural kinds. Without a
  // schema every entry is a plain Field, as before.
  private[tel] def outline(text: Text, resolver: PragmaResolver): List[Lsp.DocumentSymbol] =
    val (lines, tree) = structure(text)
    val schema = documentSchema(lines, resolver)
    val schemaDoc = isSchemaDocument(lines)
    tree.map(symbol(_, lines, schema, schemaDoc, Nil))

  // Declaration-keyword kinds for a schema document's outline.
  private val schemaDocumentKinds: Map[Text, Lsp.SymbolKind] =
    Map.of:
      scala.Predef.Map
        ( t"record"   -> Lsp.SymbolKind.Struct,
          t"scalar"   -> Lsp.SymbolKind.Class,
          t"select"   -> Lsp.SymbolKind.Enum,
          t"variant"  -> Lsp.SymbolKind.EnumMember,
          t"field"    -> Lsp.SymbolKind.Field,
          t"document" -> Lsp.SymbolKind.Module,
          t"layer"    -> Lsp.SymbolKind.Package,
          t"overlay"  -> Lsp.SymbolKind.Namespace,
          t"name"     -> Lsp.SymbolKind.Property )

  // The symbol kind the resolved schema assigns to `keyword` within the struct reached along
  // `path`: select variants are EnumMembers, flags Booleans, scalar-typed members Strings,
  // record-typed members Objects.
  private def schemaKind(schema: Tels, path: List[Text], keyword: Text): Lsp.SymbolKind =
    structAt(schema, path) match
      case struct: Tels.Struct =>
        val isVariant = struct.members.readable.to(scala.List).exists:
          case reference: Tels.SelectRef =>
            schema.selects.find(_.name == reference.reference)
            . exists(_.variants.readable.exists(_.keyword == keyword))
          case _ => false

        if isVariant then Lsp.SymbolKind.EnumMember
        else memberType(struct, keyword, schema) match
          case Tels.Flag            => Lsp.SymbolKind.Boolean
          case Tels.Scalar(_, _)    => Lsp.SymbolKind.String
          case Tels.Struct(_, _)    => Lsp.SymbolKind.Object
          case Tels.Reference(name) =>
            if schema.records.readable.exists(_.name == name) then Lsp.SymbolKind.Object
            else if schema.selects.readable.exists(_.name == name) then Lsp.SymbolKind.Enum
            else Lsp.SymbolKind.String

          case _ => Lsp.SymbolKind.Field

      case _ => Lsp.SymbolKind.Field

  private def symbol
     ( node:      Node,
       lines:     IndexedSeq[String],
       schema:    Optional[Tels],
       schemaDoc: Boolean,
       path:      List[Text] )
  :   Lsp.DocumentSymbol =

    val kind =
      if schemaDoc then schemaDocumentKinds.get(node.keyword).getOrElse(Lsp.SymbolKind.Field)
      else schema.lay(Lsp.SymbolKind.Field)(schemaKind(_, path, node.keyword))

    Lsp.DocumentSymbol
      ( name           = node.keyword,
        detail         = if node.detail.s.isEmpty then Unset else node.detail,
        kind           = kind,
        range          = Lsp.Range
                           ( Lsp.Position(node.line, node.indent),
                             Lsp.Position(node.endLine, lineLength(lines, node.endLine)) ),
        selectionRange = Lsp.Range
                           ( Lsp.Position(node.line, node.indent),
                             Lsp.Position(node.line, node.keywordEnd) ),
        children       =
          if node.children.isEmpty then Unset
          else node.children.map(symbol(_, lines, schema, schemaDoc, path :+ node.keyword)) )

  private[tel] def folds(node: Node): List[Lsp.FoldingRange] =
    val self =
      if node.endLine > node.line
      then List(Lsp.FoldingRange(startLine = node.line, endLine = node.endLine, kind = t"region"))
      else Nil

    self ::: node.children.flatMap(folds)

  private def selectionRange(position: Lsp.Position, tree: List[Node], lines: IndexedSeq[String])
  :   Lsp.SelectionRange =
    def path(nodes: List[Node]): List[Node] =
      nodes.find(n => position.line >= n.line && position.line <= n.endLine) match
        case Some(node) => node :: path(node.children)
        case None       => Nil

    val ranges = path(tree).map: node =>
      Lsp.Range(Lsp.Position(node.line, node.indent), Lsp.Position(node.endLine, lineLength(lines, node.endLine)))

    val nested = ranges.stdlib.foldLeft(Unset: Optional[Lsp.SelectionRange]): (parent, range) =>
      Lsp.SelectionRange(range, parent)

    nested.lay(Lsp.SelectionRange(Lsp.Range(position, position)))(identity)

  private def highlights(text: Text, position: Lsp.Position): List[Lsp.DocumentHighlight] =
    val nodes = flatten(structure(text)._2)
    nodes.find(_.line == position.line) match
      case Some(target) =>
        nodes.filter(_.keyword == target.keyword).map: node =>
          Lsp.DocumentHighlight
            ( Lsp.Range(Lsp.Position(node.line, node.indent), Lsp.Position(node.line, node.keywordEnd)),
              Lsp.DocumentHighlightKind.Text )

      case None =>
        Nil

  // ── Navigation ────────────────────────────────────────────────────────────────────────────────
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

    out.to(scala.List).to(List)

  // The token under a cursor position, with its column span.
  private def wordAt(position: Lsp.Position, lines: IndexedSeq[String]): Option[(Text, Int, Int)] =
    val found = lines.lift(position.line).flatMap: line =>
      tokens(line).find((_, start, end) => position.character >= start && position.character <= end)

    found.map((token, start, end) => (token.tt, start, end))

  // Named-type definitions in the document, keyed by name (the first atom of a `record`/`scalar`/
  // `select` compound).
  private def definitions(nodes: List[Node]): Map[Text, Node] =
    Map.of:
      flatten(nodes).stdlib.flatMap: node =>
        if definitionKeywords.stdlib.contains(node.keyword) then node.atoms.headOption.map(_ -> node) else None
      . toMap

  // Every whitespace-delimited occurrence of `word` as a whole token, with line and column span.
  private def occurrences(word: Text, lines: IndexedSeq[String]): List[(Int, Int, Int)] =
    lines.zipWithIndex.to(scala.List).flatMap: (line, index) =>
      tokens(line).stdlib.collect { case (token, start, end) if token.tt == word => (index, start, end) }
    . to(List)

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
    Map.of:
      nodes.stdlib.flatMap: node =>
        if definitionKeywords.stdlib.contains(node.keyword) then node.atoms.headOption.map(_ -> node) else None
      . toMap

  private def fieldNode(nodes: List[Node], name: Text): Optional[Node] =
    nodes.find(node => node.keyword == t"field" && node.atoms.headOption.contains(name)).getOrElse(Unset)

  // Descend a schema document from its `document` block along the ancestor keywords of a compound
  // (each a field whose type names a record), yielding the child nodes of the enclosing struct.
  @scala.annotation.nowarn("msg=match may not be exhaustive")
  private def descend(schemaTree: List[Node], context: List[Node], ancestors: List[Text])
  :   Optional[List[Node]] =
    ancestors match
      case Nil => context
      case keyword :: rest => fieldNode(context, keyword) match
        case field: Node => field.atoms.stdlib.lift(1) match
          case Some(typeName) => topLevelDefinitions(schemaTree).get(typeName) match
            case Some(definition) => descend(schemaTree, definition.children, rest)
            case None             => Unset

          case None => Unset
        case _ => Unset

  // The schema-document node that declares `keyword`: a `field`/`variant` in `context` named
  // `keyword`, or a `variant` of a `select` referenced from `context`.
  private def locateMember(schemaTree: List[Node], context: List[Node], keyword: Text): Optional[Node] =
    context.find(node => memberKeywords.stdlib.contains(node.keyword) && node.atoms.headOption.contains(keyword))
    . orElse:
        context.stdlib.filter(_.keyword == t"select").flatMap: reference =>
          reference.atoms.headOption.toList
          . flatMap(name => topLevelDefinitions(schemaTree).get(name).toList)
          . flatMap(_.children.stdlib)
          . filter(node => node.keyword == t"variant" && node.atoms.headOption.contains(keyword))
        . headOption
    . getOrElse(Unset)

  private def schemaDefinition
     ( lines:    IndexedSeq[String],
       tree:     List[Node],
       position: Lsp.Position,
       resolver: PragmaResolver )
  :   List[Lsp.Location] =

    val pragma = pragmaOf(lines)
    resolver(pragma).schemaFile match
      case file: (Path on Linux) => SchemaCache.readText(file).lay(Nil): text =>
          val (schemaLines, schemaTree) = structure(text)
          val uri = t"file://${file.encode}"

          if position.line == pragma.line then List(location(uri, 0, 0, 0))
          else nodeChain(tree, position.line).reverse match
            case node :: ancestors if node.line == position.line =>
              val documentBlock =
                schemaTree.find(_.keyword == t"document").map(_.children).getOrElse(Nil)

              descend(schemaTree, documentBlock, ancestors.reverse.map(_.keyword)).lay(Nil):
                context =>
                  locateMember(schemaTree, context, node.keyword).lay(Nil): target =>
                    tokens(schemaLines(target.line)).stdlib.drop(1).headOption match
                      case Some((_, start, end)) => List(location(uri, target.line, start, end))
                      case None                  =>
                        List(location(uri, target.line, target.indent, target.keywordEnd))

            case _ => Nil
      case _ => Nil

  private[tel] def definitionAt
     ( uri:      Text,
       text:     Text,
       position: Lsp.Position,
       resolver: PragmaResolver )
  :   List[Lsp.Location] =

    val (lines, tree) = structure(text)

    wordAt(position, lines) match
      // A local named-type reference (schema documents) jumps within the file.
      case Some((word, _, _)) => definitions(tree).get(word) match
        case Some(node) => tokens(lines(node.line)).stdlib.drop(1).headOption match
          case Some((_, start, end)) => List(location(uri, node.line, start, end))
          case None                  => Nil

        // Otherwise, try to jump across into the registered schema file.
        case None => schemaDefinition(lines, tree, position, resolver)

      case None => schemaDefinition(lines, tree, position, resolver)

  // ── Validation report (`tel validate`) ──────────────────────────────────────────────────────
  //
  // A command-line front-end to exactly the diagnostics the LSP publishes: the same `diagnose`
  // call, so the CLI and the editor can never disagree about a document. Two renderings: a human
  // one that quotes each offending line with the error span highlighted, and an `--llm` one that
  // gives one line per problem with 1-based positions and no source quotation — the reader has
  // the file; what it needs is an unambiguous address and the reason.

  private def severityOf(diagnostic: Lsp.Diagnostic): Lsp.DiagnosticSeverity =
    diagnostic.severity.or(Lsp.DiagnosticSeverity.Error)

  private def severityWord(severity: Lsp.DiagnosticSeverity): Text = severity match
    case Lsp.DiagnosticSeverity.Error       => t"error"
    case Lsp.DiagnosticSeverity.Warning     => t"warning"
    case Lsp.DiagnosticSeverity.Information => t"note"
    case Lsp.DiagnosticSeverity.Hint        => t"hint"

  private def tinted(severity: Lsp.DiagnosticSeverity, text: Text): Teletype = severity match
    case Lsp.DiagnosticSeverity.Error       => e"${WebColors.Tomato}($text)"
    case Lsp.DiagnosticSeverity.Warning     => e"${WebColors.Gold}($text)"
    case Lsp.DiagnosticSeverity.Information => e"${WebColors.DeepSkyBlue}($text)"
    case Lsp.DiagnosticSeverity.Hint        => e"${WebColors.Gray}($text)"

  // `1 error, 2 warnings` — only the categories that occur, in severity order.
  private def problemCounts(diagnostics: scala.List[Lsp.Diagnostic]): Text =
    scala.List
      ( Lsp.DiagnosticSeverity.Error, Lsp.DiagnosticSeverity.Warning,
        Lsp.DiagnosticSeverity.Information, Lsp.DiagnosticSeverity.Hint )
    . map(severity => severity -> diagnostics.count(severityOf(_) == severity))
    . collect { case (severity, n) if n > 0 =>
        s"$n ${severityWord(severity)}${if n == 1 then "" else "s"}" }
    . mkString(", ").tt

  // One line per problem, positions 1-based, no source text. Columns are inclusive on both ends,
  // so `columns 5-7` names three characters; a zero-width range names the single column at which
  // the problem begins.
  private[tel] def llmReport(path: Text, diagnostics: scala.List[Lsp.Diagnostic]): scala.List[Text] =
    if diagnostics.isEmpty then scala.List(t"$path: no problems")
    else
      val header = t"$path: ${problemCounts(diagnostics)}"

      header :: diagnostics.map: diagnostic =>
        val start = diagnostic.range.start
        val end = diagnostic.range.end

        val where =
          if start.line != end.line then t"lines ${start.line + 1}-${end.line + 1}"
          else if end.character > start.character + 1
          then t"line ${start.line + 1}, columns ${start.character + 1}-${end.character}"
          else t"line ${start.line + 1}, column ${start.character + 1}"

        val code = diagnostic.code.let(code => t" [$code]").or(t"")
        t"$where$code ${severityWord(severityOf(diagnostic))}: ${diagnostic.message}"

  // Each problem quotes its source line with the offending span coloured, above a caret line —
  // colour for terminals that show it, carets for those that do not.
  private[tel] def humanReport
     ( path: Text, lines: IndexedSeq[String], diagnostics: scala.List[Lsp.Diagnostic] )
  :   scala.List[Teletype] =

    if diagnostics.isEmpty then scala.List(e"${WebColors.MediumSeaGreen}(✓) $path: no problems found")
    else
      val width = diagnostics.map(_.range.start.line + 1).max.toString.length

      diagnostics.flatMap: diagnostic =>
        val severity = severityOf(diagnostic)
        val start = diagnostic.range.start
        val code = diagnostic.code.let(code => e" $Bold($code)").or(e"")
        val location = t"$path:${start.line + 1}:${start.character + 1}"

        val header =
          e"${tinted(severity, severityWord(severity))} $Bold($location)$code: ${diagnostic.message}"

        lines.lift(start.line) match
          case Some(line) =>
            // The span to highlight: to the range's end on a single-line range, or to the end of
            // the line when the range continues past it. A zero-width range still gets one caret.
            val from = start.character.min(line.length)
            val to =
              if diagnostic.range.end.line == start.line
              then diagnostic.range.end.character.min(line.length).max(from)
              else line.length

            val number = (start.line + 1).toString
            val gutter = " ".repeat(width - number.length).nn + number
            val carets = "^".repeat((to - from).max(1)).nn

            scala.List
              ( header,
                e"  $gutter │ ${line.substring(0, from).nn}${tinted(severity, line.substring(from, to).nn.tt)}${line.substring(to).nn}",
                e"  ${" ".repeat(width).nn} │ ${" ".repeat(from).nn}${tinted(severity, carets.tt)}" )

          case None => scala.List(header)
      :+ e""
      :+ e"$path: ${problemCounts(diagnostics)}"

  private def validateFile(file: Path on Local, llm: Boolean)(using Stdio, Environment, System): Exit =
    recover:
      case error: Path.Error =>
        Out.println(t"tel: could not resolve ${file.encode}: ${error.message.text}")
        Exit.Fail(1)

    . protect:
        SchemaCache.readText(file.encode.as[Path on Linux]) match
          case text: Text =>
            val registry: Registry = safely(SchemaCache.directory)
            val resolver = PragmaResolver(registry)
            val lines = text.s.linesIterator.toIndexedSeq

            val diagnostics = diagnose(text, resolver).stdlib.sortBy: diagnostic =>
              (diagnostic.range.start.line, diagnostic.range.start.character)

            if llm then llmReport(file.encode, diagnostics).foreach(Out.println(_))
            else humanReport(file.encode, lines, diagnostics).foreach(Out.println(_))

            val errors = diagnostics.count(severityOf(_) == Lsp.DiagnosticSeverity.Error)
            if errors > 0 then Exit.Fail(1) else Exit.Ok

          case _ =>
            Out.println(t"tel: could not read ${file.encode}")
            Exit.Fail(1)

  // ── Live message log ────────────────────────────────────────────────────────────────────────
  //
  // The tool runs as an Ethereal daemon: one JVM hosts every `tel` invocation, and `TelServer` is a
  // singleton loaded once in it. So `logSubscribers` is shared daemon-wide, and a `tel lsp --log`
  // session can observe — live — the messages that the editor's `tel lsp` session sends and receives.
  // Each `--log` subscriber gets its own `Spool`; the `Lsp.Observer` registered with `Lsp.listen`
  // broadcasts every message, tagged `recv` (client → server) or `send` (server → client), to all
  // subscribers.

  private val logSubscribers: scm.HashSet[Relay[Text]] = scm.HashSet()

  private def broadcastLog(marker: Text, message: Text): Unit =
    logSubscribers.synchronized(logSubscribers.foreach(_.put(t"$marker $message")))

  private object TrafficLog extends Lsp.Observer:
    def received(message: Text): Unit = broadcastLog(t"recv", message)
    def sent(message: Text): Unit = broadcastLog(t"send", message)

  // Streams messages to stdout until interrupted (Ctrl-C); used by `tel lsp --log`.
  private def streamLog()(using Stdio): Unit =
    val spool = Relay[Text]()
    logSubscribers.synchronized(logSubscribers.add(spool))
    try
      Out.println(t"Streaming messages sent/received by the tel language server. Press Ctrl-C to stop.")
      spool.lazyList.iterator.each: message =>
        Out.println(message)
    // Deliberately a `catch`, not a `recover`: `InterruptedException` is the JVM's thread-interrupt
    // signal (Ctrl-C, here), not a `Tactic` obligation, so there is nothing for a statically-checked
    // handler to discharge.
    catch case _: InterruptedException => ()
    finally logSubscribers.synchronized(logSubscribers.remove(spool))

  // ── Subcommands ─────────────────────────────────────────────────────────────────────────────
  //
  // `tel lsp` runs the language server over stdio (what an editor launches); `tel lsp --log` streams
  // the messages a running server sends and receives. Further subcommands can be added as new `case`
  // branches.

  private val LspCommand       = Subcommand("lsp", "run the TEL language server over stdio (for editors)")
  private val SchemaCommand    = Subcommand("schema", "manage the schema registry")
  private val AddCommand       = Subcommand("add", "add a schema file to the registry")
  private val ListCommand      = Subcommand("list", "list registered schemas")
  private val SignatureCommand = Subcommand("signature", "show a schema's palimpsest signature")
  private val ValidateCommand  = Subcommand("validate", "parse and validate a TEL file, reporting its errors")

  def main(args: Array[Text]): Unit = cli:
    arguments match
      case LspCommand() :: rest if rest.stdlib.exists(argument => argument() == t"--log") =>
        execute:
          streamLog()
          Exit.Ok

      case LspCommand() :: _ =>
        execute:
          // Resolved here, where the invoker's `Environment` is in scope, and closed over by the
          // handlers: `listen` keeps everything stateful within its own frame.
          val registry: Registry = safely(SchemaCache.directory)
          val resolver = PragmaResolver(registry)

          // The server loop runs under `supervise`, whose `Async.Error` is the one obligation this
          // file does not discharge locally. It is handled here rather than by a blanket
          // `throwUnsafely` so that every other raising call in this file stays statically checked.
          // The report goes to `Err`: stdout is the LSP transport, and writing to it would corrupt
          // the message stream.
          recover:
            case error: Async.Error =>
              Err.println(t"tel: the language server terminated abnormally: ${error.message.text}")
              Exit.Fail(1)

          . protect:
            supervise:
              // Scoped here, not at the object level: `Lsp.arguments` (the `executeCommand` payload)
              // would otherwise shadow Exoskeleton's CLI `arguments`, which `main` matches on.
              import Lsp.*

              Lsp.listen(t"tel", t"0.1.0", TrafficLog):
                opened:
                  client.publishDiagnostics(document.uri, diagnose(document.text, resolver))

                // The document store applies incremental edits upstream, so `document.text` is already
                // the post-edit text; the change events themselves are not needed.
                changed:
                  client.publishDiagnostics(document.uri, diagnose(document.text, resolver))

                // Clear the document's diagnostics when it closes, so they do not linger in the
                // client's problems panel for a file that is no longer open.
                closed:
                  client.publishDiagnostics(document.uri, Nil)

                hover(hoverAt(document.text, position, resolver))
                // Refactorings between inline-atom and compound-child presentation, offered only
                // when verified meaning-preserving against the resolved schema.
                codeActions(codeActionsAt(document.uri, document.text, range, resolver))
                // A space triggers completion in the slot after a keyword: a `select`-typed field's
                // variants, a member declaration's flags, or the pragma's schema identifier.
                complete(t" ")(completions(document.text, position, resolver))
                definition(definitionAt(document.uri, document.text, position, resolver))

                references:
                  val lines = structure(document.text)._1

                  wordAt(position, lines) match
                    case Some((word, _, _)) =>
                      occurrences(word, lines).map: (line, start, end) =>
                        location(document.uri, line, start, end)

                    case None =>
                      Nil

                documentSymbols(outline(document.text, resolver))

                foldingRanges(structure(document.text)._2.flatMap(folds))

                selectionRanges:
                  val (lines, tree) = structure(document.text)
                  positions.map(selectionRange(_, tree, lines))

                documentHighlights(highlights(document.text, position))

            Exit.Ok

      case SchemaCommand() :: ListCommand() :: _ =>
        execute(schemaList())

      // `Pathname` (rather than a bare `Argument`) both resolves the argument — relative or absolute —
      // against the client's working directory, and registers filename tab-completions for it.
      case SchemaCommand() :: AddCommand() :: Pathname(file) :: _ =>
        execute(schemaAdd(file))

      case SchemaCommand() :: SignatureCommand() :: Argument(name) :: layers =>
        execute(schemaSignature(name, layers.map(_())))

      // `--llm` selects the quotation-free, position-only report an LLM can act on directly.
      case ValidateCommand() :: Pathname(file) :: rest =>
        execute(validateFile(file, rest.stdlib.exists(_() == t"--llm")))

      case _ =>
        execute:
          Out.println(t"Usage:")
          Out.println(t"  tel lsp                              run the language server over stdio")
          Out.println(t"  tel lsp --log                        stream the server's message traffic")
          Out.println(t"  tel schema list                      list registered schemas")
          Out.println(t"  tel schema add <file>                add a schema to the registry")
          Out.println(t"  tel schema signature <name> [layer…] show a schema's palimpsest signature")
          Out.println(t"  tel validate <file> [--llm]          parse and validate a TEL file")
          Exit.Fail(1)

  // The `tel schema …` subcommands each end at a `recover` boundary rather than a `catch`: the
  // handler names the error types the body can actually raise, so the compiler checks that every
  // obligation is discharged and a new raising call cannot be added without being handled here. A
  // bare `catch case error: Error` would swallow whatever arrived, including errors these bodies
  // were never meant to absorb.
  private def schemaList()(using Stdio, Environment, System): Exit =
    recover:
      case error: Path.Error =>
        Out.println(t"tel: could not list schemas: ${error.message.text}")
        Exit.Fail(1)

    . protect:
        val entries = SchemaCache.entries(SchemaCache.directory)
        if entries.isEmpty then Out.println(t"No schemas registered. Add one with `tel schema add`.")
        else
          val table = Scaffold[SchemaCache.Entry]
            ( Column(t"Name")(_.name),
              Column(t"BASE-256 id")(_.id),
              Column(t"Layers")(_.layers) )

          Out.println(table.tabulate(entries).grid(120).render.join(t"\n"))
        Exit.Ok

  // `Pathname` yields a `Path on Local` (the platform the client is running on); the registry — and
  // everything else that touches it — is typed `Path on Linux`, so the resolved path is re-encoded
  // into that world at this one boundary.
  private def schemaAdd(file: Path on Local)(using Stdio, Environment, System): Exit =
    def failed(error: Error): Exit =
      Out.println(t"tel: could not add schema: ${error.message.text}")
      Exit.Fail(1)

    recover:
      case error: Bintel.Error     => failed(error)
      case error: Tel.Error        => failed(error)
      case error: Io.Error         => failed(error)
      case error: Truncation.Error => failed(error)
      case error: Path.Error       => failed(error)

    . protect:
        val entry = SchemaCache.add(SchemaCache.directory, file.encode.as[Path on Linux])
        Out.println(t"Added schema `${entry.name}` (id ${entry.id}).")
        Exit.Ok

  private def schemaSignature(name: Text, layers: List[Text])(using Stdio, Environment, System): Exit =
    def failed(error: Error): Exit =
      Out.println(t"tel: could not compute signature: ${error.message.text}")
      Exit.Fail(1)

    recover:
      case error: Bintel.Error => failed(error)
      case error: Tel.Error    => failed(error)
      case error: Path.Error   => failed(error)

    . protect:
        SchemaCache.load(SchemaCache.directory, name) match
          case tel: Tel =>
            Out.println(SchemaCache.signature(tel, layers))
            Exit.Ok

          case _ =>
            Out.println(t"tel: no schema named `$name` in the registry")
            Exit.Fail(1)
