# TELP Specification Draft

## Abstract

TELP (**TEL Path**) is a textual path language for addressing elements of the **semantic
model** of a schema-typed TEL document, analogous in purpose to XPath for XML or JSON Pointer for JSON. A
TELP resolves, against a composed schema, to a single semantic element, to one keyword's
ordered occurrence sequence, or to a failure. Components are matched by **keyword**; occurrences
of repeatable members are selected by **key value** (an identity declared in the schema via the
`key` flag, §20 of the [TEL Specification](tel.md)) or by **zero-based occurrence index**.

TELP is a companion to the [TEL Specification](tel.md); section references of the form
"TEL §n" refer to it. TELP is a **query mechanism**: it performs no mutation, defines no
document error codes, and does not extend or alter the machine-operation addressing of TEL
§22.2.

The design follows two TEL principles:

- **No escaping.** A path's component delimiter is chosen per path — the first character of the
  path — so a component containing one candidate delimiter is written using another. There is no
  escape syntax.
- **Stability under reformatting.** A TELP never mentions blocks, lines, blank lines, or atom
  presentation forms. Reformatting a document — moving an occurrence between inline-atom and
  compound-child form, splitting sibling groups with blank lines, re-tabulating — does not
  change what any TELP resolves to, in contrast to the positional presentation-layer paths of
  TEL §22.2, which such edits invalidate (TEL §22.5).

## 1. Status

This document is a draft specification of TELP.

## 2. Conformance Language

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHALL**, **SHALL NOT**, **SHOULD**,
**SHOULD NOT**, **RECOMMENDED**, **MAY**, and **OPTIONAL** in this document are to be
interpreted as described in RFC 2119 and RFC 8174 when, and only when, they appear in all
capitals.

## 3. Grammar

A TELP is a non-empty string:

```ebnf
telp    ::= delimiter , [ component , { delimiter , component } ] ;
component  ::= character , { character } ;      (* non-empty *)
character  ::= ? any Unicode code point other than the path's delimiter,
                 U+000A LINE FEED, and U+000D CARRIAGE RETURN ? ;
delimiter  ::= "!" | '"' | "#" | "$" | "%" | "&" | "*" | "+" | ","
             | "." | "/" | ":" | ";" | "=" | "?" | "@" | "\" | "^"
             | "_" | "`" | "|" | "~" ;
```

**The delimiter is the first character of the path**, and that single occurrence at the start
both selects and introduces it: the remainder of the path is split into components at every
further occurrence of that character. The delimiter set is the sigil-valid set of TEL §6
**minus `-` and `'`** — the two sigil-valid characters that may appear inside a kebab-case
identifier (TEL §20.7) — leaving the twenty-two characters enumerated above. Excluding `-` and
`'` guarantees that every schema-declarable keyword can appear as a component under every
delimiter. `.` and `/` are RECOMMENDED as conventional choices.

Components are non-empty: an empty component — a doubled delimiter, or a trailing delimiter at
the end of the path — is a syntax error, with one exception: a path consisting of the delimiter
alone (`/`, `.`, …) is the **root path**, denoting the context element itself.

A path that does not begin with a character from the delimiter set, or that contains `LF` or
`CR`, is a syntax error.

**Path identity.** The delimiter is not part of a path's identity: `.foo.bar` and `/foo/bar`
are the same path. Two TELPs are equal iff their component sequences are equal as sequences
of code-point strings.

## 4. Resolution

Resolution takes three inputs:

- a **composed schema** (TEL §20; layer composition per TEL §20.3),
- a **context element** — a semantic-model `Node` (TEL §18.2), by default the document root,
- a TELP.

and produces one of three outcomes:

- an **element** (a `Node` or `Value`, TEL §18.2),
- an **occurrence sequence** — the ordered, possibly empty sequence of children of one parent
  bearing one keyword, or
- a **failure** (§7).

Resolution is defined over the semantic model, so it requires a schema (TEL §8.2: an untyped
document has no semantic model and no TELP resolution). Wherever this section asks about a
type, the question is asked after reference resolution (TEL §20.2).

Resolution processes components left to right, maintaining a current node `current`
(initially the context element) and, transiently, a **pending keyword** when the previous
component matched a repeatable member. For each component C:

1. **Keyword step** (no keyword pending). `current` MUST be a `Struct`-typed `Node`;
   otherwise resolution fails (*non-struct descent* — descending below a `Value` or a
   `Flag`-typed node is impossible). C is matched against the keyword order of `current`'s
   type (TEL §20): the `Field` keywords and the `Variant` keywords of `SelectRef`-referenced
   `SelectDefinition`s alike. An all-digit component matches no keyword (a kebab-case
   identifier is never all-digits, TEL §20.7). If C matches no keyword, resolution fails
   (*unknown keyword*).

   Let M be the matched member.

   - If M is effectively `repeatable`, C becomes the pending keyword. If the path ends here,
     the result is the **occurrence sequence** for C: the children of `current` bearing
     keyword C, in semantic order (TEL §18.3: atoms before compound children) — possibly
     empty.
   - Otherwise (M non-repeatable), resolve to the child of `current` bearing keyword C. A
     required member is always present (a required-with-default member is filled by its
     default `Value`, TEL §18.3 step 5); an absent optional member fails (*absent member*).
     If the path ends here, that child is the result; otherwise it becomes `current` and
     resolution continues.

2. **Selector step** (keyword K pending). Let S be the sequence of children of `current`
   bearing keyword K, in semantic order.

   - **Index selector.** If C consists solely of ASCII digits, it is read as a decimal,
     zero-based index into S. An index ≥ len(S) fails (*index out of range*). Leading zeros
     are permitted (`007` is index 7); writers SHOULD omit them.
   - **Key selector.** Otherwise, K's type MUST be a `Struct` whose composed members include
     a key-flagged field (TEL §20); if not, resolution fails (*type not keyed*). C selects
     the first element of S whose **key value** (TEL §21.6, including a default-supplied
     value) equals C exactly, code point for code point. If none matches, resolution fails
     (*key not found*).

   The selected element becomes the result (if the path ends) or `current`.

A path can never descend through a repeatable member without a selector: the component after a
repeatable keyword is always consumed as a selector, never tried as a keyword.

**Non-normative note (position decides interpretation).** A component's reading is determined
entirely by its position relative to the schema, never by comparing it against both the keyword
and key-value namespaces. In `/server/db/port`, once the schema says `server` is repeatable,
`db` is a selector — even if the server record also declares a field whose keyword is `db` —
and `port` is then a keyword of the selected server. A key value equal to a keyword therefore
never creates ambiguity, though it can mislead a human reader skimming a path without the
schema; key values matching keywords of their own record are legal but SHOULD be avoided for
readability.

**Non-normative note (invalid documents).** Key selection is primarily meaningful for documents
free of E314 (TEL §21.6). Against a document carrying duplicate key values, the
first-in-semantic-order rule above still makes resolution deterministic.

## 5. The Shadowing Rule

In selector position, an all-digit component is **always** an occurrence index, never a key
value. Keywords cannot be all-digits (TEL §20.7), so no keyword is ever shadowed; but a key
*value* may be all-digits, and such a value is unreachable by key selector. The occurrence
remains addressable by index. TEL §20 accordingly recommends (SHOULD) that key values not
consist solely of ASCII digits.

## 6. Selector Scope

Both selector kinds operate on **same-keyword occurrences**: the sequence S of §4 step 2
contains only children bearing the pending keyword, not all fillings of the member. For a
`Field` member the two notions coincide. For a `SelectRef` member — whose occurrences may carry
different variant keywords — `/pet/2` means "the third `pet`", regardless of how many
occurrences of other variants (say `cat`) of the same Select member are interleaved among them.

The uniqueness constraint of TEL §21.6 is *broader* than this scope: key values are pairwise
distinct across **all** keyed children of the parent filling repeatable members, across all
keywords. A key selector over a same-keyword subsequence of that set is therefore always
unambiguous in a valid document.

## 7. Failures

TELP failures are outcomes of the resolving implementation's API — they are **not** document
errors and carry no E-codes (contrast TEL §19.3). The failure kinds, informatively:

| Kind               | Condition                                                                     |
| ------------------ | ----------------------------------------------------------------------------- |
| syntax             | Empty component, invalid delimiter character, or `LF`/`CR` in the path        |
| unknown keyword    | Keyword-step component matches nothing in the current Struct's keyword order  |
| non-struct descent | Keyword step attempted below a `Value` or a `Flag`-typed node                 |
| absent member      | Non-repeatable optional member not present (and no default)                   |
| index out of range | Index selector ≥ the number of same-keyword occurrences                       |
| type not keyed     | Key selector against a keyword whose type has no key-flagged field            |
| key not found      | Key selector matches no same-keyword occurrence's key value                   |

Implementations SHOULD report the zero-based index of the component at which resolution failed.

## 8. Addressability Limits

Some elements are unreachable by some or all TELPs. None of these conditions is an error in
the document; they only limit which paths can be written.

- A key value containing a candidate delimiter is unaddressable *under that delimiter*:
  choose another (this is the escape mechanism — see the worked example in §10).
- A key value containing `LF` or `CR` is unaddressable by key under **every** delimiter.
- An empty key value is unaddressable by key (components are non-empty).
- An all-digit key value is unaddressable by key (§5).
- A key value using all twenty-two delimiter characters is unaddressable by key.

Every such occurrence remains addressable by index. TEL §20 states the corresponding authoring
guidance: key values SHOULD be inline-safe, non-empty, and not all-digits; identifier-like key
values are RECOMMENDED and are addressable under every delimiter.

## 9. Relation to TEL §22.2 Operation Paths (Informative)

The machine operations of TEL §22.2 address targets by presentation-layer paths. A resolved
TELP element maps onto them as follows:

- A **compound-realized element** corresponds to a **compound path**: walk the presentation
  tree from the root to the element's compound, collecting a `(block_index, compound_index)`
  pair at each level.
- An **atom-realized element** (a `Scalar` or `Flag` element written as an inline atom)
  corresponds to an **atom path**: the owning compound's compound path, plus the element's
  keyword index, plus its occurrence index. Note the unit conversion for Select members: a TEL
  §22.2 atom-path occurrence index counts *all fillings of the member* in semantic order,
  whereas a TELP index selector counts only *same-keyword* occurrences (§6).
- A **default-supplied `Value`** (TEL §18.3 step 5) has no presentation counterpart and maps to
  no operation path.

The mapping is one-way and advisory: TELP defines no operations, and operation paths remain
positional and subject to the invalidation and rebasing rules of TEL §22.2 and §22.5. A tool
holding a TELP re-resolves it after each mutation rather than rebasing it.

## 10. Examples

Consider this schema:

```tel
tel 1.0

name menagerie

record Contact
  field name Identifier key
  field email String optional repeatable

record Toy
  field label String key

record Cat
  field name Identifier key
  field toy Toy optional repeatable

record Dog
  field name Identifier key

select Pet
  variant cat Cat
  variant dog Dog

document
  field owner Contact
  field contact Contact optional repeatable
  select Pet optional repeatable
```

and this document, parsed with the schema above supplied by invocation (TEL §8.2):

```tel
tel 1.0

owner amy
contact bea
  email bea@example.com
  email bea@example.org
contact chu

cat felix
  toy  ball of string
dog rex
cat tom
```

Example paths (all shown with delimiter `/`; any of §3's delimiters would do):

| Path                          | Resolves to                                                          |
| ----------------------------- | --------------------------------------------------------------------- |
| `/`                           | The document root (the context element)                                |
| `/owner`                      | The `owner` element — non-repeatable, so no selector                   |
| `/owner/name`                 | The `Value` `amy`                                                      |
| `/contact`                    | The occurrence sequence of both `contact` elements                     |
| `/contact/bea`                | The first `contact`, by key                                            |
| `/contact/1`                  | The second `contact` (`chu`), by index                                 |
| `/contact/bea/email/1`        | The `Value` `bea@example.org` — `email` has no key, so index-only     |
| `/cat/tom`                    | The second `cat`; `dog rex` between the two cats does not affect the `cat` sequence (§6) |
| `/cat/1`                      | Also the second `cat` (`tom`): index counts same-keyword occurrences   |
| `/dog/0/name`                 | The `Value` `rex`                                                      |
| `/pet`                        | *unknown keyword* — `pet` is a Select name, not a keyword; variants (`cat`, `dog`) are the keywords |
| `/cat/felix/toy/0`            | Felix's toy, by index                                                  |
| `/cat/felix/toy/ball of string` | Felix's toy, by key — components may contain spaces; only the delimiter, `LF`, and `CR` are excluded (§3) |

**Delimiter switch.** Suppose a `contact` has key value `a.b/c`. Under `/` or `.` that key is
unwritable, but any delimiter not occurring in the value works:

```text
:contact:a.b/c:email:0
```

**Shadowing.** Suppose a `contact` has key value `007`. `/contact/007` is an index selector
(index 7, leading zeros permitted) — the occurrence keyed `007` can be selected only by its
position, e.g. `/contact/2`.
