package tel

import soundness.*

import interfaces.paths.pathOnLinux
import systems.javaSystem
import filesystemBackends.virtualMachine
import filesystemOptions.createNonexistentParents.enabled
import filesystemOptions.overwritePreexisting.enabled
import filesystemOptions.dereferenceSymlinks.enabled
import filesystemOptions.deleteRecursively.disabled
import filesystemOptions.readAccess.enabled
import filesystemOptions.writeAccess.enabled
import textSanitizers.skipSanitizer
import logging.silentLogging
import charEncoders.utf8Encoder
import charDecoders.utf8Decoder

// A per-user registry of TEL schemas, shared by the `tel schema …` subcommands and the LSP. Schemas
// live as `<name>.tel` files under `$XDG_CACHE_HOME/tel/schemas` (or `~/.cache/tel/schemas`). A schema
// is validated against the built-in tel-schema meta-schema before it is cached, so the registry only
// ever holds well-formed schemas, and the LSP can load them to validate ordinary documents.
object SchemaCache:

  // A summary of one cached schema, for `tel schema list`.
  case class Entry(name: Text, id: Text, layers: Text) derives CanEqual

  // The cache directory, honouring `$XDG_CACHE_HOME`. Resolved where an invoker `Environment` is in
  // scope (the CLI, and once at LSP start-up).
  def directory(using Environment, System, Tactic[PathError]): Path on Linux =
    t"${Xdg.cacheHome[Path on Linux].encode}/tel/schemas".decode[Path on Linux]

  // The BASE-256 palimpsest for a parsed schema composed with the named layers, in order (empty = the
  // base schema alone). Unknown layer names are ignored.
  def signature(tel: Tel, layers: List[Text])(using Tactic[BintelError], Tactic[TelError]): Text =
    val (baseHash, layerHashes) = SchemaSignature.componentHashes(tel, Tels.Axiom.tels)
    val byName = Tels.Reconstructor.fromTel(tel).layers.to(List).map(_.name).zip(layerHashes).to(Map)
    Base256.encode(SchemaSignature.encode(baseHash :: layers.flatMap(byName.get)))

  // Parse + summarise a schema for the listing (base-schema id + declared layer names).
  private def entryOf(tel: Tel)(using Tactic[BintelError], Tactic[TelError]): Entry =
    val tels = Tels.Reconstructor.fromTel(tel)
    Entry(tels.name, signature(tel, Nil), tels.layers.to(List).map(_.name).join(t", "))

  private def read(file: Path on Linux)
      (using Tactic[TelError], Tactic[IoError], Tactic[StreamError])
  :   Tel =
    file.read[Text].read[Tel]

  // The raw text of a cache file (the filesystem givens live here, not in the server).
  def readText(file: Path on Linux): Optional[Text] = safely(file.read[Text])

  // Cached schema files are stored read-only, so an editor opened at one (via the LSP's cross-file
  // go-to-definition) presents it as read-only. These use `java.io.File` because the registry copy is
  // a managed artifact whose permission bit is being toggled, not filesystem I/O the typed API mediates.
  private def markReadOnly(file: Path on Linux): Unit = safely(java.io.File(file.encode.s).setReadOnly())
  private def makeWritable(file: Path on Linux): Unit = safely(java.io.File(file.encode.s).setWritable(true))

  // Write the built-in tel-schema meta-schema into the cache if it is not already there, so the
  // registry always contains it. Best-effort (the cache may be unwritable).
  def ensurePreloaded(directory: Path on Linux): Unit =
    safely:
      val file = t"${directory.encode}/tel-schema.tel".decode[Path on Linux]
      if !file.exists() then
        if !directory.exists() then directory.create[Directory]()
        file.write(MetaSchema.source)
        markReadOnly(file)

  // Every cached schema, sorted by name; unreadable or unparseable files are skipped.
  def entries(directory: Path on Linux): List[Entry] =
    ensurePreloaded(directory)
    safely(directory.children.to(List)).or(Nil).flatMap: file =>
      safely(entryOf(read(file))).let(List(_)).or(Nil)
    . sortBy(_.name)

  // Add a schema file to the cache: validate it against the meta-schema, then store it under its
  // declared name. Returns the added entry. Raises if the file is missing or is not a valid schema.
  def add(directory: Path on Linux, file: Path on Linux)
      (using Tactic[BintelError], Tactic[TelError], Tactic[IoError], Tactic[StreamError],
             Tactic[PathError])
  :   Entry =
    val text = file.read[Text]
    val entry = entryOf(text.read[Tel])   // reconstructs the Tels (raises if malformed) and its id
    if !directory.exists() then directory.create[Directory]()
    val target = t"${directory.encode}/${entry.name}.tel".decode[Path on Linux]
    makeWritable(target)                   // a prior copy is stored read-only
    target.write(text)
    markReadOnly(target)                   // keep the registry copy read-only
    entry

  // The parsed schema `Tel` cached under `name`, or `Unset` if there is none.
  def load(directory: Path on Linux, name: Text): Optional[Tel] =
    ensurePreloaded(directory)
    safely:
      val file = t"${directory.encode}/${name}.tel".decode[Path on Linux]
      if file.exists() then read(file) else Unset

  // As `resolve`, but returns the cache *file* backing the identifier (for cross-file navigation from
  // a document into its schema). Matches by name first, then by base/fully-composed signature.
  def resolveFile(directory: Path on Linux, identifier: Text): Optional[Path on Linux] =
    ensurePreloaded(directory)
    val byName = safely:
      val file = t"${directory.encode}/${identifier}.tel".decode[Path on Linux]
      if file.exists() then file else Unset

    byName.or:
      safely(directory.children.to(List)).or(Nil).find: file =>
        safely:
          val tel = read(file)
          val base = signature(tel, Nil)
          val full = signature(tel, Tels.Reconstructor.fromTel(tel).layers.to(List).map(_.name).to(List))
          identifier == base || identifier == full
        . or(false)
      . getOrElse(Unset)

  // Resolve a pragma schema identifier — a bare schema name, or a bare BASE-256 signature — to a
  // `Tels`, or `Unset` if it is neither cached nor resolvable. A name resolves to the base schema; a
  // signature resolves to whichever cached schema's base or fully-composed signature it matches.
  def resolve(directory: Path on Linux, identifier: Text): Optional[Tels] =
    ensurePreloaded(directory)
    val byName = safely:
      val file = t"${directory.encode}/${identifier}.tel".decode[Path on Linux]
      if file.exists() then Tels.Reconstructor.fromTel(read(file)) else Unset

    byName.or:
      safely(directory.children.to(List)).or(Nil).map: file =>
        safely:
          val tel = read(file)
          val base = signature(tel, Nil)
          val full = signature(tel, Tels.Reconstructor.fromTel(tel).layers.to(List).map(_.name).to(List))
          if identifier == base then Tels.Reconstructor.fromTel(tel)
          else if identifier == full then Tels.Layers.compose(Tels.Reconstructor.fromTel(tel))
          else Unset
        . or(Unset)
      . find(!_.absent).getOrElse(Unset)
