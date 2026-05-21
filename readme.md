<p align="center"><img src="/doc/logo.svg" height="300"></p>

# TEL, the Typed Element Language

TEL is a format for representing tree-structured data, designed for documents that may be edited by
both humans and computers. TEL keeps markup to a minimum: spaces and newlines define structure,
while `#` is the only other meaningful character, for starting a comment.

## Features

- Models tree-structured data
- Symbolic markup is minimal, making it more enjoyable to write
- Automatic modifications don't reformat a document
- Documents may be untyped or typed
- Allows embedded textual content (such as XML or JSON) without escaping
- Lightweight data schemas with simple syntax
- User-extensible data verification
- Fast and lightweight binary format (BinTEL)
- Safe schema evolution with compatibility checking
- Allows comments, which may be attached to data, or not
- Both data and schemas are composable

Here is an example of TEL being used to describe a contact:

```tel
name  Alice Anderson
email alice@example.org

phone
  country-code 44
  number 020-7946-0100

home
  street  221B Baker Street
  city London
  country UK

active
```

Note the two spaces after `name` and `street`: this is a *hard space*, which puts the rest of the
line into hard-space mode so that subsequent single spaces become part of the value rather than
separating atoms. Without it, `name Alice Anderson` would be three separate atoms (and would
violate a schema that expects a single string value); with it, the whole tail of the line is one
value.

The keywords here — `name`, `email`, `phone`, `home`, `active`, etc. — are not part of TEL itself
but are defined by a *schema*. Schemas are themselves TEL documents, written in the schema language
defined in §20 of the [TEL Specification](spec.md). A small schema for the document above might
look like:

```tel
tel 1.0

name contact-schema

document
  field name required
    scalar string
  field email
    scalar string
  field phone repeatable
    struct
      field country-code required
        scalar string
      field number required
        scalar string
  field home
    struct
      field street required
        scalar string
      field city required
        scalar string
      field country required
        scalar string
  select required
    variant active
      flag
    variant archived
      flag
```

The full worked example, with separate definitions for `address` and `phone-number` reused via
`type` references, is in [`examples/contact-schema.tel`](examples/contact-schema.tel) and
[`examples/contact-document.tel`](examples/contact-document.tel).

## Writing TEL

Writing TEL is easy. Each line contains some words, or data, which represent a node in the tree.
Each line is indented by a number of spaces. If the indentation is the same as the previous line,
then the node is a sibling or peer—it shares the same parent node. If it is indented two spaces more
than the previous line, then it is a child.

If the line has less indentation than the previous line, then it is the child of an earlier node:
the last one with two fewer spaces of indentation—exactly as the visual appearance implies.

Any other indentation, including having an odd number of spaces, is considered an error. There is
one exception to this for supporting multiline strings, which is explained below.

Each data line contains one or more words (or character sequences), separated by spaces. Any number
of spaces may appear between words without any semantic significance,

Blank lines may appear anywhere. This format has no significant punctuation other than whitespace,
and it should seem very natural to a human reader.

### Comments

A TEL document may contain human-readable comments. They contain no data, and their contents is not
interpreted but they are part of the TEL metamodel, and can only appear in certain places in a
document.

Comments always begin with a `#` character. The `#` must either start a line or be preceded by at
least one space, and must be followed by exactly one space (a "soft space") and then the comment
text.

For example, the line,

```tel
    email user@example.com     # The user's email address
```

would contain the data, `email user@example.com`, and the comment, `The user's email address`, but
the line,

```tel
    url https://example.com/page#ref
```

and,

```tel
  reference #foo
```

would not contain any comments.

A comment may also appear alone on a line, but the whitespace around it is significant: it must
exist at a valid indentation level, that is, preceded by an even number of spaces, and up to one
level higher than the previous line.

For example, like this,

```tel
usr
  local
    bin

      # This is a valid comment
```

or this,

```tel
usr
  local
    bin

  # This is a valid comment
```

but not this,

```tel
usr
  local
    bin

          # This is a valid comment
```

or this:

```tel
usr
  local
    bin

 # This is a valid comment
```

Comments are _attached_ to data nodes, and their attachment is determined by the whitespace around
them. Comments will attach to a data node if they appear on the line immediately preceding the data,
at the same level of indentation. If that node is deleted by a computer editor, the comment will be
deleted too.

Standalone comments may also be followed by blank line, in which case their are attached to the
parent node. Such comments will be retained even when data nodes around them are modified, but will
be removed if the parent node is deleted.

An uninterrupted sequence of comment lines at the same indentation level is treated as a single
comment.

There are two special rules relating to comments on the first line of a TEL document: if the first
line is a comment (one or more lines long), then it _must_ be followed by a blank line; and the
requirement that the `#` be followed by a space is relaxed _only_ for a comment on the first line of
the document.

These two exceptions facilitate the inclusion of a shebang line at the start of a document, such as,

```tel
#!/usr/bin/env processor

model
  data
```

### Multiline values

Sometimes it is necessary to write a value containing more than one line of text, or which contains
spaces, or the character sequence ` #`, without being considered a comment. This is possible using a
_double indent_: instead of writing a key and its value on the same line, such as,

```tel
dog
  name        Fido
  description furry
```

we can write:

```tel
dog
  name Fido
  description
      Furry, brown and cuddly.
```

A double-indented value continues so long as its indentation level is maintained. Thus, in,

```tel
dog
  name Fido
  description
      Furry, brown
      and cuddly.
```

the value of `description` would be, `Furry, brown\nand cuddly`: the newline character (`\n`) is
part of the value, but the six spaces of indentation are not. Nevertheless, additional spaces may be
included:

```tel
  description
      Furry, brown
       and cuddly
```

would be interpreted as a `Furry, brown\n and cuddly`, but any subsequent line with less indentation
would terminate the multiline value, and be interpreted as new data.

This is particularly useful for embedding other languages in TEL. For example,

```tel
data
  representations
    json
        { "name": "Fido", "description": "furry" }

    xml
        <dog>
          <name>Fido</name>
          <description>furry</description>
        </dog>

    markdown
        # Dog

        *Fido* is a furry dog.
```

Note, in particular, that since the markdown value, `markdown`, is indented as a multiline value,
`# Dog` is not interpreted as a comment.

### Embedded form

When embedding TEL data in a host language, we often want to add additional indentation so that the
embedded TEL aligns with the surrounding code. When a TEL document is parsed, the indentation of the
first line containing data is noted, and subtracted from all subsequent lines.

For example, in Scala we might write,

```scala
object Data:
  val animal: String = """
    Animal dog
      name Fido
      legs 4
      tail yes
  """
```

It is therefore also possible to interpret any contiguous fragment of a TEL document provided no
line contains less indentation than the first line of data.

From the example at the top of the page, the fragment,

```tel
  module alpha
    name         Alpha
    description  This is a description
```

is itself a valid TEL document.

## Binary form (BinTEL)

TEL can be serialised to a compact binary form for fast reading and writing. This is called BinTEL
and is defined in the [BinTEL Specification](bintel-spec.md). BinTEL is a byte sequence beginning
with the magic number `C0 D1`, followed by a schema signature identifying the document's schema,
followed by the encoded document.

BinTEL can be carried over text-oriented channels (embedded in another TEL document, displayed in
a terminal, copy-pasted between applications) by encoding it as Unicode text using
[BASE-256](base256-spec.md) — a one-character-per-byte encoding whose alphabet is chosen so that
the result is a single word for the purposes of double-click word selection.

## Schemas

A TEL schema is itself a TEL document, conforming to the *tel-schema* (the schema-for-schemas).
Schemas use a small vocabulary — `define`, `field`, `select`, `variant`, `struct`, `scalar`,
`flag`, `type` — and describe the structure that conforming documents must match. The full
vocabulary and validation rules are in [§20 of the TEL Specification](spec.md). A worked example
is in [`examples/`](examples/).

## Status

This is a specification draft. Three reference implementations live in this repo:

- `src/lib.rs` — TEL parser, schema model, validity checker, type-assignment algorithm, and
  built-in tel-schema. Used by the `cargo test` suite (300+ `.tel`-based tests plus unit tests
  for the schema layer).
- `src/base256.rs` — BASE-256 codec.
- `palimpsest/src/lib.rs` — Palimpsest codec (used for schema signatures in BinTEL).
