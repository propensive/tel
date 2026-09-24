package tel

import soundness.*

// `soundness` re-exports Proscenium's collections, but an *exported* opaque type does not carry its
// companion's extensions into implicit scope — `.stdlib`, the `::` cons and the `.to(List)` factory
// would all be unavailable — so the collection types come straight from Proscenium, as the Soundness
// modules themselves do. A named import outranks the `soundness` wildcard.
import proscenium.{List, Nil, Chain}

import pathInterfaces.pathOnLinux
import systems.javaBaseSystem
import filesystemBackends.javaBaseFilesystem
import filesystemOptions.overwritePreexisting
import textSanitizers.skipSanitizer
import logging.silentLogging
import charEncoders.utf8Encoder
import charDecoders.utf8Decoder

// A per-user registry of TEL schemas, shared by the `tel schema …` subcommands and the LSP. Schemas
// live as `<name>.tel` files under `$XDG_CACHE_HOME/tel/schemas` (or `~/.cache/tel/schemas`). A schema
// is validated against the built-in TELS meta-schema before it is cached, so the registry only
// ever holds well-formed schemas, and the LSP can load them to validate ordinary documents. The
// registry is the local content-addressed store of the Resolution Protocol (TEL §8.2, steps 2–3):
// a signature is resolved by decoding its palimpsest against the component hashes of every
// registered schema, and a bare reference by its module name.
object SchemaCache:

  // A summary of one cached schema, for `tel schema list`.
  case class Entry(name: Text, id: Text, layers: Text) derives CanEqual

  // The cache directory, honouring `$XDG_CACHE_HOME`. Resolved where an invoker `Environment` is in
  // scope (the CLI, and once at LSP start-up).
  def directory(using Environment, System, Tactic[Path.Error]): Path on Linux =
    t"${Xdg.cacheHome[Path on Linux].encode}/tel/schemas".as[Path on Linux]

  // The BASE-256 palimpsest for a parsed schema composed with the named layers, in order (empty = the
  // base schema alone). Unknown layer names are ignored.
  def signature(tel: Tel, layers: List[Text])(using Tactic[Bintel.Error], Tactic[Tel.Error]): Text =
    val (baseHash, layerHashes) = SchemaSignature.componentHashes(tel, Tels.Axiom.tels)
    val names = Tels.Reconstructor.fromTel(tel).layers.readable.to(scala.List).map(_.name)
    val byName = names.zip(layerHashes.stdlib).toMap
    Base256.encode(SchemaSignature.encode(baseHash :: layers.stdlib.flatMap(byName.get).to(List)))

  // A registered schema's component hashes (BinTEL §8.1), each layer's paired with its declared
  // name: the library a pragma signature is decoded against.
  private def components(tel: Tel)(using Tactic[Bintel.Error], Tactic[Tel.Error])
  :   (Data, List[(Text, Data)]) =

    val (baseHash, layerHashes) = SchemaSignature.componentHashes(tel, Tels.Axiom.tels)
    val names = Tels.Reconstructor.fromTel(tel).layers.readable.to(scala.List).map(_.name)
    (baseHash, names.zip(layerHashes.stdlib).to(List))

  // `Data` carries no structural equality, so hashes are compared by their BASE-256 rendering.
  private def sameHash(a: Data, b: Data): Boolean = Base256.encode(a) == Base256.encode(b)

  // Parse + summarise a schema for the listing (base-schema id + declared layer names).
  private def entryOf(tel: Tel)(using Tactic[Bintel.Error], Tactic[Tel.Error]): Entry =
    val tels = Tels.Reconstructor.fromTel(tel)
    Entry(tels.name, signature(tel, Nil), tels.layers.readable.to(List).map(_.name).join(t", "))

  // The §20.1 checks Stratiform's `Tels.Validation` does not yet perform: every type reference
  // must resolve within the composed namespace, and to a Definition of the right kind. A `Field`
  // or `Variant` may reference a record or a scalar (the built-ins are prepended into
  // `schema.scalars` at reconstruction, so they resolve here too), but not a select (E217 — the
  // sum-typed member form is the `SelectRef`); a `SelectRef` must reference a select. A name that
  // resolves to nothing at all is E209. Returns each offending TypeName with its reason, in
  // declaration order; run against the COMPOSED schema, so layer-introduced definitions and
  // references are both in scope.
  def incoherences(schema: Tels): scala.List[(Text, Tel.Error.Reason)] =
    def kindOf(name: Text): Optional[Text] =
      if schema.records.readable.exists(_.name == name) then t"record"
      else if schema.scalars.readable.exists(_.name == name) then t"scalar"
      else if schema.selects.readable.exists(_.name == name) then t"select"
      else Unset

    def checkType(fieldType: Tels.Type): scala.List[(Text, Tel.Error.Reason)] = fieldType match
      case Tels.Reference(name) => kindOf(name) match
        case t"record" | t"scalar" => scala.Nil
        case t"select"             => scala.List((name, Tel.Error.Reason.ReferenceKindMismatch))
        case _                     => scala.List((name, Tel.Error.Reason.UnresolvedReference))

      case struct: Tels.Struct => checkMembers(struct.members.readable.to(scala.List))
      case _                   => scala.Nil

    def checkMembers(members: scala.List[Tels.Member]): scala.List[(Text, Tel.Error.Reason)] =
      members.flatMap:
        case field: Tels.Field => checkType(field.fieldType)

        case select: Tels.SelectRef => kindOf(select.reference) match
          case t"select"             => scala.Nil
          case t"record" | t"scalar" => scala.List((select.reference, Tel.Error.Reason.ReferenceKindMismatch))
          case _                     => scala.List((select.reference, Tel.Error.Reason.UnresolvedReference))

        case _ => scala.Nil

    checkMembers(schema.document.members.readable.to(scala.List))
    ++ schema.records.readable.to(scala.List)
       . flatMap(record => checkMembers(record.members.readable.to(scala.List)))
    ++ schema.selects.readable.to(scala.List).flatMap: select =>
         select.variants.readable.to(scala.List).flatMap(variant => checkType(variant.variantType))

  private def read(file: Path on Linux)
      (using Tactic[Tel.Error], Tactic[Io.Error], Tactic[Truncation.Error])
  :   Tel =
    file.read[Text].read[Tel]

  // The raw text of a cache file (the filesystem givens live here, not in the server).
  def readText(file: Path on Linux): Optional[Text] = safely(file.read[Text])

  // The listing entry for a single cache file (used by the LSP to describe a resolved schema).
  def describe(file: Path on Linux): Optional[Entry] = safely(entryOf(read(file)))

  // Cached schema files are stored read-only, so an editor opened at one (via the LSP's cross-file
  // go-to-definition) presents it as read-only. These use `java.io.File` because the registry copy is
  // a managed artifact whose permission bit is being toggled, not filesystem I/O the typed API mediates.
  private def markReadOnly(file: Path on Linux): Unit = safely(java.io.File(file.encode.s).setReadOnly())
  private def makeWritable(file: Path on Linux): Unit = safely(java.io.File(file.encode.s).setWritable(true))

  // The schemas every implementation holds built in (TEL §8.2, resolution step 1): the `tels`
  // meta-schema and the `acceptance` schema (BinTEL §8.4), each under its declared name.
  private val builtins: scala.List[(Text, Text)] =
    scala.List(t"tels" -> MetaSchema.source, t"acceptance" -> MetaSchema.acceptance)

  // Write the built-in schemas into the cache, so the registry always contains them — and refresh
  // any copy whose text differs from the embedded source, so an upgraded `tel` never resolves a
  // stale pin. Best-effort (the cache may be unwritable).
  def ensurePreloaded(directory: Path on Linux): Unit =
    builtins.foreach: (name, source) =>
      safely:
        val file = t"${directory.encode}/$name.tel".as[Path on Linux]
        val current = if file.existent() then readText(file).lay(false)(_ == source) else false

        if !current then
          if !directory.existent() then directory.create[Directory](CreateFlag.Parents)
          makeWritable(file)
          file.write(source)
          markReadOnly(file)

  // Every cached schema, sorted by name; unreadable or unparseable files are skipped.
  def entries(directory: Path on Linux): List[Entry] =
    ensurePreloaded(directory)
    safely(directory.children.stdlib.to(scala.List)).or(scala.Nil).flatMap: file =>
      safely(entryOf(read(file))).let(scala.List(_)).or(scala.Nil)
    . sortBy(_.name.s).to(List)

  // Add a schema file to the cache: validate it against the meta-schema, then store it under its
  // declared name. Returns the added entry. Raises if the file is missing or is not a valid schema.
  def add(directory: Path on Linux, file: Path on Linux)
      (using Tactic[Bintel.Error], Tactic[Tel.Error], Tactic[Io.Error], Tactic[Truncation.Error],
             Tactic[Path.Error])
  :   Entry =
    val text = file.read[Text]
    val tel = text.read[Tel]
    val entry = entryOf(tel)              // reconstructs the Tels (raises if malformed) and its id

    // Full §20.1 verification before acceptance: compose the layers and run Stratiform's schema
    // validity battery, then the reference-coherence checks it does not yet include. An
    // incoherent schema would otherwise be accepted here and only fail later, when a document is
    // validated against it.
    val composed = validated(tel)
    incoherences(composed).headOption.foreach { (_, reason) => abort(Tel.Error(reason)) }

    if !directory.existent() then directory.create[Directory](CreateFlag.Parents)
    val target = t"${directory.encode}/${entry.name}.tel".as[Path on Linux]
    makeWritable(target)                   // a prior copy is stored read-only
    target.write(text)
    markReadOnly(target)                   // keep the registry copy read-only
    entry

  // Stratiform's §20.1 validity battery over the composed schema. E224 (a scalar declaring neither
  // `validate` nor `pattern`) was withdrawn from the specification — an unconstrained scalar is
  // valid — but Stratiform 0.67 still raises it, aborting the battery at that point; such a schema
  // is composed without the battery instead, until Stratiform catches up.
  def validated(tel: Tel)(using Tactic[Tel.Error]): Tels =
    recover:
      case error: Tel.Error =>
        if error.reason == Tel.Error.Reason.UnconstrainedScalar
        then Tels.Layers.compose(Tels.Reconstructor.fromTel(tel))
        else abort(error)

    . protect:
        Tels.Validation.validate(Tels.Reconstructor.fromTel(tel))

  // The declared layer names of the schema cached under `name`, in declaration order. Used to
  // tab-complete the layer operands of `tel schema signature <name> [layer…]`, where the
  // admissible layers are exactly those the named schema declares.
  def layerNames(directory: Path on Linux, name: Text): List[Text] =
    load(directory, name) match
      case tel: Tel =>
        safely(Tels.Reconstructor.fromTel(tel).layers.readable.to(List).map(_.name)).or(Nil)

      case _ =>
        Nil

  // The parsed schema `Tel` cached under `name`, or `Unset` if there is none.
  def load(directory: Path on Linux, name: Text): Optional[Tel] =
    ensurePreloaded(directory)
    safely:
      val file = t"${directory.encode}/${name}.tel".as[Path on Linux]
      if file.existent() then read(file) else Unset

  // The outcome of looking a pragma schema identification up in the registry.
  enum Lookup:
    // The schema composed with `layers` (the pragma's selection, or the layers a signature named,
    // in order), its registry file, and the composed signature.
    case Found(file: Path on Linux, schema: Tels, layers: List[Text], signature: Text)
    // The schema is registered, but a selected layer is not one it declares: a runtime resolution
    // error (§8.1), distinct from a mis-ordered selection.
    case UnknownLayer(file: Path on Linux, layer: Text)
    // The schema is registered, but the layers are not in its declaration order (E124).
    case LayerOrder(file: Path on Linux)
    // The signature resolves, but not to what the pragma's other phrases claim (§8.1).
    case Disagreement(file: Path on Linux, detail: Text)
    case Missing

  // Resolve a pragma schema identifier — a schema name (a LIRA reference's module-name tail), or
  // a BASE-256 schema signature — against the registry. A name resolves to the base schema
  // composed with exactly the selected layers (§8.1; none = the base alone). A signature is
  // decoded (BinTEL §8.2) against the component hashes of each registered schema, the pragma's
  // layer selections serving as decomposition hints, and resolves to the base composed with the
  // layers the signature names, in the order it names them; when the pragma also selects
  // layers, the signature is authoritative and MUST name exactly those (§8.1).
  def lookup(directory: Path on Linux, identifier: Text, selection: List[Text] = Nil): Lookup =
    ensurePreloaded(directory)

    val named = safely:
      val file = t"${directory.encode}/${identifier}.tel".as[Path on Linux]
      if file.existent() then compose(file, read(file), selection) else Unset

    named.or:
      safely(Base256.decodeStrict(identifier)).lay(Lookup.Missing): claimed =>
        if safely(SchemaSignature.componentCount(claimed)).absent then Lookup.Missing
        else
          safely(directory.children.stdlib.to(scala.List)).or(scala.Nil).iterator
          . map(file => safely(decodeAgainst(file, claimed, selection)).or(Unset))
          . collectFirst { case found: Lookup => found }
          . getOrElse(Lookup.Missing)

  // Compose a registered schema with a layer selection, telling the two ways a selection can
  // fail apart: Stratiform raises `Resolution.Error` for an undeclared name and E124 for an
  // order violation.
  private def compose(file: Path on Linux, tel: Tel, selection: List[Text]): Lookup =
    recover:
      case error: Tels.Resolution.Error => error.reason match
        case Tels.Resolution.Error.Reason.UnknownLayer(layer) => Lookup.UnknownLayer(file, layer)
        case _                                                => Lookup.Missing

      case error: Tel.Error =>
        if error.reason == Tel.Error.Reason.LayerOrderMismatch then Lookup.LayerOrder(file)
        else Lookup.Missing

      case error: Bintel.Error => Lookup.Missing

    . protect:
        val base = Tels.Reconstructor.fromTel(tel)
        val schema = if selection.nil then base else Tels.Layers.compose(base, selection)
        Lookup.Found(file, schema, selection, signature(tel, selection))

  // Decode a claimed signature against one registered schema's components. `Unset` when the
  // signature does not decompose over them — it belongs to some other schema.
  private def decodeAgainst(file: Path on Linux, claimed: Data, selection: List[Text])
      ( using Tactic[Tel.Error], Tactic[Io.Error], Tactic[Truncation.Error],
              Tactic[Bintel.Error] )
  :   Optional[Lookup] =

    val tel = read(file)
    val (baseHash, layers) = components(tel)

    SchemaSignature.decodeHinted(claimed, baseHash, layers, selection).let: decoded =>
      val hashes = decoded.stdlib

      if hashes.isEmpty || !sameHash(hashes.head, baseHash) then Unset
      else
        val names = hashes.tail.map: hash =>
          layers.stdlib.find((_, candidate) => sameHash(candidate, hash)).map(_(0))

        if names.exists(_.isEmpty) then Unset
        else
          val layerNames = names.flatten.to(List)

          if !selection.nil && selection.stdlib != names.flatten then
            val named = layerNames.map(name => t"`$name`").join(t", ")

            Lookup.Disagreement
              ( file, t"the signature names the layers $named, not the `+` layer selections" )
          else
            compose(file, tel, layerNames)
