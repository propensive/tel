package tel

import soundness.*

// See the note in `tel.TelServer.scala`: the collection types come straight from Proscenium.
import proscenium.{List, Nil}

// Positions in a `List` are read, and its length taken, only on the handful of results a test
// inspects.
import dysasymptotics.linearSize
// Diagnostics and edits are put in order before they are compared.
import sortingAlgorithms.timsort
import strategies.throwUnsafely
import pathInterfaces.pathOnLinux
import codepages.utf8Codepage
import filesystemBackends.javaBaseFilesystem
import filesystemOptions.overwritePreexisting
import logging.silentLogging
import systems.javaBaseSystem

// Tests for the LSP server's pure handler functions: no JSON-RPC transport is involved, and the
// schema registry is a throwaway temporary directory, so the suite exercises exactly the logic the
// handlers close over. Run with `make test` (fume) or `make test-plain`.
object Tests extends Suite(m"TEL LSP server tests"):

  // A small contact schema, registered under `contact.tel` in the temporary registry.
  val schemaText: Text = """tel 1.0 specification.tel/tels:2.1.0

name contact

record PhoneNumber
  field country-code String
  field number String

record Contact
  field label String optional
  select Status optional

select Status
  variant active Flag
  variant archived Flag

document
  field name String
  field email String optional
    description
        The contact's primary email address.
  field phone PhoneNumber optional repeatable
  field contact Contact optional
  select Status optional
""".tt

  // A well-formed document conforming to the contact schema.
  def document(identifier: Text): Text =
    t"tel 1.0 $identifier\n\nname Alice\nemail alice@example.org\ncontact active\n"

  // A schema whose repeatable `pet` member is typed by a record with a `key` field (§20), so that
  // sibling pets must have distinct names (§21.6, E314).
  val keyedSchemaText: Text = """tel 1.0 specification.tel/tels:2.1.0

name keyed

record Pet
  field name Identifier key
  field colour String optional

document
  field pet Pet optional repeatable
""".tt

  // Two pets, the second named `second`: passing `amy` (the first pet's name) collides.
  def keyedDocument(identifier: Text, second: Text): Text =
    t"tel 1.0 $identifier\n\npet amy\npet $second\n"

  // A schema whose record leads with a required Flag, so a flag atom's positional and keyword
  // bindings coincide — the meaning-preserving case for the inline/expand code actions.
  val flagsSchemaText: Text = """tel 1.0 specification.tel/tels:2.1.0

name flags

record Widget
  field visible Flag
  field label String optional

document
  field widget Widget optional repeatable
""".tt

  // A schema with an optional layer (§8.1): a document opts into `extras` with `+extras` in its
  // pragma; without it, only the base schema's members are admissible.
  val themedSchemaText: Text = """tel 1.0 specification.tel/tels:2.1.0

name themed

document
  field title String

layer
  name extras
  overlay
    field subtitle String optional

layer
  name footer
  overlay
    field footer String optional
""".tt

  // A schema whose scalar carries a `pattern` constraint (§21.8), so a value that does not match
  // it is E315.
  val patternSchemaText: Text = """tel 1.0 specification.tel/tels:2.1.0

name coded

scalar Code
  pattern [A-Z]{2}

scalar Blob
  validate string
  encoding hex-bytes

document
  field code Code
  field blob Blob optional
""".tt

  // A document with source-atom and literal-atom payloads (§14, §15): the payload lines must not
  // be mistaken for compounds by the structure scan.
  val atomForms: Text = """tel 1.0 example.org/documents-by-form

inline-value ipv4-strict

source-value
    { "address": "192.0.2.1",
      "mask":    "255.255.255.0" }

literal-value
      ---
# a comment-like payload line
field looks-like-a-compound
      ---
""".tt

  // A fresh directory beneath the system's temporary directory, which the test leaves behind
  // for the system to clear.
  def scratch(label: Text): Path on Linux =
    val temporary: Text = System.properties.java.io.tmpdir[Text]()
    val base = if temporary.ends(t"/") then temporary.skip(1, Rtl) else temporary
    val directory = t"$base/$label-${Uuid().show}".as[Path on Linux]
    directory.create[Directory](CreateFlag.Parents)
    directory

  // Every edit an action carries.
  def editsOf(action: Lsp.CodeAction): List[Lsp.TextEdit] =
    action.edit.let(_.changes).lay(Nil)(_.values.flatMap(identity(_)))

  // Writes `content` to the file `name` in `directory`, and yields the file.
  def store(directory: Path on Linux, name: Text, content: Text): Path on Linux =
    val file = t"${directory.encode}/$name".as[Path on Linux]
    file.write(content)
    file

  def run(): Unit =
    val registryDir = scratch(t"tel-lsp-tests")
    val schemaFile = store(registryDir, t"contact.tel", schemaText)
    val keyedFile = store(registryDir, t"keyed.tel", keyedSchemaText)
    val flagsFile = store(registryDir, t"flags.tel", flagsSchemaText)
    store(registryDir, t"themed.tel", themedSchemaText)
    store(registryDir, t"coded.tel", patternSchemaText)

    val registry: TelServer.Registry = registryDir
    val resolver = TelServer.PragmaResolver(registry)
    val signature: Text = SchemaCache.describe(schemaFile).let(_.id).or(t"")
    val keyedSignature: Text = SchemaCache.describe(keyedFile).let(_.id).or(t"")
    val flagsSignature: Text = SchemaCache.describe(flagsFile).let(_.id).or(t"")

    // The themed schema's signature composed with just its second layer, `footer` — a component
    // sequence that is not a prefix of the declaration order, so only a palimpsest decode can
    // resolve it.
    val themedFooterSignature: Text =
      safely(SchemaCache.signature(themedSchemaText.read[Tel], List(t"footer"))).or(t"")
    val themedExtrasSignature: Text =
      safely(SchemaCache.signature(themedSchemaText.read[Tel], List(t"extras"))).or(t"")

    suite(m"Self-upgrade"):
      val manifest: Text =
        t"version\t0.1.0\nbuild\t1000\nsigned-by\tabc123\n"
        + t"macos-arm64\thttps://example.com/tel-macos-arm64\tDEADBEEF\n"
        + t"linux-x64\thttps://example.com/tel-linux-x64\t0123\n"

      test(m"A manifest's version and build are read"):
        UpgradeManifest.parse(manifest).let { release => t"${release.version} ${release.build}" }
      . assert(_ == t"0.1.0 1000")

      test(m"A manifest's executable for a platform is read"):
        UpgradeManifest.parse(manifest).let(_.executable(t"macos-arm64")).let(_.sha256)
      . assert(_ == t"DEADBEEF")

      test(m"A platform the manifest does not name has no executable"):
        UpgradeManifest.parse(manifest).let(_.executable(t"windows-x64")).absent
      . assert(_ == true)

      test(m"An empty signed-by is no signing key"):
        UpgradeManifest.parse(t"version\t0.1.0\nbuild\t1000\nsigned-by\t\n").let(_.signedBy).absent
      . assert(_ == true)

      test(m"A manifest without a build is not a release"):
        UpgradeManifest.parse(t"version\t0.1.0\n").absent
      . assert(_ == true)

      test(m"A manifest with a non-numeric build is not a release"):
        UpgradeManifest.parse(t"version\t0.1.0\nbuild\tone\n").absent
      . assert(_ == true)

      test(m"A build id stands for its version"):
        List(SelfUpgrade.version(1000), SelfUpgrade.version(1002003))
      . assert(_ == List(t"0.1.0", t"1.2.3"))

    suite(m"Schema resolution"):
      test(m"The registered schema's signature resolves"):
        resolver(signature)
      . assert:
          case TelServer.Resolution.Resolved(entry, _, _, _, _) => entry.name == t"contact"
          case _                                          => false

      test(m"An unknown identifier is Unresolved"):
        resolver(t"NoSuchSchema")
      . assert:
          case TelServer.Resolution.Unresolved(_) => true
          case _                                  => false

      test(m"A tel-schema pragma resolves to the meta-schema"):
        resolver(t"specification.tel/tels:2.1.0")
      . assert:
          case TelServer.Resolution.Meta(_, _) => true
          case _                               => false

    suite(m"Diagnostics"):
      test(m"A valid document yields no error diagnostics"):
        TelServer.diagnose(document(signature), resolver)
        . count(_.severity == Lsp.DiagnosticSeverity.Error)
      . assert(_ == 0)

      test(m"A schema violation yields an error diagnostic"):
        TelServer.diagnose(document(signature) + t"bogus value\n", resolver)
        . count(_.severity == Lsp.DiagnosticSeverity.Error)
      . assert(_ > 0)

      test(m"An unresolved schema yields an information diagnostic on the identifier"):
        TelServer.diagnose(document(t"example.org/no-such-schema"), resolver)
        . filter(_.code == t"schema-unresolved")
      . assert: diagnostics =>
          diagnostics.size == 1 && diagnostics.all: diagnostic =>
            diagnostic.severity == Lsp.DiagnosticSeverity.Information
            && diagnostic.range.start.line == 0 && diagnostic.range.start.character == 8

      test(m"A resolved schema yields no unresolved-schema diagnostic"):
        TelServer.diagnose(document(signature), resolver)
        . count(_.code == t"schema-unresolved")
      . assert(_ == 0)

      // Stratiform's schema reconstruction understands scalar `encoding` (§21.7), so a schema
      // declaring one is simply valid — this used to raise a spurious E306 downgraded to a warning.
      test(m"A scalar `encoding` declaration is accepted"):
        val encodingSchema = List
          ( t"tel 1.0 specification.tel/tels:2.1.0", t"", t"name enc", t"",
            t"scalar Code", t"  validate string", t"  encoding hex-bytes", t"",
            t"document", t"  field code Code" )
        . join(t"\n")

        TelServer.diagnose(encodingSchema, resolver)
        . count(_.severity == Lsp.DiagnosticSeverity.Error)
      . assert(_ == 0)

      // A `key` field (§20) is likewise part of the schema language now: the declaration is valid,
      // and duplicate key values among repeatable siblings are E314 (§21.6).
      test(m"A `key` field declaration is accepted"):
        TelServer.diagnose(keyedSchemaText, resolver)
        . count(_.severity == Lsp.DiagnosticSeverity.Error)
      . assert(_ == 0)

      test(m"Duplicate key values raise E314"):
        TelServer.diagnose(keyedDocument(keyedSignature, t"amy"), resolver)
        . filter(_.code == t"E314")
      . assert(_.size == 1)

      test(m"Distinct key values raise no E314"):
        TelServer.diagnose(keyedDocument(keyedSignature, t"cat"), resolver)
        . count(_.code == t"E314")
      . assert(_ == 0)

      test(m"A duplicate definition (E210) is located on the second declaration"):
        val duplicated = List
          ( t"tel 1.0 specification.tel/tels:2.1.0", t"", t"name dup", t"",
            t"record Foo", t"  field a String", t"", t"record Foo", t"  field b String", t"",
            t"document", t"  field foo Foo optional" )
        . join(t"\n")

        TelServer.diagnose(duplicated, resolver).filter(_.code == t"E210")
      . assert: diagnostics =>
          diagnostics.size == 1 && diagnostics.all(_.range.start.line == 7)

    suite(m"Validators, patterns and encodings"):
      // The four built-in validators (§21.5) run in the editor: `Identifier` rejects a capital.
      test(m"A built-in validator rejection is E310"):
        TelServer.diagnose(keyedDocument(keyedSignature, t"Cat"), resolver).map(_.code)
      . assert(_ == List(t"E310"))

      test(m"A value failing a pattern constraint is E315"):
        TelServer.diagnose(t"tel 1.0 example.org/coded\n\ncode ab\n", resolver).map(_.code)
      . assert(_ == List(t"E315"))

      test(m"A value matching its pattern constraint is accepted"):
        TelServer.diagnose(t"tel 1.0 example.org/coded\n\ncode AB\n", resolver).size
      . assert(_ == 0)

      // The editor binds only the codecs the specification defines; an application's encoding
      // leaves its values unchecked, which is a warning here rather than the E313 error an
      // application raises.
      test(m"An application-defined encoding is an E313 warning, not an error"):
        TelServer.diagnose(t"tel 1.0 example.org/coded\n\ncode AB\nblob 0a0b\n", resolver)
        . map(diagnostic => (diagnostic.code, diagnostic.severity))
      . assert(_ == List((t"E313", Lsp.DiagnosticSeverity.Warning)))

      // E224 was withdrawn: a scalar with no constraint is valid (§20).
      test(m"An unconstrained scalar declaration is valid"):
        val text = t"tel 1.0 specification.tel/tels:2.1.0\n\nname loose\n\nscalar Loose\n\ndocument\n  field x Loose\n"
        TelServer.diagnose(text, resolver).size
      . assert(_ == 0)

      test(m"Hovering a pattern-constrained value shows the pattern"):
        TelServer.hoverAt(t"tel 1.0 example.org/coded\n\ncode AB\n", Lsp.Position(2, 6), resolver)
      . assert(_.let(_.contents.value.contains(t"[A-Z]{2}")).or(false))

    suite(m"Acceptances (BinTEL §8.4)"):
      val acceptance = t"tel 1.0 specification.tel/acceptance:1.0.0\n\n"

      test(m"The acceptance schema is built in and resolves by its pinned coordinate"):
        resolver(t"specification.tel/acceptance:1.0.0")
      . assert:
          case TelServer.Resolution.Resolved(entry, _, _, _, _) => entry.name == t"acceptance"
          case _                                                => false

      test(m"An acceptance naming a signature and a component prefix validates"):
        val text = acceptance + t"accept $signature self-contained ABCD\naccept $keyedSignature\n"
        TelServer.diagnose(text, resolver).size
      . assert(_ == 0)

      test(m"A malformed signature in an acceptance is E312 from the schema-signature codec"):
        TelServer.diagnose(acceptance + t"accept ABCDEFG\n", resolver).map(_.code)
      . assert(_ == List(t"E312"))

      test(m"A component prefix shorter than four bytes is E315"):
        TelServer.diagnose(acceptance + t"accept $signature ABC\n", resolver).map(_.code)
      . assert(_ == List(t"E315"))

    suite(m"Registry preloading"):
      test(m"A stale built-in copy is refreshed from the embedded source"):
        val registry = scratch(t"tel-preload-test")
        val stale = store(registry, t"tels.tel", t"tel 1.0\n\nname tels\n\ndocument\n")
        SchemaCache.ensurePreloaded(registry)
        val acceptanceCopy = t"${registry.encode}/acceptance.tel".as[Path on Linux].existent()
        (SchemaCache.readText(stale) == MetaSchema.source, acceptanceCopy)
      . assert(_ == (true, true))

    suite(m"Structure scan"):
      test(m"Payload lines are not compounds"):
        TelServer.flatten(TelServer.structure(atomForms)(1)).map(_.keyword)
      . assert(_ == List(t"inline-value", t"source-value", t"literal-value"))

      test(m"A source-atom payload extends its compound's endLine"):
        TelServer.flatten(TelServer.structure(atomForms)(1))
        . seek(_.keyword == t"source-value").let(_.endLine)
      . assert(_ == 6)

      test(m"A literal-atom payload extends to its closing delimiter"):
        TelServer.flatten(TelServer.structure(atomForms)(1))
        . seek(_.keyword == t"literal-value").let(_.endLine)
      . assert(_ == 12)

    suite(m"Code actions"):
      val uri = t"file:///test.tel"
      def at(line: Int): Lsp.Range = Lsp.Range(Lsp.Position(line, 0), Lsp.Position(line, 0))

      // The `newText` of every edit the actions carry.
      def editTexts(actions: List[Lsp.CodeAction]): List[Text] =
        actions.flatMap(editsOf).map(_.newText)

      // Apply every edit of the first action, back-to-front so earlier offsets stay valid.
      def applied(text: Text, actions: List[Lsp.CodeAction]): Text =
        val lines = text.cut(t"\n")

        def offset(position: Lsp.Position): Int =
          lines.keep(position.line).fold(position.character)(_ + _.size + 1)

        actions.head.lay(text): action =>
          editsOf(action).order(edit => offset(edit.range.start)).reverse.fold(text): (result, edit) =>
            t"${result.keep(offset(edit.range.start))}${edit.newText}"
            + result.skip(offset(edit.range.end))

      // `pet amy` (line 2): `amy` binds positionally to Pet's required `name` field, so it can move
      // to an explicit `name amy` child line.
      test(m"Expanding the last atom of a record-typed compound is offered"):
        TelServer.codeActionsAt(uri, keyedDocument(keyedSignature, t"cat"), at(2), resolver)
        . map(_.title)
      . assert(_ == List(t"Move `amy` to a child compound line"))

      test(m"The expansion edit writes the member keyword on a child line"):
        editTexts(TelServer.codeActionsAt(uri, keyedDocument(keyedSignature, t"cat"), at(2), resolver))
      . assert(_ == List(t"\n  name amy"))

      test(m"The applied edit leaves a diagnostic-free document"):
        val text = keyedDocument(keyedSignature, t"cat")
        val actions = TelServer.codeActionsAt(uri, text, at(2), resolver)
        TelServer.diagnose(applied(text, actions), resolver)
        . count(_.severity == Lsp.DiagnosticSeverity.Error)
      . assert(_ == 0)

      // §20.8: in `contact active`, the atom binds to `label` — an optional Scalar member is never
      // skipped — NOT to the Status select. The expansion makes that binding explicit, which is
      // half the point of the refactoring.
      test(m"Expansion names the member the atom actually binds to"):
        editTexts(TelServer.codeActionsAt(uri, document(signature), at(4), resolver))
      . assert(_ == List(t"\n  label active"))

      // `name Alice`: the compound's own type is a Scalar, so its atom is its value, not a member
      // binding — there is nothing to expand.
      test(m"A scalar-typed compound's value atom offers no action"):
        TelServer.codeActionsAt(uri, document(signature), at(2), resolver).size
      . assert(_ == 0)

      test(m"No actions without a resolved schema"):
        TelServer.codeActionsAt(uri, document(t"NoSuchSchema"), at(2), resolver).size
      . assert(_ == 0)

      test(m"A remark on the line declines the action"):
        val text = t"tel 1.0 $keyedSignature\n\npet amy # first pet\n"
        TelServer.codeActionsAt(uri, text, at(2), resolver).size
      . assert(_ == 0)

      // ── The opposite direction: inlining a child compound as an atom ──

      // The keyed document with `pet amy` already in expanded form.
      def expandedKeyed(second: Text): Text =
        t"tel 1.0 $keyedSignature\n\npet\n  name amy\npet $second\n"

      test(m"Inlining a scalar-typed first child is offered"):
        TelServer.codeActionsAt(uri, expandedKeyed(t"cat"), at(3), resolver)
        . map(_.title)
      . assert(_ == List(t"Inline `amy` onto the `pet` line"))

      test(m"Inlining is declined when the child is not on the line below its parent"):
        val text = t"tel 1.0 $keyedSignature\n\npet\n\n  name amy\npet cat\n"
        TelServer.codeActionsAt(uri, text, at(4), resolver).size
      . assert(_ == 0)

      // Inlining `active` onto `contact` would produce `contact active`, whose atom binds to
      // `label` (an optional Scalar member is never skipped, §20.8) — NOT to the Status variant the
      // child names. The semantic-model comparison catches the re-binding and declines the action.
      test(m"Inlining is declined when the atom would re-bind to a different member"):
        val text = t"tel 1.0 $signature\n\nname Alice\ncontact\n  active\n"
        TelServer.codeActionsAt(uri, text, at(4), resolver).size
      . assert(_ == 0)

      // In the flags schema, Widget's first member is the required Flag itself, so inlining is
      // meaning-preserving and offered.
      test(m"Inlining a flag-shaped child uses its keyword"):
        val text = t"tel 1.0 $flagsSignature\n\nwidget\n  visible\n"
        TelServer.codeActionsAt(uri, text, at(3), resolver)
        . map(_.title)
      . assert(_ == List(t"Inline `visible` onto the `widget` line"))

      test(m"Flag expand and inline round-trip"):
        val original = t"tel 1.0 $flagsSignature\n\nwidget visible\n"
        val expanded = applied(original, TelServer.codeActionsAt(uri, original, at(2), resolver))
        val inlined = applied(expanded, TelServer.codeActionsAt(uri, expanded, at(3), resolver))
        inlined == original
      . assert(_ == true)

      // Expansion then inlining must return to the original text: the two actions are inverses.
      test(m"Expand and inline round-trip"):
        val original = keyedDocument(keyedSignature, t"cat")
        val expanded = applied(original, TelServer.codeActionsAt(uri, original, at(2), resolver))
        val inlined = applied(expanded, TelServer.codeActionsAt(uri, expanded, at(3), resolver))
        inlined == original
      . assert(_ == true)

      // ── Hard gaps (§10.3): a 2+-space run starts an atom and locks the line into hard mode ──

      // `contact  fluffy cat` is ONE atom, `fluffy cat`, binding Contact's optional `label` (a
      // `String`; a `key` field's `Identifier` would reject the space, E310, and no refactoring is
      // offered on an invalid document); the generated child line must also use a hard gap so the
      // value stays a single phrase.
      test(m"Expanding a hard-gap atom preserves it as one phrase"):
        val text = t"tel 1.0 $signature\n\nname Alice\ncontact  fluffy cat\n"
        editTexts(TelServer.codeActionsAt(uri, text, at(3), resolver))
      . assert(_ == List(t"\n  label  fluffy cat"))

      // Soft atoms before the first hard run keep their positional bindings: `+44` stays inline on
      // the compound while the hard atom moves to a child.
      test(m"A mixed soft-then-hard line expands only its hard tail"):
        val text = t"tel 1.0 $signature\n\nname Alice\nphone +44  020 7946 0958\n"
        editTexts(TelServer.codeActionsAt(uri, text, at(3), resolver))
      . assert(_ == List(t"\n  number  020 7946 0958"))

      test(m"Hard-gap expand and inline round-trip"):
        val original = t"tel 1.0 $signature\n\nname Alice\ncontact  fluffy cat\n"
        val expanded = applied(original, TelServer.codeActionsAt(uri, original, at(3), resolver))
        val inlined = applied(expanded, TelServer.codeActionsAt(uri, expanded, at(4), resolver))
        (expanded != original, inlined == original)
      . assert(_ == (true, true))

      // Appending to a line already in hard mode must use a hard gap, or the new atom would merge
      // into the phrase before it.
      test(m"Inlining onto a hard-mode parent uses a hard gap"):
        val text = t"tel 1.0 $signature\n\nname Alice\nphone  +44\n  number 0207946\n"
        applied(text, TelServer.codeActionsAt(uri, text, at(4), resolver))
      . assert(_ == t"tel 1.0 $signature\n\nname Alice\nphone  +44  0207946\n")

    suite(m"Pragma code actions and completions"):
      val uri = t"file:///test.tel"
      def at(line: Int): Lsp.Range = Lsp.Range(Lsp.Position(line, 0), Lsp.Position(line, 0))

      def edits(actions: List[Lsp.CodeAction]): List[(Int, Text)] =
        actions.flatMap(editsOf).map(edit => (edit.range.start.character, edit.newText))

      test(m"A reference-only pragma offers to append the schema signature"):
        edits(TelServer.codeActionsAt(uri, document(t"example.org/contact"), at(0), resolver))
      . assert(_ == List((27, t" $signature")))

      test(m"The signature goes after the layer selections and before the sigil"):
        val text = t"tel 1.0 example.org/themed +extras %\n\ntitle Hello\n"
        edits(TelServer.codeActionsAt(uri, text, at(0), resolver))
      . assert(_ == List((35, t"$themedExtrasSignature ")))

      test(m"A pragma already carrying a signature offers no action"):
        TelServer.codeActionsAt(uri, document(signature), at(0), resolver).size
      . assert(_ == 0)

      test(m"The pragma's layer slot offers the schema's unselected layers"):
        TelServer.completions(t"tel 1.0 example.org/themed +extras \n", Lsp.Position(0, 35), resolver)
        . items.map(_.label)
      . assert(_ == List(t"+footer"))

      test(m"Hovering the pragma shows the composed signature"):
        TelServer.hoverAt(t"tel 1.0 example.org/themed +extras\n", Lsp.Position(0, 12), resolver)
      . assert(_.let(_.contents.value.contains(themedExtrasSignature)).or(false))

    suite(m"Validation reports"):
      // Diagnostics for a document, in report order.
      def problems(text: Text): List[Lsp.Diagnostic] =
        TelServer.diagnose(text, resolver).order: diagnostic =>
          (diagnostic.range.start.line, diagnostic.range.start.character)

      test(m"The LLM report is one header plus one line per problem, 1-based, unquoted"):
        val text = keyedDocument(keyedSignature, t"amy")
        TelServer.llmReport(t"pets.tel", problems(text))
      . assert:
          case List(header, problem) =>
            header.starts(t"pets.tel: 1 error")
            && problem.starts(t"line 3, columns 1-3 [E314] error: ")
            && !problem.contains(t"pet amy")

          case _ => false

      test(m"The LLM report on a clean document says so"):
        TelServer.llmReport(t"pets.tel", problems(keyedDocument(keyedSignature, t"cat")))
      . assert(_ == List(t"pets.tel: no problems"))

      test(m"The human report quotes the offending line with carets beneath the span"):
        val text = keyedDocument(keyedSignature, t"amy")
        TelServer.humanReport(t"pets.tel", TelServer.linesOf(text), problems(text)).map(_.plain)
      . assert: report =>
          report.exists(_.contains(t"pet amy"))
          && report.exists(_.contains(t"^"))
          && report.last == t"pets.tel: 1 error"

      test(m"The human report on a clean document reports success"):
        val text = keyedDocument(keyedSignature, t"cat")
        TelServer.humanReport(t"pets.tel", TelServer.linesOf(text), problems(text)).map(_.plain)
      . assert(_ == List(t"✓ pets.tel: no problems found"))

    suite(m"Schema coherence"):
      def schemaDoc(body: Text): Text = t"tel 1.0 specification.tel/tels:2.1.0\n\n$body"

      test(m"An unresolved type reference is E209, located on the TypeName atom"):
        val text = schemaDoc(t"name x\n\ndocument\n  field foo Widget\n")
        TelServer.diagnose(text, resolver)
        . map(d => (d.code.or(t"?"), d.range.start.line, d.range.start.character))
      . assert(_ == List((t"E209", 5, 12)))

      test(m"A field referencing a select is E217, not the SelectRef form"):
        val text = schemaDoc:
          t"name x\n\nselect Status\n  variant on Flag\n  variant off Flag\n\ndocument\n  field s Status\n"
        TelServer.diagnose(text, resolver).map(_.code.or(t"?"))
      . assert(_ == List(t"E217"))

      test(m"A default on an optional member is E203 (the §20.1 battery is wired in)"):
        val text = schemaDoc(t"name x\n\ndocument\n  field foo String optional fallback\n")
        TelServer.diagnose(text, resolver).map(_.code.or(t"?"))
      . assert(_ == List(t"E203"))

      test(m"A coherent schema stays diagnostic-free"):
        TelServer.diagnose(keyedSchemaText, resolver).size
      . assert(_ == 0)

      test(m"`schema add` rejects an incoherent schema and leaves no entry"):
        val directory = scratch(t"tel-add-test")

        val file = store
          ( directory, t"dangling.tel",
            schemaDoc(t"name dangling\n\ndocument\n  field foo Widget\n") )

        val registry = t"${directory.encode}/registry".as[Path on Linux]
        val added = safely(SchemaCache.add(registry, file))
        val stored = t"${registry.encode}/dangling.tel".as[Path on Linux].existent()
        (added.absent, stored)
      . assert(_ == (true, false))

    suite(m"LIRA references and layers"):
      test(m"A LIRA reference resolves by its module-name tail"):
        resolver(t"example.org/contact")
      . assert:
          case TelServer.Resolution.Resolved(entry, _, _, _, _) => entry.name == t"contact"
          case _                                          => false

      test(m"A selector-form reference also resolves locally"):
        resolver(t"example.org/contact:1.2.0")
      . assert:
          case TelServer.Resolution.Resolved(entry, _, _, _, _) => entry.name == t"contact"
          case _                                          => false

      test(m"The pinned tels coordinate resolves to the meta-schema"):
        resolver(t"specification.tel/tels:2.1.0")
      . assert:
          case TelServer.Resolution.Meta(_, _) => true
          case _                               => false

      test(m"A selected layer's members validate"):
        TelServer.diagnose
          ( t"tel 1.0 example.org/themed +extras\n\ntitle Hello\nsubtitle World\n", resolver )
        . count(_.severity == Lsp.DiagnosticSeverity.Error)
      . assert(_ == 0)

      test(m"Without the layer selection, the layer's members are inadmissible"):
        TelServer.diagnose(t"tel 1.0 example.org/themed\n\ntitle Hello\nsubtitle World\n", resolver)
        . count(_.severity == Lsp.DiagnosticSeverity.Error)
      . assert(_ > 0)

      // §8.1: an undeclared layer name is a runtime resolution error, which carries no E-code;
      // only a selection out of declaration order is E124.
      test(m"An unknown layer selection is a resolution error, not E124"):
        TelServer.diagnose(t"tel 1.0 example.org/themed +bogus\n\ntitle Hello\n", resolver)
        . map(diagnostic => (diagnostic.code, diagnostic.severity))
      . assert(_ == List((t"layer-unknown", Lsp.DiagnosticSeverity.Error)))

      test(m"A layer selection out of declaration order is E124"):
        TelServer.diagnose(t"tel 1.0 example.org/themed +footer +extras\n\ntitle Hello\n", resolver)
        . map(_.code)
      . assert(_ == List(t"E124"))

      test(m"A signature naming a non-prefix subset of the layers resolves by decoding"):
        resolver(themedFooterSignature)
      . assert:
          case TelServer.Resolution.Resolved(entry, _, _, layers, signature) =>
            entry.name == t"themed" && layers == List(t"footer")
            && signature == themedFooterSignature
          case _ => false

      test(m"A decoded signature composes exactly the layers it names"):
        TelServer.diagnose
          ( t"tel 1.0 $themedFooterSignature\n\ntitle Hello\nfooter Bye\n", resolver )
        . count(_.severity == Lsp.DiagnosticSeverity.Error)
      . assert(_ == 0)

      test(m"A decoded signature admits no member of a layer it does not name"):
        TelServer.diagnose
          ( t"tel 1.0 $themedFooterSignature\n\ntitle Hello\nsubtitle World\n", resolver )
        . count(_.severity == Lsp.DiagnosticSeverity.Error)
      . assert(_ > 0)

      test(m"A signature that agrees with the layer selections resolves"):
        TelServer.diagnose
          ( t"tel 1.0 example.org/themed +extras $themedExtrasSignature\n\ntitle Hello\n",
            resolver )
        . size
      . assert(_ == 0)

      test(m"A signature that disagrees with the layer selections is reported"):
        TelServer.diagnose
          ( t"tel 1.0 example.org/themed +extras $themedFooterSignature\n\ntitle Hello\n",
            resolver )
        . map(_.code)
      . assert(_ == List(t"signature-disagrees"))

      test(m"A signature belonging to another schema than the reference is reported"):
        TelServer.diagnose(t"tel 1.0 example.org/themed $signature\n\ntitle Hello\n", resolver)
        . map(_.code)
      . assert(_.has(t"signature-disagrees"))

    suite(m"Outline"):
      def kinds(text: Text): Map[Text, Lsp.SymbolKind] =
        def walk(symbols: List[Lsp.DocumentSymbol]): List[(Text, Lsp.SymbolKind)] =
          symbols.flatMap(symbol => (symbol.name, symbol.kind) :: walk(symbol.children.or(Nil)))

        walk(TelServer.outline(text, resolver)).to[Map]

      test(m"The outline follows the document structure"):
        TelServer.outline(document(signature), resolver).map(_.name)
      . assert(_ == List(t"name", t"email", t"contact"))

      test(m"A resolved document's outline is classified by the schema"):
        kinds(document(signature))
      . assert: byName =>
          byName.at(t"contact") == Lsp.SymbolKind.Object
          && byName.at(t"name") == Lsp.SymbolKind.String

      test(m"A schema document's outline uses declaration kinds"):
        kinds(schemaText)
      . assert: byName =>
          byName.at(t"record") == Lsp.SymbolKind.Struct
          && byName.at(t"select") == Lsp.SymbolKind.Enum
          && byName.at(t"document") == Lsp.SymbolKind.Module

    suite(m"Hover"):
      test(m"Hovering a field keyword shows its schema declaration"):
        TelServer.hoverAt(document(signature), Lsp.Position(3, 2), resolver)
      . assert(_.let(_.contents.value.contains(t"**email**")).or(false))

      test(m"Hovering a field keyword shows the schema's description"):
        TelServer.hoverAt(document(signature), Lsp.Position(3, 2), resolver)
      . assert(_.let(_.contents.value.contains(t"primary email address")).or(false))

      test(m"Hovering a select-typed field's value shows the variant"):
        TelServer.hoverAt(document(signature), Lsp.Position(4, 8), resolver)
      . assert(_.let { hover => hover.contents.value.contains(t"variant") }.or(false))

      test(m"Hovering the pragma of a resolved document names the schema"):
        TelServer.hoverAt(document(signature), Lsp.Position(0, 2), resolver)
      . assert(_.let(_.contents.value.contains(t"contact")).or(false))

      test(m"Hovering the pragma of an unresolved document explains the failure"):
        TelServer.hoverAt(document(t"example.org/no-such-schema"), Lsp.Position(0, 2), resolver)
      . assert(_.let(_.contents.value.contains(t"not registered")).or(false))

      test(m"Hovering a built-in validator name shows its blurb"):
        val schemaDocument =
          t"tel 1.0 specification.tel/tels:2.1.0\nname x\nscalar Foo\n  validate identifier\n"

        TelServer.hoverAt(schemaDocument, Lsp.Position(3, 12), resolver)
      . assert(_.let(_.contents.value.contains(t"built-in validator")).or(false))

    suite(m"Completion"):
      test(m"The keyword slot offers the enclosing struct's members"):
        TelServer.completions(document(signature) + t"\n", Lsp.Position(6, 0), resolver)
        . items.map(_.label)
      . assert { labels => labels.has(t"email") && labels.has(t"phone") }

      test(m"A select-typed field's value slot offers its variants"):
        TelServer.completions(document(signature), Lsp.Position(4, 7), resolver)
        . items.map(_.label)
      . assert { labels => labels.has(t"active") && labels.has(t"archived") }

      test(m"The pragma's identifier slot offers registered schemas by signature"):
        TelServer.completions(t"tel 1.0 \n", Lsp.Position(0, 8), resolver).items
      . assert(_.exists { item => item.label == t"contact" && item.insertText == signature })

      test(m"A member declaration in a schema document completes its flags"):
        val schemaDocument =
          t"tel 1.0 specification.tel/tels:2.1.0\nname x\nrecord Foo\n  field name Identifier \n"

        TelServer.completions(schemaDocument, Lsp.Position(3, 24), resolver)
        . items.map(_.label)
      . assert: labels =>
          labels.has(t"optional") && labels.has(t"irrepeatable")
          && !labels.has(t"keyword")

      test(m"A validate line completes the built-in validators"):
        val schemaDocument =
          t"tel 1.0 specification.tel/tels:2.1.0\nname x\nscalar Foo\n  validate \n"

        TelServer.completions(schemaDocument, Lsp.Position(3, 11), resolver)
        . items.map(_.label)
      . assert { labels => labels.has(t"identifier") && labels.has(t"type-name") }

      test(m"The type-name slot in a schema document offers definitions and built-ins"):
        val schemaDocument =
          t"tel 1.0 specification.tel/tels:2.1.0\nname x\nrecord Foo\ndocument\n  field a \n"

        TelServer.completions(schemaDocument, Lsp.Position(4, 10), resolver)
        . items.map(_.label)
      . assert { labels => labels.has(t"Foo") && labels.has(t"String") }
