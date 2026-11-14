package tel

import soundness.*

// `soundness` re-exports Proscenium's collections, but an *exported* opaque type does not carry its
// companion's extensions into implicit scope — `.stdlib`, the `::` cons and the `.to(List)` factory
// would all be unavailable — so the collection types come straight from Proscenium, as the Soundness
// modules themselves do. A named import outranks the `soundness` wildcard.
import proscenium.{List, Nil, Chain}

// Schema names are identifiers, so the listing is ordered by code point. `Text` is comparable
// only once a collation has been chosen, and the given which makes it so is lexically scoped.
import soundness.collationComparable
import collations.codepointCollation
import sortingAlgorithms.timsort
// Taking the tail of a `List` is gated as a linear operation; the lists here are the handful of
// components in one schema signature.
import dysasymptotics.linearSize
import pathInterfaces.pathOnLinux
import systems.javaBaseSystem
import filesystemBackends.javaBaseFilesystem
import filesystemOptions.overwritePreexisting
import textSanitizers.skipSanitizer
import logging.silentLogging
import codepages.utf8Codepage
import charsets.utf8Charset

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
    val declared = tel.as[Tels].layers.to[List].map(_.name).zip(layerHashes)
    val selected = layers.flatMap(layer => declared.filter(_(0) == layer).map(_(1)))
    Base256.encode(SchemaSignature.encode(baseHash :: selected))

  // A registered schema's component hashes (BinTEL §8.1), each layer's paired with its declared
  // name: the library a pragma signature is decoded against.
  private def components(tel: Tel)(using Tactic[Bintel.Error], Tactic[Tel.Error])
  :   (Data, List[(Text, Data)]) =

    val (baseHash, layerHashes) = SchemaSignature.componentHashes(tel, Tels.Axiom.tels)
    (baseHash, tel.as[Tels].layers.to[List].map(_.name).zip(layerHashes))

  // `Data` carries no structural equality, so hashes are compared by their BASE-256 rendering.
  private def sameHash(a: Data, b: Data): Boolean = Base256.encode(a) == Base256.encode(b)

  // Parse + summarise a schema for the listing (base-schema id + declared layer names).
  private def entryOf(tel: Tel)(using Tactic[Bintel.Error], Tactic[Tel.Error]): Entry =
    val tels = tel.as[Tels]
    Entry(tels.name, signature(tel, Nil), tels.layers.map(_.name).join(t", "))

  // The §20.1 checks Stratiform's `Tels.Validation` does not yet perform: every type reference
  // must resolve within the composed namespace, and to a Definition of the right kind. A `Field`
  // or `Variant` may reference a record or a scalar (the built-ins are prepended into
  // `schema.scalars` at reconstruction, so they resolve here too), but not a select (E217 — the
  // sum-typed member form is the `SelectRef`); a `SelectRef` must reference a select. A name that
  // resolves to nothing at all is E209. Returns each offending TypeName with its reason, in
  // declaration order; run against the COMPOSED schema, so layer-introduced definitions and
  // references are both in scope.
  def incoherences(schema: Tels): List[(Text, Tel.Error.Reason)] =
    def kindOf(name: Text): Optional[Text] =
      if schema.records.exists(_.name == name) then t"record"
      else if schema.scalars.exists(_.name == name) then t"scalar"
      else if schema.selects.exists(_.name == name) then t"select"
      else Unset

    def checkType(fieldType: Tels.Type): List[(Text, Tel.Error.Reason)] = fieldType match
      case Tels.Reference(name) => kindOf(name) match
        case t"record" | t"scalar" => Nil
        case t"select"             => List((name, Tel.Error.Reason.ReferenceKindMismatch))
        case _                     => List((name, Tel.Error.Reason.UnresolvedReference))

      case struct: Tels.Struct => checkMembers(struct.members.to[List])
      case _                   => Nil

    def checkMembers(members: List[Tels.Member]): List[(Text, Tel.Error.Reason)] =
      members.flatMap:
        case field: Tels.Field => checkType(field.fieldType)

        case select: Tels.SelectRef => kindOf(select.reference) match
          case t"select"             => Nil
          case t"record" | t"scalar" => List((select.reference, Tel.Error.Reason.ReferenceKindMismatch))
          case _                     => List((select.reference, Tel.Error.Reason.UnresolvedReference))

        case _ => Nil

    checkMembers(schema.document.members.to[List])
    + schema.records.to[List].flatMap(record => checkMembers(record.members.to[List]))
    + schema.selects.to[List].flatMap: select =>
        select.variants.to[List].flatMap(variant => checkType(variant.variantType))

  private def read(file: Path on Linux)
      (using Tactic[Tel.Error], Tactic[Io.Error], Tactic[Truncation.Error])
  :   Tel =
    file.read[Text].read[Tel]

  // The raw text of a cache file (the filesystem givens live here, not in the server).
  def readText(file: Path on Linux): Optional[Text] = safely(file.read[Text])

  // The listing entry for a single cache file (used by the LSP to describe a resolved schema).
  def describe(file: Path on Linux): Optional[Entry] = safely(entryOf(read(file)))

  // When the registry directory last changed, in milliseconds since the epoch, or `0` if it
  // cannot be read: what the LSP keys its memo of resolutions on, since `tel schema add` writes
  // into the directory.
  def modified(directory: Path on Linux): Long =
    safely(summon[FilesystemBackend on Linux].stat(directory, true).modified).or(0L)

  // Cached schema files are stored read-only, so an editor opened at one (via the LSP's cross-file
  // go-to-definition) presents it as read-only. A copy is made writable again only to be replaced.
  private def protect(file: Path on Linux): Unit = safely(file.writable() = false)
  private def unprotect(file: Path on Linux): Unit = safely(file.writable() = true)

  // The schemas every implementation holds built in (TEL §8.2, resolution step 1): the `tels`
  // meta-schema and the `acceptance` schema (BinTEL §8.4), each under its declared name.
  private val builtins: List[(Text, Text)] =
    List(t"tels" -> MetaSchema.source, t"acceptance" -> MetaSchema.acceptance)

  // Write the built-in schemas into the cache, so the registry always contains them — and refresh
  // any copy whose text differs from the embedded source, so an upgraded `tel` never resolves a
  // stale pin. Best-effort (the cache may be unwritable).
  def ensurePreloaded(directory: Path on Linux): Unit =
    builtins.each: (name, source) =>
      safely:
        val file = t"${directory.encode}/$name.tel".as[Path on Linux]
        val current = if file.existent() then readText(file).lay(false)(_ == source) else false

        if !current then
          if !directory.existent() then directory.create[Directory](CreateFlag.Parents)
          unprotect(file)
          file.write(source)
          protect(file)

  // Every cached schema, sorted by name; unreadable or unparseable files are skipped.
  def entries(directory: Path on Linux): List[Entry] =
    ensurePreloaded(directory)
    safely(directory.children.to[List]).or(Nil).map(file => safely(entryOf(read(file))))
    . sweep { case entry: Entry => entry }
    . order(_.name)

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
    incoherences(composed).head.let { (_, reason) => abort(Tel.Error(reason)) }

    if !directory.existent() then directory.create[Directory](CreateFlag.Parents)
    val target = t"${directory.encode}/${entry.name}.tel".as[Path on Linux]
    unprotect(target)                   // a prior copy is stored read-only
    target.write(text)
    protect(target)                   // keep the registry copy read-only
    entry

  // Stratiform's §20.1 validity battery over the composed schema.
  def validated(tel: Tel)(using Tactic[Tel.Error]): Tels =
    Tels.Validation.validate(tel.as[Tels])

  // The declared layer names of the schema cached under `name`, in declaration order. Used to
  // tab-complete the layer operands of `tel schema signature <name> [layer…]`, where the
  // admissible layers are exactly those the named schema declares.
  def layerNames(directory: Path on Linux, name: Text): List[Text] =
    load(directory, name) match
      case tel: Tel =>
        safely(tel.as[Tels].layers.to[List].map(_.name)).or(Nil)

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
          // `children` is a `Chain`, so each registered schema is read and decoded only until
          // one of them accounts for the signature.
          safely(directory.children).or(Chain())
          . map(file => safely(decodeAgainst(file, claimed, selection)))
          . seek(_.present)
          . or(Lookup.Missing)

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
        val base = tel.as[Tels]
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
      decoded.head.let: first =>
        if !sameHash(first, baseHash) then Unset
        else
          val names = decoded.tail.or(Nil).map: hash =>
            layers.seek((_, candidate) => sameHash(candidate, hash)).let(_(0))

          if names.exists(_.absent) then Unset
          else
            val layerNames: List[Text] = names.sweep { case name: Text => name }

            if !selection.nil && selection != layerNames then
              val named = layerNames.map(name => t"`$name`").join(t", ")

              Lookup.Disagreement
                ( file, t"the signature names the layers $named, not the `+` layer selections" )
            else
              compose(file, tel, layerNames)
