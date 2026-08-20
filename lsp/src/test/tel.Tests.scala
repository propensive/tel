package tel

import soundness.*

// See the note in `tel.TelServer.scala`: the collection types come straight from Proscenium.
import proscenium.{List, Nil}

import strategies.throwUnsafely
import interfaces.paths.pathOnLinux

// Tests for the LSP server's pure handler functions: no JSON-RPC transport is involved, and the
// schema registry is a throwaway temporary directory, so the suite exercises exactly the logic the
// handlers close over. Run with `mill tel.test.run`.
object Tests extends Suite(m"TEL LSP server tests"):

  // A small contact schema, registered under `contact.tel` in the temporary registry.
  val schemaText: Text = """tel 1.0 https://tel-lang.org/schema/tels

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
  field phone PhoneNumber optional repeatable
  field contact Contact optional
  select Status optional
""".tt

  // A well-formed document conforming to the contact schema.
  def document(identifier: Text): Text =
    t"tel 1.0 $identifier\n\nname Alice\nemail alice@example.org\ncontact active\n"

  // A schema whose repeatable `pet` member is typed by a record with a `key` field (§20), so that
  // sibling pets must have distinct names (§21.6, E314).
  val keyedSchemaText: Text = """tel 1.0 https://tel-lang.org/schema/tels

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
  val flagsSchemaText: Text = """tel 1.0 https://tel-lang.org/schema/tels

name flags

record Widget
  field visible Flag
  field label String optional

document
  field widget Widget optional repeatable
""".tt

  // A document with source-atom and literal-atom payloads (§14, §15): the payload lines must not
  // be mistaken for compounds by the structure scan.
  val atomForms: Text = """tel 1.0 https://example.org/documents-by-form

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

  def run(): Unit =
    val registryDir = java.nio.file.Files.createTempDirectory("tel-lsp-tests").nn

    java.nio.file.Files.write
      ( registryDir.resolve("contact.tel").nn, schemaText.s.getBytes("UTF-8").nn )

    java.nio.file.Files.write
      ( registryDir.resolve("keyed.tel").nn, keyedSchemaText.s.getBytes("UTF-8").nn )

    java.nio.file.Files.write
      ( registryDir.resolve("flags.tel").nn, flagsSchemaText.s.getBytes("UTF-8").nn )

    val registry: TelServer.Registry = t"${registryDir.toString}".as[Path on Linux]
    val resolver = TelServer.SchemaResolver(registry)
    val schemaFile = t"${registryDir.toString}/contact.tel".as[Path on Linux]
    val signature: Text = SchemaCache.describe(schemaFile).let(_.id).or(t"")
    val keyedFile = t"${registryDir.toString}/keyed.tel".as[Path on Linux]
    val keyedSignature: Text = SchemaCache.describe(keyedFile).let(_.id).or(t"")
    val flagsFile = t"${registryDir.toString}/flags.tel".as[Path on Linux]
    val flagsSignature: Text = SchemaCache.describe(flagsFile).let(_.id).or(t"")

    suite(m"Schema resolution"):
      test(m"The registered schema's signature resolves"):
        resolver(signature)
      . assert:
          case TelServer.Resolution.Resolved(entry, _, _) => entry.name == t"contact"
          case _                                          => false

      test(m"An unknown identifier is Unresolved"):
        resolver(t"NoSuchSchema")
      . assert:
          case TelServer.Resolution.Unresolved(_) => true
          case _                                  => false

      test(m"A tel-schema pragma resolves to the meta-schema"):
        resolver(t"https://tel-lang.org/schema/tels")
      . assert:
          case TelServer.Resolution.Meta(_, _) => true
          case _                               => false

    suite(m"Diagnostics"):
      test(m"A valid document yields no error diagnostics"):
        TelServer.diagnose(document(signature), resolver).stdlib
        . count(_.severity == Lsp.DiagnosticSeverity.Error)
      . assert(_ == 0)

      test(m"A schema violation yields an error diagnostic"):
        TelServer.diagnose(document(signature) + t"bogus value\n", resolver).stdlib
        . count(_.severity == Lsp.DiagnosticSeverity.Error)
      . assert(_ > 0)

      test(m"An unresolved schema yields an information diagnostic on the identifier"):
        TelServer.diagnose(document(t"NoSuchSchema"), resolver).stdlib
        . filter(_.code == t"schema-unresolved")
      . assert: diagnostics =>
          diagnostics.length == 1 && diagnostics.forall: diagnostic =>
            diagnostic.severity == Lsp.DiagnosticSeverity.Information
            && diagnostic.range.start.line == 0 && diagnostic.range.start.character == 8

      test(m"A resolved schema yields no unresolved-schema diagnostic"):
        TelServer.diagnose(document(signature), resolver).stdlib
        . count(_.code == t"schema-unresolved")
      . assert(_ == 0)

      // Stratiform's schema reconstruction understands scalar `encoding` (§21.7), so a schema
      // declaring one is simply valid — this used to raise a spurious E306 downgraded to a warning.
      test(m"A scalar `encoding` declaration is accepted"):
        val encodingSchema = List
          ( t"tel 1.0 https://tel-lang.org/schema/tels", t"", t"name enc", t"",
            t"scalar Code", t"  validate string", t"  encoding hex-bytes", t"",
            t"document", t"  field code Code" )
        . join(t"\n")

        TelServer.diagnose(encodingSchema, resolver).stdlib
        . count(_.severity == Lsp.DiagnosticSeverity.Error)
      . assert(_ == 0)

      // A `key` field (§20) is likewise part of the schema language now: the declaration is valid,
      // and duplicate key values among repeatable siblings are E314 (§21.6).
      test(m"A `key` field declaration is accepted"):
        TelServer.diagnose(keyedSchemaText, resolver).stdlib
        . count(_.severity == Lsp.DiagnosticSeverity.Error)
      . assert(_ == 0)

      test(m"Duplicate key values raise E314"):
        TelServer.diagnose(keyedDocument(keyedSignature, t"amy"), resolver).stdlib
        . filter(_.code == t"E314")
      . assert(_.length == 1)

      test(m"Distinct key values raise no E314"):
        TelServer.diagnose(keyedDocument(keyedSignature, t"cat"), resolver).stdlib
        . count(_.code == t"E314")
      . assert(_ == 0)

      test(m"A duplicate definition (E210) is located on the second declaration"):
        val duplicated = List
          ( t"tel 1.0 https://tel-lang.org/schema/tels", t"", t"name dup", t"",
            t"record Foo", t"  field a String", t"", t"record Foo", t"  field b String", t"",
            t"document", t"  field foo Foo optional" )
        . join(t"\n")

        TelServer.diagnose(duplicated, resolver).stdlib.filter(_.code == t"E210")
      . assert: diagnostics =>
          diagnostics.length == 1 && diagnostics.forall(_.range.start.line == 7)

    suite(m"Structure scan"):
      test(m"Payload lines are not compounds"):
        TelServer.flatten(TelServer.structure(atomForms)(1)).map(_.keyword).stdlib
      . assert(_ == scala.List(t"inline-value", t"source-value", t"literal-value"))

      test(m"A source-atom payload extends its compound's endLine"):
        TelServer.flatten(TelServer.structure(atomForms)(1)).stdlib
        . find(_.keyword == t"source-value").map(_.endLine)
      . assert(_ == Some(6))

      test(m"A literal-atom payload extends to its closing delimiter"):
        TelServer.flatten(TelServer.structure(atomForms)(1)).stdlib
        . find(_.keyword == t"literal-value").map(_.endLine)
      . assert(_ == Some(12))

    suite(m"Code actions"):
      val uri = t"file:///test.tel"
      def at(line: Int): Lsp.Range = Lsp.Range(Lsp.Position(line, 0), Lsp.Position(line, 0))

      // The `newText` of every edit an action carries.
      def editTexts(actions: List[Lsp.CodeAction]): scala.List[String] =
        actions.stdlib.flatMap: action =>
          action.edit.let(_.changes).lay(scala.List[Lsp.TextEdit]()): changes =>
            changes.stdlib.values.to(scala.List).flatMap(_.stdlib)
        . map(_.newText.s)

      // Apply every edit of the first action, back-to-front so earlier offsets stay valid.
      def applied(text: Text, actions: List[Lsp.CodeAction]): Text =
        actions.stdlib.headOption.flatMap(_.edit.let(_.changes).option).fold(text): changes =>
          val edits = changes.stdlib.values.to(scala.List).flatMap(_.stdlib)
          val ls = text.s.split("\n", -1).nn.toIndexedSeq.map(_.nn)
          def offset(p: Lsp.Position): Int = ls.take(p.line).map(_.length + 1).sum + p.character
          edits.sortBy(edit => offset(edit.range.start)).reverse.foldLeft(text.s): (acc, edit) =>
            acc.substring(0, offset(edit.range.start)).nn + edit.newText.s
            + acc.substring(offset(edit.range.end)).nn
          . tt

      // `pet amy` (line 2): `amy` binds positionally to Pet's required `name` field, so it can move
      // to an explicit `name amy` child line.
      test(m"Expanding the last atom of a record-typed compound is offered"):
        TelServer.codeActionsAt(uri, keyedDocument(keyedSignature, t"cat"), at(2), resolver)
        . stdlib.map(_.title.s)
      . assert(_ == scala.List("Move `amy` to a child compound line"))

      test(m"The expansion edit writes the member keyword on a child line"):
        editTexts(TelServer.codeActionsAt(uri, keyedDocument(keyedSignature, t"cat"), at(2), resolver))
      . assert(_ == scala.List("\n  name amy"))

      test(m"The applied edit leaves a diagnostic-free document"):
        val text = keyedDocument(keyedSignature, t"cat")
        val actions = TelServer.codeActionsAt(uri, text, at(2), resolver)
        TelServer.diagnose(applied(text, actions), resolver).stdlib
        . count(_.severity == Lsp.DiagnosticSeverity.Error)
      . assert(_ == 0)

      // §20.8: in `contact active`, the atom binds to `label` — an optional Scalar member is never
      // skipped — NOT to the Status select. The expansion makes that binding explicit, which is
      // half the point of the refactoring.
      test(m"Expansion names the member the atom actually binds to"):
        editTexts(TelServer.codeActionsAt(uri, document(signature), at(4), resolver))
      . assert(_ == scala.List("\n  label active"))

      // `name Alice`: the compound's own type is a Scalar, so its atom is its value, not a member
      // binding — there is nothing to expand.
      test(m"A scalar-typed compound's value atom offers no action"):
        TelServer.codeActionsAt(uri, document(signature), at(2), resolver).stdlib.length
      . assert(_ == 0)

      test(m"No actions without a resolved schema"):
        TelServer.codeActionsAt(uri, document(t"NoSuchSchema"), at(2), resolver).stdlib.length
      . assert(_ == 0)

      test(m"A remark on the line declines the action"):
        val text = t"tel 1.0 $keyedSignature\n\npet amy # first pet\n"
        TelServer.codeActionsAt(uri, text, at(2), resolver).stdlib.length
      . assert(_ == 0)

      // ── The opposite direction: inlining a child compound as an atom ──

      // The keyed document with `pet amy` already in expanded form.
      def expandedKeyed(second: Text): Text =
        t"tel 1.0 $keyedSignature\n\npet\n  name amy\npet $second\n"

      test(m"Inlining a scalar-typed first child is offered"):
        TelServer.codeActionsAt(uri, expandedKeyed(t"cat"), at(3), resolver)
        . stdlib.map(_.title.s)
      . assert(_ == scala.List("Inline `amy` onto the `pet` line"))

      test(m"Inlining is declined when the child is not on the line below its parent"):
        val text = t"tel 1.0 $keyedSignature\n\npet\n\n  name amy\npet cat\n"
        TelServer.codeActionsAt(uri, text, at(4), resolver).stdlib.length
      . assert(_ == 0)

      // Inlining `active` onto `contact` would produce `contact active`, whose atom binds to
      // `label` (an optional Scalar member is never skipped, §20.8) — NOT to the Status variant the
      // child names. The semantic-model comparison catches the re-binding and declines the action.
      test(m"Inlining is declined when the atom would re-bind to a different member"):
        val text = t"tel 1.0 $signature\n\nname Alice\ncontact\n  active\n"
        TelServer.codeActionsAt(uri, text, at(4), resolver).stdlib.length
      . assert(_ == 0)

      // In the flags schema, Widget's first member is the required Flag itself, so inlining is
      // meaning-preserving and offered.
      test(m"Inlining a flag-shaped child uses its keyword"):
        val text = t"tel 1.0 $flagsSignature\n\nwidget\n  visible\n"
        TelServer.codeActionsAt(uri, text, at(3), resolver)
        . stdlib.map(_.title.s)
      . assert(_ == scala.List("Inline `visible` onto the `widget` line"))

      test(m"Flag expand and inline round-trip"):
        val original = t"tel 1.0 $flagsSignature\n\nwidget visible\n"
        val expanded = applied(original, TelServer.codeActionsAt(uri, original, at(2), resolver))
        val inlined = applied(expanded, TelServer.codeActionsAt(uri, expanded, at(3), resolver))
        inlined.s == original.s
      . assert(_ == true)

      // Expansion then inlining must return to the original text: the two actions are inverses.
      test(m"Expand and inline round-trip"):
        val original = keyedDocument(keyedSignature, t"cat")
        val expanded = applied(original, TelServer.codeActionsAt(uri, original, at(2), resolver))
        val inlined = applied(expanded, TelServer.codeActionsAt(uri, expanded, at(3), resolver))
        inlined.s == original.s
      . assert(_ == true)

    suite(m"Hover"):
      test(m"Hovering a field keyword shows its schema declaration"):
        TelServer.hoverAt(document(signature), Lsp.Position(3, 2), resolver)
      . assert(_.let(_.contents.value.s.contains("**email**")).or(false))

      test(m"Hovering a select-typed field's value shows the variant"):
        TelServer.hoverAt(document(signature), Lsp.Position(4, 8), resolver)
      . assert(_.let { hover => hover.contents.value.s.contains("variant") }.or(false))

      test(m"Hovering the pragma of a resolved document names the schema"):
        TelServer.hoverAt(document(signature), Lsp.Position(0, 2), resolver)
      . assert(_.let(_.contents.value.s.contains("contact")).or(false))

      test(m"Hovering the pragma of an unresolved document explains the failure"):
        TelServer.hoverAt(document(t"NoSuchSchema"), Lsp.Position(0, 2), resolver)
      . assert(_.let(_.contents.value.s.contains("not registered")).or(false))

      test(m"Hovering a built-in validator name shows its blurb"):
        val schemaDocument =
          t"tel 1.0 https://tel-lang.org/schema/tels\nname x\nscalar Foo\n  validate identifier\n"

        TelServer.hoverAt(schemaDocument, Lsp.Position(3, 12), resolver)
      . assert(_.let(_.contents.value.s.contains("built-in validator")).or(false))

    suite(m"Completion"):
      test(m"The keyword slot offers the enclosing struct's members"):
        TelServer.completions(document(signature) + t"\n", Lsp.Position(6, 0), resolver)
        . items.stdlib.map(_.label)
      . assert { labels => labels.contains(t"email") && labels.contains(t"phone") }

      test(m"A select-typed field's value slot offers its variants"):
        TelServer.completions(document(signature), Lsp.Position(4, 7), resolver)
        . items.stdlib.map(_.label)
      . assert { labels => labels.contains(t"active") && labels.contains(t"archived") }

      test(m"The pragma's identifier slot offers registered schemas by signature"):
        TelServer.completions(t"tel 1.0 \n", Lsp.Position(0, 8), resolver).items.stdlib
      . assert(_.exists { item => item.label == t"contact" && item.insertText == signature })

      test(m"A member declaration in a schema document completes its flags"):
        val schemaDocument =
          t"tel 1.0 https://tel-lang.org/schema/tels\nname x\nrecord Foo\n  field name Identifier \n"

        TelServer.completions(schemaDocument, Lsp.Position(3, 24), resolver)
        . items.stdlib.map(_.label)
      . assert: labels =>
          labels.contains(t"optional") && labels.contains(t"irrepeatable")
          && !labels.contains(t"keyword")

      test(m"A validate line completes the built-in validators"):
        val schemaDocument =
          t"tel 1.0 https://tel-lang.org/schema/tels\nname x\nscalar Foo\n  validate \n"

        TelServer.completions(schemaDocument, Lsp.Position(3, 11), resolver)
        . items.stdlib.map(_.label)
      . assert { labels => labels.contains(t"identifier") && labels.contains(t"type-name") }

      test(m"The type-name slot in a schema document offers definitions and built-ins"):
        val schemaDocument =
          t"tel 1.0 https://tel-lang.org/schema/tels\nname x\nrecord Foo\ndocument\n  field a \n"

        TelServer.completions(schemaDocument, Lsp.Position(4, 10), resolver)
        . items.stdlib.map(_.label)
      . assert { labels => labels.contains(t"Foo") && labels.contains(t"String") }
