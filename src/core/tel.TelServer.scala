package tel

import soundness.*

// `soundness` re-exports Proscenium's collections, but an *exported* opaque type does not carry its
// companion's extensions into implicit scope — `.stdlib`, the `::` cons and the `.to(List)` factory
// would all be unavailable — so the collection types come straight from Proscenium, as the Soundness
// modules themselves do. A named import outranks the `soundness` wildcard.
import proscenium.{List, Nil, Chain, Map, Set, `::`}

// Appending to a `List`, taking its length and reading it by position are all O(n) on a linked
// structure, and all gated behind this acknowledgement. The lists here are the errors in one
// document, the tokens on one line and the phrases of one pragma, so linear cost is not a concern.
import dysasymptotics.linearSize
// Type names and diagnostics are ordered for display: by code point, which for the identifiers
// TEL admits is alphabetical order. `Text` is comparable only once a collation has been chosen,
// and the given which makes it so is lexically scoped.
import soundness.collationComparable
import collations.codepointCollation
import sortingAlgorithms.timsort
import backstops.stackTraceBackstop
import codepages.utf8Codepage
import errorDiagnostics.emptyDiagnostics
import executives.completionsExecutive
import interpreters.posixInterpreter
import parsing.trackPositions
import probates.awaitProbate
import threads.virtualThreads
import systems.javaBaseSystem
import pathInterfaces.pathOnLinux
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
  // The bolt code of the error the specification numbers `number` — its family in the hundreds,
  // so that 107 is ↯/t.1.7 (§19.3).
  private def bolt(number: Int): Text = t"↯/t.${number/100}.${number%100}"

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
  extends Error(m"${items.size} TEL errors"):
    def add(focus: Optional[Tel.Focus], error: Tel.Error): Accrued = Accrued(items :+ (focus, error))

  // The validators a document is checked with (§21.4, §21.5): the four built-ins, which TELS
  // itself relies on, run for real; an application-defined validator name — which only the
  // application can run — is treated as satisfied here. §21.4's rule that an unknown validator
  // is never silently satisfied binds the application, not an editor that has no way to bind it;
  // hover shows the name so the reader knows the value is unchecked. Struct validators are
  // application-defined without exception.
  private val editorValidators: Tel.Validator.Registry = new Tel.Validator.Registry:
    override def apply(request: Tel.Validator.Request): Tel.Validator.Response =
      Tel.Validator.Registry.builtins(request) match
        case Tel.Validator.Response.Invalid(Tel.Validator.Diagnostic.Scalar(message, _))
             if message.starts(t"unknown validator") =>
          Tel.Validator.Response.Valid

        case Tel.Validator.Response.Invalid(Tel.Validator.Diagnostic.Struct(_, _)) =>
          Tel.Validator.Response.Valid

        case response => response

  // The two codecs the specification defines (BinTEL §8.4), so that acceptances validate: every
  // BASE-256 string is an accepted `base-256` text, and a `schema-signature` text must further
  // decode to bytes that are structurally a signature (length and cadence byte, §8.2).
  private object Base256Codec extends Tel.Codec:
    def encode(text: Text): Tel.Codec.Encoded =
      safely(Base256.decodeStrict(text))
      . lay(Tel.Codec.Encoded.Invalid(t"not a BASE-256 string"))(Tel.Codec.Encoded.Bytes(_))

    def decode(bytes: Data): Tel.Codec.Decoded = Tel.Codec.Decoded.Value(Base256.encode(bytes))

  private object SignatureCodec extends Tel.Codec:
    def encode(text: Text): Tel.Codec.Encoded =
      safely(Base256.decodeStrict(text)).lay(Tel.Codec.Encoded.Invalid(t"not a BASE-256 string")):
        bytes =>
          if safely(SchemaSignature.componentCount(bytes)).present
          then Tel.Codec.Encoded.Bytes(bytes)
          else
            Tel.Codec.Encoded.Invalid
              ( t"not a schema signature: the length must be 33 or 37 + 2·(n − 2) bytes and the "
                + t"bytes must XOR to the cadence byte 0x79" )

    def decode(bytes: Data): Tel.Codec.Decoded = Tel.Codec.Decoded.Value(Base256.encode(bytes))

  // The codec binding (§21.7) a document is checked with: the two specified codecs resolve; any
  // other encoding name is application-defined, and its values go unchecked — reported by
  // `diagnostic` as a warning rather than the ↯/t.3.13 error an application would raise.
  private val editorCodecs: Tel.Codec.Bindings = new Tel.Codec.Bindings:
    def apply(name: Text): Optional[Tel.Codec] = name match
      case t"base-256"         => Base256Codec
      case t"schema-signature" => SignatureCodec
      case _                   => Unset

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
  private def wellFormed(line: Text): Boolean = line match
    case r"tel [0-9]+\.[0-9]+( .*)?" => true
    case _                           => false

  // A well-formed BASE-256 schema signature phrase (§8.1): 33 characters for one component,
  // `37 + 2·(n − 2)` for `n ≥ 2`, every character a Unicode letter or decimal digit.
  private def isSignatureToken(token: Text): Boolean = token match
    case r"[\p{L}\p{Nd}]{33}"                       => true
    case r"[\p{L}\p{Nd}]{37}([\p{L}\p{Nd}]{2})*" => true
    case _                                          => false

  // A sigil phrase (§6): a single character which is neither a letter, a digit nor the `+` that
  // introduces a layer selection.
  private def isSigilToken(token: Text): Boolean = token match
    case r"[^\p{L}\p{Nd}+]" => true
    case _                   => false

  // How a pragma phrase after the version reads, by its form alone (§8). The sigil is recognised
  // only as the last phrase.
  private enum Phrase derives CanEqual:
    case Layer, Reference, Signature, Sigil, Unclassified

  private def phrase(token: Text, last: Boolean): Phrase =
    if token.starts(t"+") && token.length > 1 then Phrase.Layer
    else if token.contains(t"/") && token.length > 1 then Phrase.Reference
    else if isSignatureToken(token) then Phrase.Signature
    else if last && isSigilToken(token) then Phrase.Sigil
    else Phrase.Unclassified

  private[tel] def pragmaOf(lines: Sequence[Text]): Pragma =
    val index = if lines.head.lay(false)(_.starts(t"#!")) then 1 else 0

    lines.at(index.z).lay(Pragma(index, false, Unset, Nil, Unset, '#')): line =>
      // The phrases after `tel` and the version, each with the form it has.
      val phrases = tokens(line).skip(2)

      val classified = phrases.map: token =>
        phrase(token.text, phrases.last.lay(false)(_.start == token.start)) -> token

      // The first phrase of a form is the one that counts; a second is the parser's to report.
      def first(form: Phrase): Optional[PragmaId] =
        classified.seek(_(0) == form).let: (_, token) =>
          PragmaId(token.text, token.start, token.end)

      val layers = classified.filter(_(0) == Phrase.Layer).map: (_, token) =>
        PragmaId(token.text.skip(1), token.start, token.end)

      Pragma
        ( index, wellFormed(line), first(Phrase.Reference), layers, first(Phrase.Signature),
          first(Phrase.Sigil).let(_.name.at(Prim)).or('#') )

  // Does this pragma schema identifier name TELS, the schema-of-schemas (§20.5)? The meta-schema's
  // canonical coordinate is `specification.tel/tels`, pinned in the specification to `:2.1.0`;
  // Stratiform's `Reference.isTels` matches the module alone, with or without — and whatever — its
  // version pin, so `:2.1.0` resolves to `Tels.Axiom.tels` even while Stratiform's own
  // `Reference.tels` still names `:2.0.0`.
  private[tel] def namesTels(identifier: Text): Boolean =
    Tel.Pragma.Reference.parse(identifier).lay(false)(_.isTels)

  // The outcome of resolving a document's pragma schema identification. `Unresolved` is
  // distinguished from `NoSchema` so that an identifier which matches nothing in the registry can
  // be reported to the user, rather than validation being skipped invisibly; `BadLayers` reports
  // a resolved schema whose layer selection or signature does not agree with the rest of the
  // pragma (§8.1), with the diagnostic code that names the disagreement. A `Resolved` schema
  // carries the layers it was composed with — the pragma's selection, or the components a
  // signature named — and the signature of that composition.
  private[tel] enum Resolution:
    case NoSchema
    case Meta(meta: Tels, file: Optional[Path on Linux])

    case Resolved
      ( entry:     SchemaCache.Entry,
        file:      Path on Linux,
        schema:    Tels,
        layers:    List[Text],
        signature: Text )

    case Unresolved(identifier: Text)
    case BadLayers(identifier: Text, detail: Text, code: Text)

    // The schema to validate and navigate with, if resolution succeeded.
    def tels: Optional[Tels] = this match
      case Meta(meta, _)                => meta
      case Resolved(_, _, schema, _, _) => schema
      case _                            => Unset

    // The registry file backing the schema (for cross-file go-to-definition).
    def schemaFile: Optional[Path on Linux] = this match
      case Meta(_, file)              => file
      case Resolved(_, file, _, _, _) => file
      case _                          => Unset

  // The registry lookup name for a pragma schema identifier: a LIRA reference resolves by its
  // module-name tail (the local tel cache stores schemas as `<name>.tel`, and §2.7 of the design
  // binds the published name to the declared name), with any `:version`/`:tag` selector stripped;
  // a signature (or a bare name, for direct calls) is used verbatim.
  private def lookupName(identifier: Text): Text =
    if identifier.contains(t"/")
    then identifier.cut(t":").head.cut(t"/").last
    else identifier

  // Resolves pragma schema identifications against the registry, memoized per identification:
  // matching a BASE-256 signature re-reads and re-hashes every cached schema, which is too slow
  // to repeat on every keystroke. The memo is dropped whenever the registry directory's
  // modification time changes (i.e. `tel schema add` ran).
  private[tel] class PragmaResolver(registry: Registry):
    // What has been resolved since the registry last changed.
    private case class Memo(stamp: Long, resolutions: Map[Text, Resolution])

    private val mutex: Mutex = Mutex()
    private val memo = Atomic(Memo(0L, Map()))

    private def registryStamp: Long = registry.lay(0L)(SchemaCache.modified(_))

    // A pragma carrying both a reference and a signature is resolved by the signature, which is
    // authoritative (§8.1), and the reference must agree with it (§8.2): the schema the signature
    // decodes to must be the referenced one.
    def apply(pragma: Pragma): Resolution =
      pragma.identifier.lay(Resolution.NoSchema): id =>
        val expected =
          if pragma.signature.present then pragma.reference.let(ref => lookupName(ref.name))
          else Unset

        apply(id.name, pragma.layerNames, expected)

    def apply(identifier: Text): Resolution = apply(identifier, Nil, Unset)

    def apply(identifier: Text, layers: List[Text]): Resolution = apply(identifier, layers, Unset)

    def apply(identifier: Text, layers: List[Text], expected: Optional[Text]): Resolution =
      mutex:
        val stamp = registryStamp
        val current = memo().when(_.stamp != stamp)(_ => Memo(stamp, Map()))
        val key = (identifier :: layers).join(t" +") + expected.let(name => t" =$name").or(t"")

        current.resolutions.at(key).or:
          resolve(identifier, layers, expected).tap: resolution =>
            memo() = Memo(stamp, current.resolutions.define(key, resolution))

    def entries: List[SchemaCache.Entry] = registry.lay(Nil)(SchemaCache.entries(_))

    // The layers the registered schema `name` declares, in declaration order.
    def layersOf(name: Text): List[Text] = registry.lay(Nil)(SchemaCache.layerNames(_, name))

    private def resolve(identifier: Text, layers: List[Text], expected: Optional[Text])
    :   Resolution =

      val meta = namesTels(identifier)

      registry.lay(if meta then Resolution.Meta(Tels.Axiom.tels, Unset) else Resolution.NoSchema):
        directory =>
          if meta then
            val file = SchemaCache.lookup(directory, t"tels") match
              case SchemaCache.Lookup.Found(file, _, _, _) => file
              case _                                       => Unset

            Resolution.Meta(Tels.Axiom.tels, file)
          else
            SchemaCache.lookup(directory, lookupName(identifier), layers) match
              case SchemaCache.Lookup.Found(file, schema, selected, signature) =>
                val entry =
                  SchemaCache.describe(file).or(SchemaCache.Entry(identifier, identifier, t""))

                if expected.lay(false)(_ != entry.name) then
                  Resolution.BadLayers
                    ( identifier,
                      t"the signature identifies the schema `${entry.name}`, not the referenced "
                      + t"schema `${expected.or(t"")}`",
                      t"↯/t.6.3" )
                else
                  Resolution.Resolved(entry, file, schema, selected, signature)

              case SchemaCache.Lookup.UnknownLayer(_, layer) =>
                Resolution.BadLayers
                  ( identifier, t"the selected layer `$layer` is not one the schema declares",
                    t"↯/t.6.2" )

              case SchemaCache.Lookup.LayerOrder(_) =>
                Resolution.BadLayers
                  ( identifier,
                    t"the `+` layer selections are not in the schema's declaration order",
                    t"↯/t.1.24" )

              case SchemaCache.Lookup.Disagreement(_, detail) =>
                Resolution.BadLayers(identifier, detail, t"↯/t.6.3")

              case SchemaCache.Lookup.Missing =>
                Resolution.Unresolved(identifier)

  private[tel] def diagnose(text: Text, resolver: PragmaResolver): List[Lsp.Diagnostic] =
    val lines = linesOf(text)
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
          // (`tel.as[Tels]`), catching schema-validity errors (↯/t.2). Any other pragma
          // schema — a LIRA reference or BASE-256 signature — is resolved against the local registry
          // populated by `tel schema add`. The two meta-schema passes are ventured separately so
          // that a failure in the first still lets the second contribute its errors.
          guard:
            val tel = parsed()
            document = tel

            resolution match
              case Resolution.Meta(_, _) =>
                venture(Tel.Type.assign(tel, Tels.Axiom.tels, editorValidators, editorCodecs))

                // Reconstruction alone checks only the document's shape; `Validation.validate`
                // composes the layers and runs the §20.1 schema-validity battery (↯/t.2.1–↯/t.2.21)
                // over the result. The composed schema feeds the reference-coherence pass.
                val validated = venture(Tels.Validation.validate(tel.as[Tels]))
                guard:
                  composed = validated()

              case Resolution.Resolved(_, _, schema, _, _) =>
                venture(Tel.Type.assign(tel, schema, editorValidators, editorCodecs))

              case _ =>
                ()

    // The two meta-schema passes often report the same defect, so schema errors are collapsed by
    // reason. This is deliberately not applied when the parse failed: parse errors legitimately
    // repeat one reason at several positions, and the `guard` guarantees the two sets are never
    // both present.
    // ↯/t.2.24 (a scalar declaring neither `validate` nor `pattern`) was withdrawn from the
    // specification: an unconstrained scalar is valid. Stratiform 0.67 still raises it, so it is
    // dropped here; the registry's `validated` composes such a schema without the battery.
    val retired = accrued.items.filter((_, error) => error.reason.number != 224)

    val collapsed =
      if document.absent then retired
      else retired.deduplicate((_, error) => (error.reason.number, error.reason))

    // Stratiform's validity battery now checks reference coherence itself (↯/t.2.9/↯/t.2.17), and — since
    // Soundness 0.67.0 — duplicate definition names (↯/t.2.10) too, but it aborts at the first defect
    // without a source span, so the entry would land at the head of the file. Those entries are
    // dropped for a schema document and re-located precisely: the coherence pair by the
    // incoherences pass below, against the composed schema, and `DuplicateDefinition` by the
    // `duplicateDefinitions` source scan, on the second declaration's name atom.
    val coherenceReasons = List(Tel.Error.Reason.UnresolvedReference,
        Tel.Error.Reason.ReferenceKindMismatch, Tel.Error.Reason.DuplicateDefinition)

    val entries = resolution match
      case Resolution.Meta(_, _) if !document.absent =>
        if composed.absent then composed = document.let: tel =>
          safely(Tels.Layers.compose(tel.as[Tels]))

        collapsed.filter((_, error) => !coherenceReasons.has(error.reason))

      case _ =>
        collapsed

    // Stratiform's `Reconstructor` leaves §20.1 schema-validity checking out of scope, so the one
    // cheap, high-value case — duplicate or built-in-colliding definition names (↯/t.2.10) — is
    // checked here against the source scan, on the offending name atom.
    val duplicates = resolution match
      case Resolution.Meta(_, _) if !document.absent => duplicateDefinitions(lines)
      case _                                         => Nil

    // Reference-coherence findings (↯/t.2.9/↯/t.2.17), located on the offending TypeName atom in the
    // source scan. Computed only when the whole validity battery passed, so they are never
    // cascade noise from a malformed schema.
    val incoherent: List[Lsp.Diagnostic] = composed.lay(Nil): schema =>
      SchemaCache.incoherences(schema).map: (name, reason) =>
        incoherenceDiagnostic(name, reason, lines)

    // An identifier that matches nothing in the registry gets a diagnostic of its own: the document
    // is valid TEL, but it is not being validated, which would otherwise be invisible to the user.
    // A bad `+` layer selection against a resolved schema is an error (§8.1): a selection out of
    // declaration order is ↯/t.1.24, and an undeclared layer name — like a signature that disagrees
    // with the reference or selections beside it — is a runtime resolution error, which carries
    // no code and is reported under one of the tool's own, in ↯/t.6.
    val unresolved = resolution match
      case Resolution.Unresolved(identifier) =>
        val range = pragma.identifier
          . lay(Lsp.Range(Lsp.Position(pragma.line, 0), Lsp.Position(pragma.line, 1))): id =>
              Lsp.Range(Lsp.Position(pragma.line, id.start), Lsp.Position(pragma.line, id.end))

        List(Lsp.Diagnostic
          ( range    = range,
            severity = Lsp.DiagnosticSeverity.Information,
            code     = t"↯/t.6.1",
            source   = t"tel",
            message  = t"Schema `$identifier` is not registered, so this document is parsed but "
                       + t"not validated. Register the schema with `tel schema add <file>`." ))

      case Resolution.BadLayers(identifier, detail, code) =>
        // The layer phrases when the selection is at fault; the whole identification otherwise.
        val phrases = if code == t"↯/t.6.3" then Nil else pragma.layers
        val start = phrases.head.let(_.start).or(pragma.identifier.let(_.start).or(0))
        val end = phrases.last.let(_.end).or(pragma.identifier.let(_.end).or(1))

        List(Lsp.Diagnostic
          ( range    = Lsp.Range(Lsp.Position(pragma.line, start), Lsp.Position(pragma.line, end)),
            severity = Lsp.DiagnosticSeverity.Error,
            code     = code,
            source   = t"tel",
            message  = t"Schema `$identifier` resolves, but $detail." ))

      case _ => Nil

    (entries.map(diagnostic(_, lines, document)) + duplicates + incoherent + unresolved)
    . deduplicate(entry => (entry.range, entry.code, entry.message))

  // ↯/t.2.10 (§20.1): two base Definitions sharing a name, or a Definition using a predefined built-in
  // name. Located on the second (or built-in-colliding) definition's name atom.
  private def duplicateDefinitions(lines: Sequence[Text]): List[Lsp.Diagnostic] =
    def diagnostic(line: Int, name: Token, message: Text): Lsp.Diagnostic =
      Lsp.Diagnostic
        ( range    = Lsp.Range(Lsp.Position(line, name.start), Lsp.Position(line, name.end)),
          severity = Lsp.DiagnosticSeverity.Error,
          code     = t"↯/t.2.10",
          source   = t"tel",
          message  = message )

    // Every unindented definition line, as its line number and its name atom.
    val declared: List[(Int, Token)] = numbered(lines).flatMap: (line, index) =>
      tokens(line) match
        case keyword :: name :: _
            if leadingSpaces(line) == 0 && definitionKeywords.has(keyword.text) =>
          List((index, name))

        case _ => Nil

    declared.flatMap: (index, name) =>
      if builtinTypeNames.has(name.text)
      then List(diagnostic(index, name, t"`${name.text}` is a predefined built-in type name"))
      else if declared.exists((earlier, other) => earlier < index && other.text == name.text)
      then List(diagnostic(index, name, t"a definition named `${name.text}` is already declared"))
      else Nil

  // A reference-coherence finding, located on the first occurrence of the offending TypeName in a
  // type slot: the third token of a `field`/`variant` line, or the second token of an indented
  // `select` line (a `SelectRef`; the definition form is unindented). Falls back to the first
  // line when the scan cannot find it, which only a scan/parse disagreement would cause.
  private def incoherenceDiagnostic(name: Text, reason: Tel.Error.Reason, lines: Sequence[Text])
  :   Lsp.Diagnostic =

    val slots: List[(Int, Token)] = numbered(lines).flatMap: (line, index) =>
      tokens(line) match
        case keyword :: _ :: slot :: _
            if memberKeywords.has(keyword.text) && slot.text == name =>
          List((index, slot))

        case keyword :: slot :: _
            if keyword.text == t"select" && leadingSpaces(line) > 0 && slot.text == name =>
          List((index, slot))

        case _ => Nil

    val (line, start, end) = slots.head.lay((0, 0, 1)): (index, slot) =>
      (index, slot.start, slot.end)

    Lsp.Diagnostic
      ( range    = Lsp.Range(Lsp.Position(line, start), Lsp.Position(line, end)),
        severity = Lsp.DiagnosticSeverity.Error,
        code     = bolt(reason.number),
        source   = t"tel",
        message  = m"$reason".text )

  // ── Schema-aware information (hover + completion) ──────────────────────────────────────────────
  //
  // When a document resolves to a registered schema, the compound keywords correspond to schema
  // members. These helpers navigate the schema alongside the document's compound tree to surface each
  // member's type, cardinality, default and description.

  // The chain of compound nodes whose block contains `line`, outermost first.
  private def nodeChain(nodes: List[Node], line: Int): List[Node] =
    nodes.seek(node => line >= node.line && line <= node.endLine).lay(Nil): node =>
      node :: nodeChain(node.children, line)

  // The struct a `Reference`/`Struct` type denotes (a record reference resolves to its struct).
  private def structOf(fieldType: Tels.Type, schema: Tels): Optional[Tels.Struct] =
    fieldType match
      case struct: Tels.Struct => struct

      case Tels.Reference(name) =>
        schema.records.seek(_.name == name).let: record =>
          Tels.Struct(record.members, record.validators)

      case _ => Unset

  // The variants of the select named `reference`, or none if the schema defines no such select.
  private def variantsOf(reference: Text, schema: Tels): List[Tels.Variant] =
    schema.selects.seek(_.name == reference).lay(Nil)(_.variants.to[List])

  // The type that the member `keyword` of `struct` denotes: a field's declared type, or — because
  // a `select` reference is flattened so its variants appear as sibling keywords — the variant
  // type of a matching variant of a referenced select.
  private def memberType(struct: Tels.Struct, keyword: Text, schema: Tels): Optional[Tels.Type] =
    val members = struct.members.to[List]

    members.reap { case field: Tels.Field if field.keyword == keyword => field.fieldType }.or:
      members.flatMap:
        case reference: Tels.SelectRef => variantsOf(reference.reference, schema)
        case _                         => Nil
      . seek(_.keyword == keyword).let(_.variantType)

  // The struct that a member's own members live in, one keyword deeper.
  private def memberStruct(struct: Tels.Struct, keyword: Text, schema: Tels): Optional[Tels.Struct] =
    memberType(struct, keyword, schema).let(structOf(_, schema))

  // The struct reached by descending `schema.document` along a keyword path.
  private def structAt(schema: Tels, path: List[Text]): Optional[Tels.Struct] =
    path.fold(schema.document: Optional[Tels.Struct]): (current, keyword) =>
      current.let(memberStruct(_, keyword, schema))

  private def typeLabel(fieldType: Tels.Type, schema: Tels): Text =
    fieldType match
      case Tels.Reference(name) => name
      case Tels.Flag            => t"Flag"
      case Tels.Struct(_, _)    => t"record"

      // A declared `encoding` (§21.7) and the `pattern` constraints (§21.8) are part of what the
      // scalar accepts — the codec's encoder and each pattern are further validity constraints —
      // so they belong in the label beside the validators.
      case Tels.Scalar(validators, encoding, patterns) =>
        val base = if validators.length == 0 then t"scalar" else validators.to[List].join(t"+")
        val constrained =
          if patterns.length == 0 then base
          else t"$base ~ ${patterns.to[List].map(pattern => t"/$pattern/").join(t" ")}"

        encoding.let(codec => t"$constrained [$codec]").or(constrained)

  // The declaration flags shown after a member's type in hover markup. `key` (§20) is not a
  // polarity — it marks the field as its record's identifier — but it reads naturally in the same
  // list, and it is the one flag a reader most needs to see.
  private def cardinality(required: Tels.Polarity, repeatable: Tels.Polarity, key: Boolean): Text =
    val flags: List[Text] =
      List
        ( t"optional"   -> (required == Tels.Polarity.Loose),
          t"repeatable" -> (repeatable == Tels.Polarity.Loose),
          t"key"        -> key )
      . filter(_(1)).map(_(0))

    if flags.nil then t"" else t" (${flags.join(t", ")})"

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
    struct.members.to[List].flatMap:
      case field: Tels.Field if field.keyword == keyword =>
        List(fieldMarkdown(field, schema))

      case reference: Tels.SelectRef =>
        variantsOf(reference.reference, schema).filter(_.keyword == keyword)
        . map(variantMarkdown(_, reference.reference, schema))

      case _ =>
        Nil
    . head

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
       encoding:    Optional[Text],
       patterns:    List[Text] = Nil )
  :   Text =

    val checks =
      if validators.nil then t""
      else t", validated as ${validators.map(validator => t"`$validator`").join(t", ")}"

    // Every `pattern` (§21.8) must match the whole value; several AND-conjoin.
    val matching =
      if patterns.nil then t""
      else t", matching ${patterns.map(pattern => t"`$pattern`").join(t" and ")}"

    // A declared `encoding` (§21.7) is a further constraint on the value — the codec's encoder
    // rejects what it cannot represent (↯/t.3.12) — as well as its BinTEL representation.
    val codec = encoding.let(name => t", encoded as `$name`").or(t"")
    val header = t"**$keyword** value — `${name.or(t"scalar")}`$checks$matching$codec"
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
        // A select-typed slot: the variant the atom names, or else the variants it could name.
        def selected: Optional[Text] = schema.selects.seek(_.name == name).let: select =>
          select.variants.seek(_.keyword == atom).lay
            ( t"**$keyword** value — one of "
              + select.variants.to[List].map(variant => t"`${variant.keyword}`").join(t", ") )
            ( variantMarkdown(_, name, schema) )

        def defined: Optional[Text] = schema.scalars.seek(_.name == name).let: scalar =>
          scalarValueMarkup
            ( keyword, name, scalar.validators.to[List], default, scalar.description,
              scalar.encoding, scalar.patterns.to[List] )

        def builtin: Optional[Text] = builtinScalars.seek(_(0) == name).let: (_, validator) =>
          scalarValueMarkup(keyword, name, List(validator), default, Unset, Unset)

        // A record-typed member: an inline atom fills one of the record's atom-assignable
        // members (§21) — a flag field or a flag-typed select variant — so describe that.
        def record: Optional[Text] =
          structOf(Tels.Reference(name), schema).let(fieldMarkup(_, atom, schema))

        selected.or(defined).or(builtin).or(record)

      case Tels.Scalar(validators, encoding, patterns) =>
        scalarValueMarkup
          ( keyword, Unset, validators.to[List], default, Unset, encoding, patterns.to[List] )

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
      case Tels.Reference(_) | Tels.Scalar(_, _, _) => t"$keyword "
      case _                                     => Unset

  private def keywordCompletions(struct: Tels.Struct, schema: Tels): List[Lsp.CompletionItem] =
    struct.members.to[List].flatMap:
      case field: Tels.Field =>
        List
          ( Lsp.CompletionItem
              ( label         = field.keyword,
                kind          = Lsp.CompletionItemKind.Field,
                detail        = typeLabel(field.fieldType, schema),
                documentation = field.description.let(text => Lsp.MarkupContent(value = text)),
                insertText    = keywordInsertText(field.keyword, field.fieldType) ) )

      case reference: Tels.SelectRef =>
        variantsOf(reference.reference, schema).map(variantCompletion(_, reference.reference))

      case _ =>
        Nil

  // The predefined TEL type names, always available in a schema document.
  private val builtinTypeNames = List(t"String", t"Identifier", t"TypeName", t"Sigil", t"Flag")

  // The schema a document is checked against: the built-in meta-schema for a schema document,
  // otherwise the registered schema its pragma resolves to.
  private def documentSchema(lines: Sequence[Text], resolver: PragmaResolver): Optional[Tels] =
    resolver(pragmaOf(lines)).tels

  private def isSchemaDocument(lines: Sequence[Text]): Boolean =
    pragmaOf(lines).identifier.lay(false)(id => namesTels(id.name))

  // The line's indentation, its keyword (the first token, if any), and how many whole atoms precede
  // the cursor: 0 = keyword position, 1 = first value/atom, 2 = second atom, and so on.
  private def completionContext(line: Text, character: Int): (Int, Optional[Text], Int) =
    val lineTokens = tokens(line)
    (leadingSpaces(line), lineTokens.head.let(_.text), lineTokens.count(character > _.end))

  // Type-name completions for a schema document: the document's own definitions plus the built-ins.
  private def typeNameCompletions(tree: List[Node]): List[Lsp.CompletionItem] =
    (definitions(tree).keys.to[List] + builtinTypeNames).distinct.order(identity).map: name =>
      Lsp.CompletionItem(label = name, kind = Lsp.CompletionItemKind.Class)

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
    def flag(fieldType: Tels.Type): Boolean = fieldType match
      case Tels.Flag => true
      case _         => false

    atomType match
      case Tels.Reference(name) if schema.selects.exists(_.name == name) =>
        variantsOf(name, schema).map(variantCompletion(_, name))

      case other =>
        structOf(other, schema).lay(Nil): struct =>
          struct.members.to[List].flatMap:
            case field: Tels.Field if flag(field.fieldType) =>
              List(Lsp.CompletionItem
                ( label         = field.keyword,
                  kind          = Lsp.CompletionItemKind.Field,
                  detail        = t"Flag",
                  documentation =
                    field.description.let(text => Lsp.MarkupContent(value = text)) ))

            case reference: Tels.SelectRef =>
              variantsOf(reference.reference, schema).filter(variant => flag(variant.variantType))
              . map(variantCompletion(_, reference.reference))

            case _ => Nil

  // Value completions: what may fill the inline-atom slot after `keyword`.
  private def atomValueCompletions
     ( lines:    Sequence[Text],
       tree:     List[Node],
       line:     Int,
       indent:   Int,
       keyword:  Text,
       resolver: PragmaResolver )
  :   Lsp.CompletionList =

    val items: Optional[List[Lsp.CompletionItem]] = documentSchema(lines, resolver).let: schema =>
      memberType(enclosingStruct(tree, line, indent, schema), keyword, schema)
      . let(atomCompletions(_, schema))

    Lsp.CompletionList(items = items.or(Nil))

  // Flag completions after a member declaration in a schema document: the Flag-typed members of
  // the meta-schema record that describes the declaration (`optional`, `required`, `repeatable`,
  // `irrepeatable`), derived from the meta-schema rather than hardcoded, minus any already given.
  private def flagCompletions
     ( lines:    Sequence[Text],
       tree:     List[Node],
       line:     Int,
       indent:   Int,
       keyword:  Text,
       content:  Text,
       resolver: PragmaResolver )
  :   List[Lsp.CompletionItem] =

    documentSchema(lines, resolver).lay(Nil): schema =>
      val struct = enclosingStruct(tree, line, indent, schema)
      val present = tokens(content).map(_.text)

      memberStruct(struct, keyword, schema).lay(Nil): member =>
        member.members.to[List].flatMap:
          case field: Tels.Field if !present.has(field.keyword) => field.fieldType match
            case Tels.Flag =>
              List(Lsp.CompletionItem
                ( label         = field.keyword,
                  kind          = Lsp.CompletionItemKind.Keyword,
                  detail        = t"Flag",
                  documentation = field.description.let(text => Lsp.MarkupContent(value = text)) ))

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
          detail     = if entry.layers == t"" then t"BASE-256 signature"
                       else t"BASE-256 signature; layers: ${entry.layers}",
          insertText = entry.id )

  // Layer-selection completions on the pragma line: `+name` for each layer the resolved schema
  // declares and the pragma does not yet select. Declaration order is preserved, since a
  // selection must be written in it (↯/t.1.24).
  private def layerCompletions(pragma: Pragma, resolver: PragmaResolver): List[Lsp.CompletionItem] =
    resolver(pragma) match
      case Resolution.Resolved(entry, _, _, _, _) =>
        val selected = pragma.layerNames

        resolver.layersOf(entry.name).filter(!selected.has(_)).map: (layer: Text) =>
          val selection: Text = t"+$layer"

          Lsp.CompletionItem
            ( label      = selection,
              kind       = Lsp.CompletionItemKind.Module,
              detail     = t"layer of `${entry.name}`",
              insertText = selection )

      case _ => Nil

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
    val line = lines.at(position.line.z).or(t"")
    val (indent, keyword, atomsBefore) = completionContext(line, position.character)

    // The pragma line's schema-identifier slot completes to the registered schemas; once the
    // schema is named, each further slot completes to a `+` layer selection (§8.1) — the layers
    // the resolved schema declares, minus those already selected, in declaration order.
    if position.line == pragma.line then
      if atomsBefore == 2 then Lsp.CompletionList(items = pragmaCompletions(resolver))
      else if atomsBefore >= 3 then Lsp.CompletionList(items = layerCompletions(pragma, resolver))
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
      case (kw: Text, 2) if isSchemaDocument(lines) && memberKeywords.has(kw) =>
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
    val pragma = pragmaOf(lines)

    if range.start.line == pragma.line
    then signatureAction(uri, lines, pragma, resolver).lay(Nil)(List(_))
    else documentSchema(lines, resolver).lay(Nil): schema =>
      expandAtomAction(uri, text, lines, tree, range.start.line, schema).lay(Nil)(List(_))
      + inlineChildAction(uri, text, lines, tree, range.start.line, schema).lay(Nil)(List(_))

  // Rubber-stamping (§8.1): a document whose pragma names its schema by reference alone is
  // portable nowhere, since a bare reference resolves only against local state; appending the
  // resolved composition's signature gives it a content-addressed identity wherever it goes. The
  // signature is that of the base composed with the pragma's selected layers, and goes after the
  // layer selections and before any sigil, in the pragma's positional order. Offered only when
  // the pragma carries no signature yet.
  private def signatureAction
     ( uri: Text, lines: Sequence[Text], pragma: Pragma, resolver: PragmaResolver )
  :   Optional[Lsp.CodeAction] =

    if !pragma.wellFormed || pragma.signature.present then Unset
    else
      val signature: Optional[Text] = resolver(pragma) match
        case Resolution.Resolved(_, _, _, _, signature) => signature
        case Resolution.Meta(_, _)                      => SchemaResolver.telsSignature
        case _                                          => Unset

      signature.let: signature =>
        val line = lines.at(pragma.line.z).or(t"")
        val lineTokens = tokens(line)

        // Insert before a trailing sigil phrase, else at the end of the line.
        val at = lineTokens.last.lay(line.length): token =>
          if lineTokens.size > 2 && isSigilToken(token.text) then token.start else line.length

        val insertion = if at == line.length then t" $signature" else t"$signature "
        val position = Lsp.Position(pragma.line, at)

        Lsp.CodeAction
          ( title = t"Append the schema signature to the pragma",
            kind  = t"refactor.rewrite",
            edit  = Lsp.WorkspaceEdit(changes = Map(uri -> List(
              Lsp.TextEdit(Lsp.Range(position, position), insertion)))) )

  // The inline-atom spans of a compound line after `keywordEnd`, per §10.3 phrase separation:
  // initially a single space terminates a phrase, but from the start of the first hard-space run
  // (two or more spaces) onward only hard runs terminate phrases, and single spaces become phrase
  // content. Returns (text, start, end) column spans. Remarks are NOT recognised here: the code
  // actions decline any line whose space-split tokens include a bare sigil before tokenising —
  // conservative, since a sigil can legitimately sit inside a hard-mode phrase, but never wrong.
  // If this tokenisation ever disagrees with Stratiform's, the generated candidate fails semantic
  // verification and the action is declined, so correctness does not rest on it.
  private def inlineAtomSpans(line: Text, keywordEnd: Int): List[Token] =
    // `hard` is whether a hard-space run has been seen before `from`.
    def recur(from: Int, hard: Boolean): List[Token] =
      val start = column(line, from)(_ != ' ')

      if start >= line.length then Nil else
        val hardened = hard || start - from >= 2

        // In hard mode the phrase runs to the next hard run, or to the end of the line less a
        // trailing single space (which would be ↯/t.1.8 anyway, and verification catches the
        // invalid document); otherwise it runs to the next space.
        val end =
          if !hardened then column(line, start)(_ == ' ')
          else
            line.skip(start).offsetOf(t"  ").lay(line.length - (if line.ends(t" ") then 1 else 0)):
              offset => start + offset.n0

        Token(line.skip(start).keep(end - start), start, end) :: recur(end, hardened)

    recur(keywordEnd, false)

  // The separator to place before `atom` when writing it after `keyword` (or appending it to a
  // line): a hard gap whenever the atom contains a space — so §10.3 keeps it one phrase — or when
  // the line is already in hard mode, where a single space would merge the atom into the phrase
  // before it.
  private def gapFor(atom: Text, hardMode: Boolean): Text =
    if hardMode || atom.contains(t" ") then t"  " else t" "

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
       lines:  Sequence[Text],
       tree:   List[Node],
       line:   Int,
       schema: Tels )
  :   Optional[Lsp.CodeAction] =

    flatten(tree).seek(_.line == line).let: node =>
      val lineText = lines.at(line.z).or(t"")
      val spans = inlineAtomSpans(lineText, node.keywordEnd)

      // The last phrase must run to the end of the line: with remarks declined, anything after it
      // would be trailing space (↯/t.1.8), and verification would reject the document anyway.
      spans.last.let: moved =>
        if declined(lines, node) || moved.end != lineText.length then Unset else
          val struct = enclosingStruct(tree, node.line, node.indent, schema)

          memberStruct(struct, node.keyword, schema).let: owner =>
            expansionOf(owner, spans.map(_.text), schema).let: childText =>
              // Where the line ends once the atom has gone: after the atom before it, if any.
              val previousEnd = spans.reverse match
                case _ :: previous :: _ => previous.end
                case _                  => node.keywordEnd

              val indent = t"${lineText.keep(node.indent)}  "

              val patched =
                lines.to[List].keep(line) + List(lineText.keep(previousEnd), t"$indent$childText")
                + lines.to[List].skip(line + 1)

              verifiedAction(text, patched.join(t"\n"), schema):
                Lsp.CodeAction
                  ( title = t"Move `${moved.text}` to a child compound line",
                    kind  = t"refactor.rewrite",
                    edit  = Lsp.WorkspaceEdit(changes = Map(uri -> List(
                      Lsp.TextEdit
                        ( Lsp.Range
                            ( Lsp.Position(line, previousEnd),
                              Lsp.Position(line, lineText.length) ),
                          t"\n$indent$childText" )))) )

  // Whether the code actions must leave the compound `node` alone: it carries a source- or
  // literal-atom payload (which must immediately follow its compound, §14/§15), or a bare sigil
  // among its tokens, which may begin a remark.
  private def declined(lines: Sequence[Text], node: Node): Boolean =
    val payload = lines.at((node.line + 1).z).lay(false): next =>
      leadingSpaces(next) < next.length && leadingSpaces(next) >= node.indent + 4

    val sigil = pragmaOf(lines).sigil.show
    payload || lines.at(node.line.z).lay(false)(tokens(_).exists(_.text == sigil))

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
       lines:  Sequence[Text],
       tree:   List[Node],
       line:   Int,
       schema: Tels )
  :   Optional[Lsp.CodeAction] =

    nodeChain(tree, line).reverse match
      case child :: parent :: _
          if child.line == line && child.line == parent.line + 1 && child.children.nil
             && parent.children.head.lay(false)(_.line == line)
             && !declined(lines, child) && !declined(lines, parent) =>

        val owner = enclosingStruct(tree, parent.line, parent.indent, schema)

        memberStruct(owner, parent.keyword, schema).let: parentStruct =>
          val childLine = lines.at(child.line.z).or(t"")
          val childSpans = inlineAtomSpans(childLine, child.keywordEnd)

          val atom: Optional[Text] =
            memberType(parentStruct, child.keyword, schema).let(resolvedType(_, schema)).let:
              case _: Tels.Scalar => childSpans.unique.let(_.text)
              case Tels.Flag      => if childSpans.nil then child.keyword else Unset
              case _              => Unset

          atom.let: atom =>
            val parentLine = lines.at(parent.line.z).or(t"")

            // A hard gap is needed when the parent line is already in hard mode — a single space
            // would merge the atom into the phrase before it — or when the atom itself contains a
            // space and must stay one phrase.
            val parentHard = parentLine.skip(parent.keywordEnd).contains(t"  ")
            val gap = gapFor(atom, parentHard)

            val patched =
              lines.to[List].keep(parent.line) + List(t"$parentLine$gap$atom")
              + lines.to[List].skip(child.line + 1)

            verifiedAction(text, patched.join(t"\n"), schema):
              Lsp.CodeAction
                ( title = t"Inline `$atom` onto the `${parent.keyword}` line",
                  kind  = t"refactor.inline",
                  edit  = Lsp.WorkspaceEdit(changes = Map(uri -> List
                    ( Lsp.TextEdit
                        ( Lsp.Range
                            ( Lsp.Position(parent.line, parentLine.length),
                              Lsp.Position(parent.line, parentLine.length) ),
                          t"$gap$atom" ),
                      Lsp.TextEdit
                        ( Lsp.Range
                            ( Lsp.Position(child.line, 0),
                              Lsp.Position(child.line + 1, 0) ),
                          t"" )))) )

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
            model = Tel.Type.assign(parsed(), schema, editorValidators, editorCodecs)

    if accrued.items.nil then model else Unset

  // Structural equality of two semantic models produced against the SAME schema: the flat keyword
  // index fully identifies the member (and, through the schema, its type), so index, tree shape and
  // scalar text suffice. Element order is compared too — order is part of the model — which makes
  // the check conservative: a refactoring that merely reorders equivalent elements is declined.
  private def sameModel(a: Tel.Element, b: Tel.Element): Boolean = (a, b) match
    case (Tel.Element.Value(i, _, s), Tel.Element.Value(j, _, t)) => i == j && s == t

    case (Tel.Element.Node(i, _, as), Tel.Element.Node(j, _, bs)) =>
      i == j && as.length == bs.length && as.to[List].zip(bs.to[List]).all(sameModel(_, _))

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
        def record: Optional[Tels.Type] = schema.records.seek(_.name == name).let: record =>
          Tels.Struct(record.members, record.validators)

        def scalar: Optional[Tels.Type] = schema.scalars.seek(_.name == name).let: definition =>
          Tels.Scalar(definition.validators, definition.encoding, definition.patterns)

        record.or(scalar)

      case other => other

  // Where the §20.2 atom phase has reached: the position of the next member to try, and what
  // the atom last assigned expands to.
  private case class Walk(position: Int, expansion: Optional[Text])

  private def expansionOf(struct: Tels.Struct, atoms: List[Text], schema: Tels): Optional[Text] =
    val members = struct.members

    def resolved(fieldType: Tels.Type): Optional[Tels.Type] = resolvedType(fieldType, schema)

    def required(member: Tels.Member): Boolean = member match
      case field: Tels.Field      => field.required != Tels.Polarity.Loose
      case select: Tels.SelectRef => select.required != Tels.Polarity.Loose
      case _: Tels.Exclude        => false

    def repeatable(member: Tels.Member): Boolean = member match
      case field: Tels.Field      => field.repeatable == Tels.Polarity.Loose
      case select: Tels.SelectRef => select.repeatable == Tels.Polarity.Loose
      case _: Tels.Exclude        => false

    def flag(fieldType: Tels.Type): Boolean = resolved(fieldType) match
      case Tels.Flag => true
      case _         => false

    def atomAssignable(member: Tels.Member): Boolean = member match
      case field: Tels.Field => resolved(field.fieldType) match
        case _: Tels.Scalar => true
        case Tels.Flag      => true
        case _              => false

      case select: Tels.SelectRef =>
        val variants = variantsOf(select.reference, schema)
        !variants.nil && variants.all(variant => flag(variant.variantType))

      case _: Tels.Exclude => false

    def flagShaped(member: Tels.Member): Boolean = member match
      case field: Tels.Field      => flag(field.fieldType)
      case select: Tels.SelectRef => atomAssignable(select)
      case _: Tels.Exclude        => false

    def matches(member: Tels.Member, atom: Text): Boolean = member match
      case field: Tels.Field      => field.keyword == atom
      case select: Tels.SelectRef => variantsOf(select.reference, schema).exists(_.keyword == atom)
      case _: Tels.Exclude        => false

    def skippable(member: Tels.Member, atom: Text): Boolean = member match
      case _: Tels.Exclude => true
      case _ =>
        !required(member)
        && (!atomAssignable(member) || (flagShaped(member) && !matches(member, atom)))

    // The first member at or after `position` which `atom` may not skip (step 3a).
    @tailrec
    def settle(position: Int, atom: Text): Int = members.at(position.z) match
      case member: Tels.Member if skippable(member, atom) => settle(position + 1, atom)
      case _                                              => position

    // Assigning `atom` to the member the walk settles on; a repeatable member's position is not
    // advanced (step 3d). `Unset` when the atom does not assign.
    def assign(walk: Walk, atom: Text): Optional[Walk] =
      val position = settle(walk.position, atom)

      members.at(position.z).let: member =>
        val next = if repeatable(member) then position else position + 1

        member match
          case field: Tels.Field => resolved(field.fieldType) match
            case _: Tels.Scalar => Walk(next, t"${field.keyword}${gapFor(atom, false)}$atom")
            case Tels.Flag      => if atom == field.keyword then Walk(next, atom) else Unset
            case _              => Unset

          case select: Tels.SelectRef => if matches(select, atom) then Walk(next, atom) else Unset
          case _: Tels.Exclude        => Unset

    atoms.fold(Walk(0, Unset): Optional[Walk]): (walk, atom) =>
      walk.let(assign(_, atom))
    . let(_.expansion)

  private def diagnostic
     ( entry:    (Optional[Tel.Focus], Tel.Error),
       lines:    Sequence[Text],
       document: Optional[Tel] )
  :   Lsp.Diagnostic =

    val (focus, error) = entry
    val range = errorRange(focus, error, lines, document)

    // Every TEL error is a hard error, with one editor-specific exception: ↯/t.3.13 (an encoding
    // name the codec binding does not resolve) is what an application must raise, but here it
    // means the codec is the application's — the editor binds only the two the specification
    // defines (BinTEL §8.4) — so the value is merely unchecked, and that is a warning.
    val unchecked = error.reason.number == 313

    Lsp.Diagnostic
      ( range    = range,
        severity = if unchecked then Lsp.DiagnosticSeverity.Warning
                   else Lsp.DiagnosticSeverity.Error,
        code     = bolt(error.reason.number),
        source   = t"tel",
        message  =
          if unchecked
          then t"${m"${error.reason}".text} — the encoding is application-defined, so the editor "
               + t"cannot check this value"
          else m"${error.reason}".text )

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
  //   3. Failing both — an error accrued outside `assign`, e.g. from `tel.as[Tels]` —
  //      the focus's keyword path is located against the position-tracked document here.
  //
  // An error with no span at all (nothing in the source to point at) falls back to the first line.
  private def errorRange
     ( focus:    Optional[Tel.Focus],
       error:    Tel.Error,
       lines:    Sequence[Text],
       document: Optional[Tel] )
  :   Lsp.Range =

    val span: Span =
      if error.span.exists then error.span
      else focus.lay(Span.empty): focus =>
        if focus.span.exists then focus.span
        else document.lay(Span.empty)(_.locate(focus.pointer).lay(Span.empty)(_.span))

    val fallback =
      val end = lines.head.lay(1)(_.length.max(1))
      Lsp.Range(Lsp.Position(0, 0), Lsp.Position(0, end))

    Lsp.Range.from(span).lay(fallback)(fit(_, lines))

  // Reconcile a span-derived range with the document as the client sees it. A span's columns are
  // the parser's, so a column just past the last character of a line (an error at end-of-input, or
  // a saturated span) has to be pulled back within the line, and a range left with no width — a
  // located compound whose length was not recorded — is widened to one character, so that the
  // client has something to underline. A range that spans lines is left alone.
  private def fit(range: Lsp.Range, lines: Sequence[Text]): Lsp.Range =
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
    else wordAt(position, lines).let: word =>
      val range =
        Lsp.Range(Lsp.Position(position.line, word.start), Lsp.Position(position.line, word.end))

      // The word's place on its line: as many tokens as begin before it.
      val index = lines.at(position.line.z).lay(0)(tokens(_).count(_.start < word.start))

      hoverMarkup(lines, tree, position, index, word.text, resolver)
      . let(value => Lsp.Hover(Lsp.MarkupContent(value = value), range))

  // Hover on the pragma line reports the schema-resolution status.
  private def pragmaHover(pragma: Pragma, lines: Sequence[Text], resolver: PragmaResolver)
  :   Lsp.Hover =

    val range = pragma.identifier.let: id =>
      Lsp.Range(Lsp.Position(pragma.line, id.start), Lsp.Position(pragma.line, id.end))

    val markup = resolver(pragma) match
      case Resolution.Resolved(entry, file, _, selected, signature) =>
        val composition =
          if selected.nil then t"the base schema"
          else t"the base with the layers ${selected.map(layer => t"`$layer`").join(t", ")}"

        val declared =
          if entry.layers == t"" then t""
          else t"; the schema declares the layers ${entry.layers}"

        t"**TEL document** — schema `${entry.name}`: $composition, signature `$signature`"
        + t"$declared. Registered at `${file.encode}`."

      case Resolution.Meta(_, _) =>
        t"**TEL schema document** — validated against the built-in `tels` meta-schema"

      case Resolution.Unresolved(identifier) =>
        t"**TEL document** — schema `$identifier` is not registered, so the document is parsed "
        + t"but not validated. Register it with `tel schema add <file>`."

      case Resolution.BadLayers(identifier, detail, _) =>
        t"**TEL document** — schema `$identifier` resolves, but $detail."

      case Resolution.NoSchema =>
        t"**TEL document** — pragma `${lines.at(pragma.line.z).or(t"")}`"

    Lsp.Hover(Lsp.MarkupContent(value = markup), range)

  // Hover markup for the token under the cursor. The keyword slot (token 0) shows the schema
  // member's declaration; a value slot shows what the schema says about the atom; named-type
  // references and built-in validator names show their own descriptions.
  private def hoverMarkup
     ( lines:    Sequence[Text],
       tree:     List[Node],
       position: Lsp.Position,
       index:    Int,
       word:     Text,
       resolver: PragmaResolver )
  :   Optional[Text] =

    def definition: Optional[Text] = definitions(tree).at(word).let(describe)

    if index == 0 then keywordMarkup(lines, tree, position, resolver).or(definition)
    else
      definition.or(builtinValidatorMarkup(lines, position, word))
      . or(valueSlotMarkup(lines, tree, position, word, resolver))

  private def keywordMarkup
     ( lines: Sequence[Text], tree: List[Node], position: Lsp.Position, resolver: PragmaResolver )
  :   Optional[Text] =

    documentSchema(lines, resolver).let: schema =>
      enclosing(tree, position, schema).let: slot =>
        fieldMarkup(slot.struct, slot.node.keyword, schema)

  private def valueSlotMarkup
     ( lines: Sequence[Text], tree: List[Node], position: Lsp.Position, atom: Text,
       resolver: PragmaResolver )
  :   Optional[Text] =

    documentSchema(lines, resolver).let: schema =>
      enclosing(tree, position, schema).let: slot =>
        val keyword = slot.node.keyword

        val default: Optional[Text] = slot.struct.members.to[List].reap:
          case field: Tels.Field if field.keyword == keyword => field.default

        memberType(slot.struct, keyword, schema).let(valueMarkup(keyword, _, default, atom, schema))

  // A validator name on a `validate` line gets the §21.5 blurb for the built-in validators.
  private def builtinValidatorMarkup(lines: Sequence[Text], position: Lsp.Position, word: Text)
  :   Optional[Text] =

    val onValidate = lines.at(position.line.z).lay(false): line =>
      tokens(line).head.lay(false)(_.text == t"validate")

    if !onValidate then Unset else builtinValidators.seek(_(0) == word).let: (name, blurb) =>
      t"**$name** — built-in validator: $blurb"

  private def describe(node: Node): Text =
    val members = node.children.map(_.keyword).distinct
    val head = t"**${node.keyword} ${node.atoms.head.or(t"")}**"
    if members.nil then head else t"$head — ${members.join(t", ")}"

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

  // A whitespace-delimited token of a line, with its column span.
  private[tel] case class Token(text: Text, start: Int, end: Int)

  // The document's lines, as an editor counts them: ended by a line feed, a carriage return or
  // the two together, and without the empty line which a final terminator would leave.
  private[tel] def linesOf(text: Text): Sequence[Text] =
    val lines = text.sub(t"\r\n", t"\n").sub(t"\r", t"\n").cut(t"\n")
    (if lines.last == t"" then lines.lead else lines).to[Sequence]

  // Each line paired with its zero-based line number.
  private def numbered(lines: Sequence[Text]): List[(Text, Int)] =
    lines.to[List].indexed.map((line, ordinal) => (line, ordinal.n0))

  private def lineLength(lines: Sequence[Text], line: Int): Int = lines.at(line.z).lay(0)(_.length)

  // The first column at or after `from` whose character satisfies `predicate`, or the line's
  // length if there is none.
  private def column(line: Text, from: Int)(predicate: Char => Boolean): Int =
    line.pinpoint(predicate, from.z).lay(line.length)(_.n0)

  private def leadingSpaces(line: Text): Int = column(line, 0)(_ != ' ')

  // A compound line, keeping its source line index, indentation, keyword and inline atoms.
  // `payloadEnd` is the last line of the compound's source- or literal-atom payload (§14, §15), or
  // the compound's own line if it has none.
  private case class Raw
      ( index: Int, indent: Int, keyword: Text, keywordEnd: Int, detail: Text, payloadEnd: Int )

  // The full document as `lines` plus a nested tree of compound `Node`s.
  private[tel] def structure(text: Text): (Sequence[Text], List[Node]) =
    val lines = linesOf(text)
    val pragma = pragmaOf(lines)
    val count = lines.size

    def blank(line: Text): Boolean = leadingSpaces(line) >= line.length

    // Source atom (§14): captured lines run to the first non-blank line with fewer leading
    // spaces than the first payload line; interior blank lines are permitted. Yields the last
    // non-blank line captured, and the line the scan resumes at.
    @tailrec
    def sourcePayload(index: Int, indent: Int, last: Int): (Int, Int) = lines.at(index.z) match
      case line: Text if blank(line)                   => sourcePayload(index + 1, indent, last)
      case line: Text if leadingSpaces(line) >= indent => sourcePayload(index + 1, indent, index)
      case _                                           => (last, index)

    // Literal atom (§15): the payload runs verbatim from the delimiter line to the first line
    // whose content matches it exactly (inclusive), or to end of input. Yields its last line.
    @tailrec
    def literalPayload(index: Int, delimiter: Text): Int = lines.at(index.z) match
      case line: Text if line != delimiter => literalPayload(index + 1, delimiter)
      case _: Text                         => index
      case _                               => count - 1

    // `raws` holds the compounds found so far, latest first. `adjacent` is whether the latest is
    // on the immediately preceding line: only such a compound can introduce a source-atom
    // payload (+2 indent levels = +4 spaces, §14) or a literal-atom payload (+3 levels = +6
    // spaces, §15) on the current line. The payload trigger is checked before the comment rule,
    // because the sigil has no special meaning on the first payload line.
    @tailrec
    def scan(index: Int, raws: List[Raw], adjacent: Boolean): List[Raw] = lines.at(index.z) match
      case line: Text =>
        val indent = leadingSpaces(line)

        // A blank line (or the pragma) is no compound, and ends payload adjacency.
        if blank(line) || index == pragma.line then scan(index + 1, raws, false)
        else raws match
          case previous :: earlier if adjacent && indent == previous.indent + 4 =>
            val (last, next) = sourcePayload(index, indent, index)
            scan(next, previous.copy(payloadEnd = last) :: earlier, false)

          case previous :: earlier if adjacent && indent == previous.indent + 6 =>
            val last = literalPayload(index + 1, line)
            scan(last + 1, previous.copy(payloadEnd = last) :: earlier, false)

          case _ =>
            // A comment/separator line is no compound; it also ends payload adjacency.
            if line.at(indent.z).lay(false)(_ == pragma.sigil) then scan(index + 1, raws, false)
            else
              val end = column(line, indent)(_ == ' ')
              val detail = line.skip(column(line, end)(_ != ' '))
              val keyword = line.skip(indent).keep(end - indent)
              scan(index + 1, Raw(index, indent, keyword, end, detail, index) :: raws, true)

      case _ =>
        raws.reverse

    def build(items: List[Raw]): List[Node] = items match
      case head :: tail =>
        val (descendants, rest) = tail.span(_.indent > head.indent)
        val endLine = descendants.last.lay(head.payloadEnd)(_.payloadEnd)
        val atoms = head.detail.cut(t" ").filter(_ != t"")

        Node(head.index, head.indent, head.keyword, head.keywordEnd, head.detail, atoms, endLine,
            build(descendants))
        :: build(rest)

      case _ =>
        Nil

    (lines, build(scan(0, Nil, false)))

  private[tel] def flatten(nodes: List[Node]): List[Node] =
    nodes.flatMap(node => node :: flatten(node.children))

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
    Map
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
    structAt(schema, path).lay(Lsp.SymbolKind.Field): struct =>
      val isVariant = struct.members.to[List].exists:
        case reference: Tels.SelectRef =>
          variantsOf(reference.reference, schema).exists(_.keyword == keyword)

        case _ => false

      if isVariant then Lsp.SymbolKind.EnumMember
      else memberType(struct, keyword, schema) match
        case Tels.Flag            => Lsp.SymbolKind.Boolean
        case Tels.Scalar(_, _, _) => Lsp.SymbolKind.String
        case Tels.Struct(_, _)    => Lsp.SymbolKind.Object
        case Tels.Reference(name) =>
          if schema.records.exists(_.name == name) then Lsp.SymbolKind.Object
          else if schema.selects.exists(_.name == name) then Lsp.SymbolKind.Enum
          else Lsp.SymbolKind.String

        case _ => Lsp.SymbolKind.Field

  private def symbol
     ( node:      Node,
       lines:     Sequence[Text],
       schema:    Optional[Tels],
       schemaDoc: Boolean,
       path:      List[Text] )
  :   Lsp.DocumentSymbol =

    val kind =
      if schemaDoc then schemaDocumentKinds.at(node.keyword).or(Lsp.SymbolKind.Field)
      else schema.lay(Lsp.SymbolKind.Field)(schemaKind(_, path, node.keyword))

    Lsp.DocumentSymbol
      ( name           = node.keyword,
        detail         = node.detail.puncture(t""),
        kind           = kind,
        range          = Lsp.Range
                           ( Lsp.Position(node.line, node.indent),
                             Lsp.Position(node.endLine, lineLength(lines, node.endLine)) ),
        selectionRange = Lsp.Range
                           ( Lsp.Position(node.line, node.indent),
                             Lsp.Position(node.line, node.keywordEnd) ),
        children       =
          if node.children.nil then Unset
          else node.children.map(symbol(_, lines, schema, schemaDoc, path :+ node.keyword)) )

  private[tel] def folds(node: Node): List[Lsp.FoldingRange] =
    val self =
      if node.endLine > node.line
      then List(Lsp.FoldingRange(startLine = node.line, endLine = node.endLine, kind = t"region"))
      else Nil

    self + node.children.flatMap(folds)

  private def selectionRange(position: Lsp.Position, tree: List[Node], lines: Sequence[Text])
  :   Lsp.SelectionRange =

    val ranges = nodeChain(tree, position.line).map: node =>
      Lsp.Range
        ( Lsp.Position(node.line, node.indent),
          Lsp.Position(node.endLine, lineLength(lines, node.endLine)) )

    ranges.fold(Unset: Optional[Lsp.SelectionRange]): (parent, range) =>
      Lsp.SelectionRange(range, parent)
    . or(Lsp.SelectionRange(Lsp.Range(position, position)))

  private def highlights(text: Text, position: Lsp.Position): List[Lsp.DocumentHighlight] =
    val nodes = flatten(structure(text)(1))

    nodes.seek(_.line == position.line).lay(Nil): target =>
      nodes.filter(_.keyword == target.keyword).map: node =>
        Lsp.DocumentHighlight
          ( Lsp.Range
              ( Lsp.Position(node.line, node.indent), Lsp.Position(node.line, node.keywordEnd) ),
            Lsp.DocumentHighlightKind.Text )

  // ── Navigation ────────────────────────────────────────────────────────────────────────────────
  //
  // Type-name navigation for schema documents: a `record`/`scalar`/`select` compound *defines* a
  // named type; a `field`/`variant`/`select`/`record` compound *references* one by its inline atom.
  // These are resolved textually against the source scan, so they work for any document that carries
  // such definitions (i.e. schemas).

  private val definitionKeywords = Set(t"record", t"scalar", t"select")

  // The whitespace-delimited tokens of a line, each with its start/end column.
  private[tel] def tokens(line: Text): List[Token] =
    def recur(from: Int): List[Token] =
      val start = column(line, from)(_ != ' ')

      if start >= line.length then Nil else
        val end = column(line, start)(_ == ' ')
        Token(line.skip(start).keep(end - start), start, end) :: recur(end)

    recur(0)

  // The token under a cursor position.
  private def wordAt(position: Lsp.Position, lines: Sequence[Text]): Optional[Token] =
    lines.at(position.line.z).let: line =>
      tokens(line).seek(token => position.character >= token.start && position.character <= token.end)

  // The named types which `nodes` define, keyed by name: the first atom of each
  // `record`/`scalar`/`select` compound among them.
  private def defined(nodes: List[Node]): Map[Text, Node] =
    nodes.filter(node => definitionKeywords.has(node.keyword)).flatMap: node =>
      node.atoms.head.lay(Nil)(name => List(name -> node))
    . to[Map]

  // Named-type definitions anywhere in the document, keyed by name.
  private def definitions(nodes: List[Node]): Map[Text, Node] = defined(flatten(nodes))

  // Every whitespace-delimited occurrence of `word` as a whole token, with line and column span.
  private def occurrences(word: Text, lines: Sequence[Text]): List[(Int, Int, Int)] =
    numbered(lines).flatMap: (line, index) =>
      tokens(line).filter(_.text == word).map(token => (index, token.start, token.end))

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
  private def topLevelDefinitions(nodes: List[Node]): Map[Text, Node] = defined(nodes)

  // Whether the compound's first atom is `name`.
  private def named(node: Node, name: Text): Boolean = node.atoms.head.lay(false)(_ == name)

  // The name atom of the compound on `line`: its second token.
  private def nameToken(lines: Sequence[Text], line: Int): Optional[Token] =
    lines.at(line.z).let(tokens(_).skip(1).head)

  private def fieldNode(nodes: List[Node], name: Text): Optional[Node] =
    nodes.seek(node => node.keyword == t"field" && named(node, name))

  // Descend a schema document from its `document` block along the ancestor keywords of a compound
  // (each a field whose type names a record), yielding the child nodes of the enclosing struct.
  private def descend(schemaTree: List[Node], context: List[Node], ancestors: List[Text])
  :   Optional[List[Node]] =
    ancestors match
      case keyword :: rest =>
        fieldNode(context, keyword).let(_.atoms.skip(1).head).let: typeName =>
          topLevelDefinitions(schemaTree).at(typeName).let: definition =>
            descend(schemaTree, definition.children, rest)

      case _ =>
        context

  // The schema-document node that declares `keyword`: a `field`/`variant` in `context` named
  // `keyword`, or a `variant` of a `select` referenced from `context`.
  private def locateMember(schemaTree: List[Node], context: List[Node], keyword: Text)
  :   Optional[Node] =
    context.seek(node => memberKeywords.has(node.keyword) && named(node, keyword)).or:
      context.filter(_.keyword == t"select").flatMap: reference =>
        reference.atoms.head.let(topLevelDefinitions(schemaTree).at(_)).lay(Nil)(_.children)
        . filter(node => node.keyword == t"variant" && named(node, keyword))
      . head

  private def schemaDefinition
     ( lines:    Sequence[Text],
       tree:     List[Node],
       position: Lsp.Position,
       resolver: PragmaResolver )
  :   List[Lsp.Location] =

    val pragma = pragmaOf(lines)

    resolver(pragma).schemaFile.let(file => SchemaCache.readText(file).let(file -> _)).lay(Nil):
      (file, text) =>
        val (schemaLines, schemaTree) = structure(text)
        val uri = t"file://${file.encode}"

        if position.line == pragma.line then List(location(uri, 0, 0, 0))
        else nodeChain(tree, position.line).reverse match
          case node :: ancestors if node.line == position.line =>
            val documentBlock = schemaTree.seek(_.keyword == t"document").lay(Nil)(_.children)

            descend(schemaTree, documentBlock, ancestors.reverse.map(_.keyword))
            . let(locateMember(schemaTree, _, node.keyword)).lay(Nil): target =>
                nameToken(schemaLines, target.line)
                . lay(List(location(uri, target.line, target.indent, target.keywordEnd))): name =>
                    List(location(uri, target.line, name.start, name.end))

          case _ => Nil

  private[tel] def definitionAt
     ( uri:      Text,
       text:     Text,
       position: Lsp.Position,
       resolver: PragmaResolver )
  :   List[Lsp.Location] =

    val (lines, tree) = structure(text)

    // A local named-type reference (schema documents) jumps within the file; anything else
    // tries to jump across into the registered schema file.
    wordAt(position, lines).let(word => definitions(tree).at(word.text))
    . lay(schemaDefinition(lines, tree, position, resolver)): node =>
        nameToken(lines, node.line).lay(Nil): name =>
          List(location(uri, node.line, name.start, name.end))

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
  private def problemCounts(diagnostics: List[Lsp.Diagnostic]): Text =
    List
      ( Lsp.DiagnosticSeverity.Error, Lsp.DiagnosticSeverity.Warning,
        Lsp.DiagnosticSeverity.Information, Lsp.DiagnosticSeverity.Hint )
    . map(severity => severity -> diagnostics.count(severityOf(_) == severity))
    . filter(_(1) > 0)
    . map: (severity, count) =>
        t"$count ${severityWord(severity)}${if count == 1 then t"" else t"s"}"
    . join(t", ")

  // One line per problem, positions 1-based, no source text. Columns are inclusive on both ends,
  // so `columns 5-7` names three characters; a zero-width range names the single column at which
  // the problem begins.
  private[tel] def llmReport(path: Text, diagnostics: List[Lsp.Diagnostic]): List[Text] =
    if diagnostics.nil then List(t"$path: no problems")
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
  private[tel] def humanReport(path: Text, lines: Sequence[Text], diagnostics: List[Lsp.Diagnostic])
  :   List[Teletype] =

    if diagnostics.nil then List(e"${WebColors.MediumSeaGreen}(✓) $path: no problems found")
    else
      // The gutter is as wide as the highest line number quoted.
      val width = diagnostics.fold(0)((most, next) => most.max(next.range.start.line + 1)).show.length

      diagnostics.flatMap: diagnostic =>
        val severity = severityOf(diagnostic)
        val start = diagnostic.range.start
        val code = diagnostic.code.let(code => e" $Bold($code)").or(e"")
        val location = t"$path:${start.line + 1}:${start.character + 1}"

        val header =
          e"${tinted(severity, severityWord(severity))} $Bold($location)$code: ${diagnostic.message}"

        lines.at(start.line.z).lay(List(header)): line =>
          // The span to highlight: to the range's end on a single-line range, or to the end of
          // the line when the range continues past it. A zero-width range still gets one caret.
          val from = start.character.min(line.length)
          val to =
            if diagnostic.range.end.line == start.line
            then diagnostic.range.end.character.min(line.length).max(from)
            else line.length

          val gutter = (start.line + 1).show.pad(width, Rtl)
          val carets = t"".pad((to - from).max(1), char = '^')
          val quoted = tinted(severity, line.skip(from).keep(to - from))

          List
            ( header,
              e"  $gutter │ ${line.keep(from)}$quoted${line.skip(to)}",
              e"  ${t"".pad(width)} │ ${t"".pad(from)}${tinted(severity, carets)}" )
      + List(e"", e"$path: ${problemCounts(diagnostics)}")

  private def validateFile(file: Path on Local, llm: Boolean)
      (using Stdio, Environment, System)
  :   DocumentInvalid.type | Unreadable.type | Exit.Ok.type =
    recover:
      case error: Path.Error =>
        Out.println(t"tel: [↯/t.6.4] could not resolve ${file.encode}: ${error.message.text}")
        Unreadable

    . protect:
        SchemaCache.readText(file.encode.as[Path on Linux]) match
          case text: Text =>
            val registry: Registry = safely(SchemaCache.directory)
            val resolver = PragmaResolver(registry)
            val diagnostics = diagnose(text, resolver).order: diagnostic =>
              (diagnostic.range.start.line, diagnostic.range.start.character)

            if llm then llmReport(file.encode, diagnostics).each(Out.println(_))
            else humanReport(file.encode, linesOf(text), diagnostics).each(Out.println(_))

            val errors = diagnostics.count(severityOf(_) == Lsp.DiagnosticSeverity.Error)
            if errors > 0 then DocumentInvalid else Exit.Ok

          case _ =>
            Out.println(t"tel: [↯/t.6.5] could not read ${file.encode}")
            Unreadable

  // ── Live message log ────────────────────────────────────────────────────────────────────────
  //
  // The tool runs as an Ethereal daemon: one JVM hosts every `tel` invocation, and `TelServer` is a
  // singleton loaded once in it. So `logSubscribers` is shared daemon-wide, and a `tel lsp --log`
  // session can observe — live — the messages that the editor's `tel lsp` session sends and receives.
  // Each `--log` subscriber gets its own `Spool`; the `Lsp.Observer` registered with `Lsp.listen`
  // broadcasts every message, tagged `recv` (client → server) or `send` (server → client), to all
  // subscribers.

  private val logSubscribers = Atomic(List[Relay[Text]]())

  private def broadcastLog(marker: Text, message: Text): Unit =
    logSubscribers().each(_.put(t"$marker $message"))

  private object TrafficLog extends Lsp.Observer:
    def received(message: Text): Unit = broadcastLog(t"recv", message)
    def sent(message: Text): Unit = broadcastLog(t"send", message)

  // Streams messages to stdout until interrupted (Ctrl-C); used by `tel lsp --log`.
  private def streamLog()(using Stdio): Unit =
    val spool = Relay[Text]()
    logSubscribers.ere(spool :: _)
    try
      Out.println(t"Streaming messages sent/received by the tel language server. Press Ctrl-C to stop.")
      spool.chain.each: message =>
        Out.println(message)
    // Deliberately a `catch`, not a `recover`: `InterruptedException` is the JVM's thread-interrupt
    // signal (Ctrl-C, here), not a `Tactic` obligation, so there is nothing for a statically-checked
    // handler to discharge.
    catch case _: InterruptedException => ()
    finally logSubscribers.ere(_.filter(!_.eq(spool)))

  // ── Command-line interface ──────────────────────────────────────────────────────────────────
  //
  // Ethereal serves tab-completions by re-running this dispatch in *completion mode*, where the
  // `execute` blocks are never run. Everything the shell can offer must therefore be registered
  // *outside* `execute`: a `Flag` read inside one registers nothing and the shell offers nothing.
  // That is why each case below reads its flags and computes its suggestions before handing the
  // work off. Matching a `Subcommand` is itself what suggests it, and `--help` is answered from
  // the help tree Ethereal derives from these same registrations — so the usage text below is
  // generated, never written by hand.

  private val ServerGroup = CommandGroup(t"Language server")
  private val SchemaGroup = CommandGroup(t"Schema registry")

  private val LspCommand =
    Subcommand
      ( t"lsp", t"run the TEL language server over stdio (for editors)", group = ServerGroup )

  private val ValidateCommand =
    Subcommand
      ( t"validate", t"parse and validate a TEL file, reporting its errors", group = ServerGroup )

  private val InstallCommand =
    Subcommand(t"install", t"install tab-completions into the shell", group = ServerGroup)

  // `upgrade` is always accepted, but suggested only while a newer release is known: the hidden
  // twin is what the dispatch matches against otherwise (see `SelfUpgrade`).
  private val UpgradeCommand =
    Subcommand
     (t"upgrade", t"replace this executable with the newest release", group = ServerGroup)

  private val HiddenUpgrade =
    Subcommand
     (t"upgrade", t"replace this executable with the newest release", group = ServerGroup,
      hidden = true)

  private val SchemaCommand =
    Subcommand(t"schema", t"manage the schema registry", group = SchemaGroup)

  private val AddCommand      = Subcommand(t"add", t"add a schema file to the registry")
  private val ListCommand     = Subcommand(t"list", t"list registered schemas")

  private val SignatureCommand =
    Subcommand(t"signature", t"show a schema's palimpsest signature")

  // Flags carry their own descriptions and short forms, so they appear in both tab-completion and
  // the generated help. `Flag[Unit]` is a presence-only switch: it takes no operand, so help does
  // not show it with a `<value>` placeholder.
  private val LogFlag =
    Flag[Unit]
      ( t"log", false, List('l'), t"stream the messages a running server sends and receives" )

  private val LlmFlag =
    Flag[Unit]
      ( t"llm", false, Nil,
        t"report problems by position only, without quoting source lines (for an LLM)" )

  private val HelpFlag =
    Flag[Unit](t"help", false, List('h'), t"describe the available subcommands and options")

  // The exit statuses, declared alongside the flags and subcommands they accompany. Returning one
  // from an `execute` block both sets the process's exit code and documents it: a block RETURNS
  // the status object itself (Soundness #1853 retired the contravariant `Status.Registry`), and
  // `execute`'s `Precise` bound keeps the block's result type as the union of the singleton types
  // of every status reachable from it — including those returned by the handlers it calls, which
  // declare that union as their own result type. `Status.Admissible` reifies it, so the generated
  // help lists the statuses without their being written down a second time.
  //
  // These must be `object`s rather than `val`s: capture checking currently rejects
  // `value.type <: value.type | other.type` for a `val`'s singleton type (soundness#1811), which
  // would stop the union forming.
  object UsageError extends Status(1, t"the command was not invoked correctly")
  object DocumentInvalid extends Status(2, t"the document has validation errors")
  object Unreadable extends Status(3, t"a file or the schema registry could not be read")
  object SchemaRejected extends Status(4, t"the schema was not accepted into the registry")
  object SchemaMissing extends Status(5, t"no schema of that name is registered")
  object ServerFailed extends Status(6, t"the language server terminated abnormally")

  // A registered schema completes to its name, described by its layers (or its signature when it
  // declares none), so `tel schema signature <TAB>` offers what the registry actually holds.
  private given SchemaCache.Entry is Suggestible = entry =>
    Suggestion
      ( entry.name,
        if entry.layers == t"" then t"schema ${entry.id}" else t"layers: ${entry.layers}" )

  private given Text is Suggestible = Suggestion(_)

  // The schema registry, read through the *client's* environment rather than the daemon's: the
  // registry lives under `$XDG_CACHE_HOME`, and in a daemon-backed CLI those differ. This runs in
  // completion mode too, which is the point — the suggestions are the registry's real contents.
  private def registryDirectory(using cli: Cli): Optional[Path on Linux] =
    given Environment = cli.environment
    safely(SchemaCache.directory)

  private def registryEntries(using Cli): List[SchemaCache.Entry] =
    registryDirectory.lay(Nil)(directory => safely(SchemaCache.entries(directory)).or(Nil))

  // `arguments` carries flags alongside operands, so the positional operands are those that do not
  // look like flags. (A leading `--` terminates flag parsing upstream, per POSIX.)
  private def operands(arguments: List[Argument]): List[Argument] =
    arguments.filter(!_().starts(t"-"))

  // The command dispatch. The `@main` entry point and burdock's `externalize` wrapper live alone
  // in the `launcher` module (`src/launcher/tel_launcher.scala`), which depends on THIS module as a
  // PUBLISHED coordinate — so `externalize` records the released jar's hash and the repackager turns
  // it into an on-demand `Burdock-Require` download instead of inlining it.
  def run(): Unit = cli:

    // What the launcher did with an upgrade staged earlier, and the daily check for a newer
    // release: for a real invocation only, never for a tab-completion.
    summon[Cli] match
      case _: Invocation =>
        SelfUpgrade.report()
        SelfUpgrade.check()

      case _ =>
        ()

    val upgrade = if SelfUpgrade.available.present then UpgradeCommand else HiddenUpgrade

    // Read at the top level, so `--help` is offered (and honoured) for every subcommand.
    val help = HelpFlag().present

    if help then execute:
      Out.println(resident.help())
      Exit.Ok

    else arguments match
      case LspCommand() :: _ if LogFlag().present =>
        execute:
          streamLog()
          Exit.Ok

      case LspCommand() :: _ =>
        execute:
          // Resolved here, where the invoker's `Environment` is in scope, and closed over by the
          // handlers: `listen` keeps everything stateful within its own frame.
          val registry: Registry = safely(SchemaCache.directory)
          val resolver = PragmaResolver(registry)

          // The server loop runs under `supervise`, whose `Async.Error` — with the transport's
          // `Framing.Error` (a malformed JSON-RPC frame) and `Truncation.Error` (a standard stream
          // cut short), which `Lsp.listen` reports rather than throws — are the obligations this
          // file does not discharge locally. They are handled here rather than by a blanket
          // `throwUnsafely` so that every other raising call in this file stays statically checked.
          // The report goes to `Err`: stdout is the LSP transport, and writing to it would corrupt
          // the message stream.
          recover:
            case error: Async.Error =>
              Err.println(t"tel: [↯/t.6.6] the language server terminated abnormally: ${error.message.text}")
              ServerFailed

            case error: Framing.Error =>
              Err.println(t"tel: [↯/t.6.7] the language server's transport failed: ${error.message.text}")
              ServerFailed

            case error: Truncation.Error =>
              Err.println(t"tel: [↯/t.6.7] the language server's transport failed: ${error.message.text}")
              ServerFailed

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
                  val lines = linesOf(document.text)

                  wordAt(position, lines).lay(Nil): word =>
                    occurrences(word.text, lines).map: (line, start, end) =>
                      location(document.uri, line, start, end)

                documentSymbols(outline(document.text, resolver))

                foldingRanges(structure(document.text)(1).flatMap(folds))

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

      case SchemaCommand() :: SignatureCommand() :: rest =>
        // `select` registers the suggestions *and* reads the operand, so the schema name completes
        // to the registry's contents and each layer operand completes to the layers that schema
        // actually declares — minus the ones already given, which cannot be repeated.
        val entries = registryEntries

        operands(rest) match
          case name :: layers =>
            val entry = name.select(entries)

            val declared = entry.lay(Nil): entry =>
              registryDirectory.lay(Nil)(SchemaCache.layerNames(_, entry.name))

            // The menu title applies only once the cursor has moved past the schema name, where
            // the operands being completed really are that schema's layers.
            if !layers.nil && !declared.nil
            then explain(t"layers of schema `${name()}`, in declaration order")

            // Register a completion for each layer operand, excluding those already given: a layer
            // may be selected at most once. The fold's accumulator exists to narrow each
            // subsequent operand's suggestions, not to supply the values — a half-typed operand
            // selects nothing but must still reach `schemaSignature` verbatim.
            layers.fold(Nil: List[Text]): (taken, argument) =>
              argument.select(declared.filter(!taken.has(_))).lay(taken)(taken :+ _)

            execute(schemaSignature(name(), layers.map(_())))

          case _ =>
            // No schema named yet: the cursor is on the name operand, so suggest the registry.
            rest.head.let(_.select(entries))
            execute(usage(t"tel schema signature <name> [layer…]"))

      case ValidateCommand() :: rest =>
        // `--llm` selects the quotation-free, position-only report an LLM can act on directly.
        val llm = LlmFlag().present

        operands(rest) match
          case Pathname(file) :: _ => execute(validateFile(file, llm))
          case _                   => execute(usage(t"tel validate <file> [--llm]"))

      case InstallCommand() :: _ =>
        execute(installCompletions())

      case upgrade() :: _ =>
        execute(SelfUpgrade.upgrade())

      // A bare `tel` is not an error: it prints the generated help and succeeds, so that a user
      // who just runs the command discovers what it offers.
      case Nil =>
        execute:
          Out.println(resident.help())
          Exit.Ok

      case _ =>
        execute:
          Err.println(t"tel: [↯/t.6.8] unrecognised command")
          Out.println(resident.help())
          UsageError

  // A malformed invocation reports the one command's synopsis on stderr, leaving stdout clean for
  // callers that pipe it; the full generated help stays one `--help` away.
  private def usage(synopsis: Text)(using Stdio): UsageError.type =
    Err.println(t"tel: usage: $synopsis")
    UsageError

  // Ethereal installs shell completions from the launcher stub it already knows about, so this
  // needs no knowledge of the shell: `ensure` locates each installed shell's completion directory
  // and writes (or refreshes) the entry, reporting the paths it wrote.
  private def installCompletions()(using Stdio, Resident, Diagnostics, Cli): Exit =
    import workingDirectories.javaBaseWorkingDirectory
    import environments.daemonClientEnvironment
    import logging.silentLogging
    given Entrypoint = scala.caps.unsafe.unsafeAssumePure(summon[Resident])

    Out.println(t"Installed tab-completions at:")
    Completions.ensure(force = true).each(Out.println(_))
    Exit.Ok

  // The `tel schema …` subcommands each end at a `recover` boundary rather than a `catch`: the
  // handler names the error types the body can actually raise, so the compiler checks that every
  // obligation is discharged and a new raising call cannot be added without being handled here. A
  // bare `catch case error: Error` would swallow whatever arrived, including errors these bodies
  // were never meant to absorb.
  private def schemaList()(using Stdio, Environment, System): Unreadable.type | Exit =
    recover:
      case error: Path.Error =>
        Out.println(t"tel: [↯/t.6.9] could not list schemas: ${error.message.text}")
        Unreadable

    . protect:
        val entries = SchemaCache.entries(SchemaCache.directory)
        if entries.nil then Out.println(t"No schemas registered. Add one with `tel schema add`.")
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
  private def schemaAdd(file: Path on Local)
      (using Stdio, Environment, System)
  :   SchemaRejected.type | Exit =
    def failed(error: Error): SchemaRejected.type =
      Out.println(t"tel: [↯/t.6.10] could not add schema: ${error.message.text}")
      SchemaRejected

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

  private def schemaSignature(name: Text, layers: List[Text])
      (using Stdio, Environment, System)
  :   Unreadable.type | SchemaMissing.type | Exit =
    def failed(error: Error): Unreadable.type =
      Out.println(t"tel: [↯/t.6.11] could not compute signature: ${error.message.text}")
      Unreadable

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
            Out.println(t"tel: [↯/t.6.12] no schema named `$name` in the registry")
            SchemaMissing
