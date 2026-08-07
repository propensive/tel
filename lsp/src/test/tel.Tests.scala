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
  val schemaText: Text = """tel 1.0 https://tel-lang.org/schema/tel-schema

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

    val registry: TelServer.Registry = t"${registryDir.toString}".as[Path on Linux]
    val resolver = TelServer.SchemaResolver(registry)
    val schemaFile = t"${registryDir.toString}/contact.tel".as[Path on Linux]
    val signature: Text = SchemaCache.describe(schemaFile).let(_.id).or(t"")

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
        resolver(t"https://tel-lang.org/schema/tel-schema")
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

      test(m"A spurious E306 on scalar `encoding` is downgraded to a warning"):
        val encodingSchema = List
          ( t"tel 1.0 https://tel-lang.org/schema/tel-schema", t"", t"name enc", t"",
            t"scalar Code", t"  validate string", t"  encoding hex-bytes", t"",
            t"document", t"  field code Code" )
        . join(t"\n")

        TelServer.diagnose(encodingSchema, resolver).stdlib.filter(_.code == t"E306")
      . assert: diagnostics =>
          diagnostics.nonEmpty && diagnostics.forall: diagnostic =>
            diagnostic.severity == Lsp.DiagnosticSeverity.Warning
            && diagnostic.range.start.line == 6

      test(m"A duplicate definition (E210) is located on the second declaration"):
        val duplicated = List
          ( t"tel 1.0 https://tel-lang.org/schema/tel-schema", t"", t"name dup", t"",
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
          t"tel 1.0 https://tel-lang.org/schema/tel-schema\nname x\nscalar Foo\n  validate identifier\n"

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
          t"tel 1.0 https://tel-lang.org/schema/tel-schema\nname x\nrecord Foo\n  field name Identifier \n"

        TelServer.completions(schemaDocument, Lsp.Position(3, 24), resolver)
        . items.stdlib.map(_.label)
      . assert: labels =>
          labels.contains(t"optional") && labels.contains(t"irrepeatable")
          && !labels.contains(t"keyword")

      test(m"A validate line completes the built-in validators"):
        val schemaDocument =
          t"tel 1.0 https://tel-lang.org/schema/tel-schema\nname x\nscalar Foo\n  validate \n"

        TelServer.completions(schemaDocument, Lsp.Position(3, 11), resolver)
        . items.stdlib.map(_.label)
      . assert { labels => labels.contains(t"identifier") && labels.contains(t"type-name") }

      test(m"The type-name slot in a schema document offers definitions and built-ins"):
        val schemaDocument =
          t"tel 1.0 https://tel-lang.org/schema/tel-schema\nname x\nrecord Foo\ndocument\n  field a \n"

        TelServer.completions(schemaDocument, Lsp.Position(4, 10), resolver)
        . items.stdlib.map(_.label)
      . assert { labels => labels.contains(t"Foo") && labels.contains(t"String") }
