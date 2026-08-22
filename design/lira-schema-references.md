# LIRA Schema References

**Status: proposal.** This document proposes replacing the URL form of the pragma schema
identifier with LIRA coordinates, and distributing TEL schemas through the LIRA distribution
system. It contains proposed normative wording for the affected sections of the
[TEL Specification](../spec/tel.md), written so that merging it into the specification is
mechanical once the wording is settled. Section and line references to the current
specification are given as `tel.md §n` and refer to the specification as of this proposal's
writing. The corresponding LIRA-side changes (chiefly a new `tels` discipline) are tracked as
issues on the LIRA repository.

## 1. Motivation

The pragma (tel.md §8.1) currently identifies a document's schema in exactly two forms: an
HTTP(S) URL, optionally carrying a BASE-256 schema signature as a fragment, or a bare BASE-256
schema signature. There is no human-friendly naming scheme: `Schema.name` is explicitly *not* a
pragma identifier (tel.md §20.1), so the only memorable handle is a URL.

The URL form has structural weaknesses:

- A URL-only identifier binds by location, not content: the fetched body is authoritative and
  unverifiable (tel.md §8.2, Signature Verification). Nothing authenticates the publisher.
- Freshness depends on HTTP cache headers; an implementation without HTTP caching must
  re-resolve on every parse (tel.md §8.2, Caching).
- Redirect policy, transport downgrade rules, and hosting are all the publisher's problem;
  there is no lineage — a URL's content may change arbitrarily between fetches.

The LIRA distribution system already provides what the URL form lacks: namespaces validated by
DNS (`_lira.<domain>` TXT records, re-verified at every publish), ML-DSA publisher keys,
lineage control independent of domain control, a BLAKE3 transparency log, and *derived*
semantic versions computed from a compatibility algebra (lira.md §12). LIRA's log records and
manifests are themselves BinTEL documents, and LIRA's extensibility mechanism is expressed as
TEL schema layers with no specified shipping mechanism. The two systems are natural
counterparts: LIRA gives TEL schemas names, versions, and provenance; TEL gives LIRA the
compatibility discipline for schema payloads.

Since TEL has no users yet, the URL form is deleted outright, with no deprecation period.
LIRA becomes the sole network resolution path, and every network-resolved schema is
signature-verified.

## 2. Pragma Grammar

*Replaces the phrase rules of tel.md §8 (the "at most three phrases" rule and the positional
form) and the whole of §8.1.*

### 2.1 Positional form

The pragma line consists of the keyword `tel` followed by the following parameters, each
OPTIONAL except the version, in this order:

1. TEL version (REQUIRED, `x.y` — rules unchanged from tel.md §8)
2. schema reference (a LIRA reference)
3. zero or more layer selections
4. schema signature
5. sigil

```text
tel 1.0 propensive.dev/build +publishing +maven ‹base256-signature› #
```

Phrases after the version are classified by form, then checked against the positional order:

- A phrase beginning with `+` is a **layer selection**; the remainder of the phrase is a layer
  name.
- A phrase containing `/` and longer than one character is a **schema reference**.
- A phrase that is a valid BASE-256 string of length 33 or `37 + 2·(n − 2)`, `n ≥ 2`, is a
  **schema signature**.
- A final phrase that is a single sigil-valid character (§6) is a **sigil**. (Form-first
  classification makes a lone `/` or `+` unreachable as a sigil; see §2.4.)

A phrase matching none of these forms, or a sequence of phrases violating the order or
multiplicity above (e.g. a signature before a layer selection, two references, two signatures),
is invalid (**E121**/**E122**, reworded — see §7). Remarks are still not recognised on the
pragma line.

The classification is unambiguous:

- The BASE-256 alphabet consists entirely of Unicode letters and ASCII digits (base256.md §4),
  so a signature can never contain `+`, `/`, `:` or `.`.
- A schema reference always contains `/` (it carries a domain and a name) and never `+`.
- `+` is removed from the sigil alphabet (§2.4), and no other sigil-valid character can begin
  a layer selection or appear in a signature; a lone `/` or `:` cannot be a reference (too
  short / no `/`).

### 2.2 Schema reference

A **schema reference** is a LIRA coordinate, optionally followed by a release selector:

```text
reference  = domain "/" module-name [":" selector]
selector   = version | tag
```

- `domain` is a DNS domain name, the LIRA namespace (lira distribution design §2), proven by a
  `_lira.<domain>` TXT record.
- `module-name` is LIRA's `module-name` scalar: kebab-case segments joined by `/` or `.`
  (lira.md §14).
- `version` is LIRA's `semver` scalar: exactly `x.y.z`, each a decimal natural with no
  superfluous leading zero (lira.md §14). LIRA versions are derived, not chosen (lira.md
  §12.5); prerelease and build suffixes do not exist.
- `tag` is LIRA's `tag-name` scalar: a letter followed by letters, digits, `-` and `.`
  (lira.md §14). Because a version begins with a digit and a tag begins with a letter, the two
  selector forms are syntactically disjoint.

A reference with a selector (`propensive.dev/build:2.1.0`, `specification.tel/jdk:jdk-19`)
names exactly one published LIRA release and is globally resolvable (§3). A reference without a
selector (`propensive.dev/build`) is a **development reference**: it resolves only against
local state and is not portable (§3, §5).

`:` appears in neither the BASE-256 alphabet nor the coordinate grammar, so a reference
(with or without selector) always occupies a single phrase, and the presence of `/`
distinguishes it from every other phrase form.

### 2.3 Layer selections

A schema published through LIRA may declare OPTIONAL layers. A document selects layers by
listing them as `+`-prefixed phrases between the reference and the signature:

```text
tel 1.0 propensive.dev/build +publishing +maven ‹signature› #
```

- Each layer name MUST match a layer declared by the referenced schema; an unknown name is a
  resolution error.
- Composition follows the schema's **declaration order**, and the pragma MUST list the
  selected layers in declaration order (new E1xx error — see §7). Each selected layer set
  therefore has exactly one canonical pragma spelling and exactly one composed signature; two
  documents selecting the same layers can never disagree on order and spuriously fail the
  compatibility check of tel.md §8.2.
- Layer selections without a reference are permitted only alongside a signature, where they
  serve as decomposition hints for library lookup (resolution step 3): the names guide the
  match against declared layer names, as the LSP schema registry already does.

### 2.4 Sigil alphabet change

`+` is removed from the `Sigil` type of tel.md §6, reducing the enumeration from twenty-four
characters to twenty-three. This keeps single-character phrase classification trivially
unambiguous: `+` always introduces a layer selection, and a bare `+` phrase is invalid (E121)
rather than a sigil. All rules stated in terms of sigil-validity (E105, E207, `Schema.sigil`,
§21.5) inherit the change.

The positional note of tel.md §8 ("In `tel 1.0 %`, the `%` occupies the schema-identifier
position…") is superseded: classification is now by form first, so `tel 1.0 %` reads `%` as a
sigil. A document may therefore set its sigil without naming a schema. (E121 remains for
phrases that match no form.)

### 2.5 Signature semantics with layers

The schema signature phrase is unchanged in form (tel.md §8.1: a BASE-256 palimpsest of the
256-bit BLAKE3 component hashes; 33 characters for one component, `37 + 2·(n − 2)` for `n ≥ 2`;
constructed per bintel.md §8). Signatures remain 256-bit: truncation to 128 bits was considered
for brevity and rejected, since 2⁶⁴ collision work would let a publisher mint a benign and a
malicious schema sharing one signature, defeating transparency-log auditability. References
make long signatures rare in hand-written documents anyway.

When a signature follows layer selections, it is authoritative and MUST include them: the
signature MUST decompose into exactly `1 + n` components — the referenced schema's base hash
followed by the hashes of the `n` selected layers, in order. Any mismatch (wrong component
count, wrong hashes, wrong order) is a runtime resolution error. The pragma reads as a single
claim: *this document uses schema `ref`, with these layers, as attested by this signature.*

### 2.6 The `tels` meta-schema

The built-in `tels` meta-schema has the canonical, version-pinned coordinate:

```text
specification.tel/tels:2.0.0
```

The pin is fixed until the TEL specification itself is revised; a later revision publishes a
new LIRA release of `tels` and advances the pinned version. Resolution step 1 (built-in
lookup) recognises this pinned coordinate as well as the built-in signature, in both cases
without network access.

### 2.7 Name binding

A schema is published under the module name it declares: a schema whose source contains
`name foo` MUST be published as `‹domain›/foo`, and its declared layer names are the names
addressable by `+` selections. Both bindings are enforced at publish time by the LIRA `tels`
discipline (§6). This guarantees that the names in a pragma are exactly the names in the
schema source a reader will find.

## 3. Schema Resolution

*Replaces tel.md §8.2's matching rule, Resolution Protocol, Signature Verification, and
Caching subsections. The invocation-vs-document outcome table, the compatibility
(signature-subsequence) rule, Layered-Signature Decomposition, and the Runtime Resolution
Error subsection are retained as they stand, except as noted.*

### 3.1 Matching

Two schema identifications **match** iff both carry a signature and the signatures are
identical. The URL-identity clause of the current matching rule is deleted along with the URL
form. A document whose pragma carries only a development reference (no selector, no
signature) has no global identity and never matches an invocation schema by name; the outcome
table's "matching"/"compatible" rows apply only once a signature is in play (directly, or
computed from a resolved release).

### 3.2 Resolution protocol

Steps 0–3 are unchanged in substance (embedded-schema lookup; built-in lookup, now also
matching `specification.tel/tels:2.0.0`; cache lookup; library lookup), with one
generalisation: **any content-addressed store may serve steps 2–3.** In particular, an
implementation MAY consult both the tel schema cache and a local LIRA store, in either order —
a hash lookup is order-independent, because any store's answer for a given signature is the
right answer. No precedence rule is needed or specified.

Step 4 (URL fetch) is replaced by **LIRA resolution**, by identifier form:

- **Signature present** (with or without a reference): resolve the signature's components by
  exact identity through LIRA (local store first, then network), verify the recomputed
  signature byte-for-byte as in tel.md §8.2 Signature Verification, and fail on mismatch. If a
  reference is also present and the resolved lineage does not serve the signature, resolution
  fails: the signature is authoritative, and the reference must agree with it.
- **Reference with `:version` or `:tag`**: globally resolvable and unambiguous. A version
  exists only on a published, signed release — unpublished LIRA releases are normatively
  versionless (lira.md §12.5, L117) — and a tag is signed, unique, and immutable within its
  module (lira.md §12.6, L142). Resolution consults the local LIRA store, then LIRA network
  resolution. The resolved body MUST be a TEL document conforming to the `tels` schema; its
  computed signature becomes the identity under which it is cached and matched. A resolver
  MUST verify the release's manifest signature — it MUST NOT trust a local store index alone.
- **Bare reference** (no selector, no signature): **local-only by design.** The reference
  resolves against the local tel schema cache — the developer's working copy — and never
  triggers network resolution. A document carrying only a bare reference is deliberately not
  portable; portability is always explicit, via a selector or a signature (§5).

Step 5 (failure) is unchanged: a resolution failure is a runtime error reported outside the
E1xx/E2xx/E3xx taxonomy, identifying the failing step. A non-network-capable parser MAY omit
the network half of step 4 and treat anything unresolved by local state as a failure. Offline,
a bare reference needs no network at all, and a selector-form reference fails resolution only
if absent from every local store.

The following current rules are deleted with the URL form: the HTTPS/HTTP transport and
redirect rules, the URL-only authoritative binding ("no verification is possible"), and
HTTP-cache-header-driven freshness. After this change **every cached schema is
signature-verified**, and schema bodies are cached indefinitely, keyed by signature.

### 3.3 Serving validation

A tel server (LSP or `tel validate`) asked to process a document whose pragma carries a
selector-form reference it does not hold locally SHOULD defer to LIRA to fetch it, subject to
the same verification rules, rather than reporting an unresolved-schema diagnostic outright.

## 4. Compatibility: TEL Subtyping as LIRA Grades

TEL already has a complete structural versioning discipline: schemas grow by appending layers,
`S_doc <: S_cons` iff `S_cons`'s decoded hash sequence is a subsequence of `S_doc`'s (tel.md
§8.2, §24.4, §24.5), and the signature encodes the exact composition. LIRA independently has a
release-grading algebra: a successor is a **patch** if its atom set is identical, **minor** if
it extends the predecessor within rigid monotonicity, and otherwise **major**, beginning a new
lineage (lira.md §12.3); the published version number is *derived* from the grade (lira.md
§12.5).

For TEL schema payloads the two coincide, by construction:

| TEL relation between successive releases            | LIRA grade | Derived version step |
| --------------------------------------------------- | ---------- | -------------------- |
| identical composed signature                        | patch      | `z + 1`              |
| predecessor's hash sequence is a proper subsequence | minor      | `y + 1`, `z := 0`    |
| subsequence relation broken                         | major      | new lineage          |

This mapping is the substance of the LIRA `tels` discipline (§6): the discipline decomposes a
schema payload into atoms such that LIRA's grade computation *is* TEL's signature-subsequence
test. A consumer can then ask LIRA for "the newest release of `propensive.dev/build` whose
schema extends the signature my document carries" and get an answer that TEL's own subtype
relation certifies.

## 5. Development and Publication Lifecycle

The design supports developing a document and its schema in parallel, before anything is
published:

1. **Local development.** The developer loads the schema into the local tel cache
   (`tel schema add`) and edits it in an IDE with LSP feedback. Documents under development
   reference it by bare reference (`propensive.dev/build`): resolvable locally, portable
   nowhere. Schemas at this stage can change freely; nothing global has been claimed.
2. **Rubber-stamping.** To make a document portable without publishing the schema, the LSP
   offers a code action that appends the resolved schema's composed signature to the pragma
   (and, once the schema is published, the `:version` selector). The document then carries
   content-addressed identity wherever it goes. (Specified here; implemented in a later LSP
   round.)
3. **Publication.** Publishing a schema happens exclusively through LIRA and requires an
   explicit signature from a key bound to the module's lineage — never implicitly. The `tels`
   discipline computes the grade against the previous release and derives the next version
   number (§4). From then on, `‹domain›/‹name›:‹version›` is a stable, globally resolvable,
   signed name for that exact schema.

There is no cache ambiguity in this lifecycle. Hash lookups are content-addressed and may be
served by the tel cache or the LIRA store in any order. Selector lookups can only ever match
published, signed releases, because a local store cannot hold an unpublished bundle claiming a
version (unpublished releases are versionless) and tags are signed and immutable. The only
name that means different things on different machines is the bare reference — and that is its
purpose.

## 6. LIRA-Side Changes

Filed as issues on the LIRA repository; recorded here for cross-reference:

1. **A `tels` discipline** (the main change): a per-format discipline in the pattern of LIRA's
   existing discipline specs, decomposing a TEL schema payload into atoms such that grade
   computation coincides with the signature-subsequence relation (§4); determining the derived
   version on publication; enforcing the name bindings of §2.7; and exposing the composed
   schema signature for resolution queries.
2. **Serving TEL schemas as LIRA modules**: adopt `‹domain›/‹name›:‹version›` (and `:‹tag›`)
   as LIRA's general versioned-reference syntax, cross-referencing the pragma grammar of §2.2;
   resolution semantics for version, tag, and signature queries; a normative requirement that
   resolvers verify release manifest signatures rather than trusting a store index; and using
   schema modules as the shipping mechanism for LIRA's own so-far-unspecified "schema layers
   that arrive" extensibility seam.

The selector grammar needs no LIRA-side tightening: `tag-name` already begins with a letter
and `semver` with a digit (lira.md §14), so the two are disjoint as required by §2.2.

## 7. Consequential Specification Edits

Checklist of every location touched when this proposal merges (line references as of this
writing):

- `spec/tel.md` §6 (`Sigil` type, lines 162–193): remove `+`; twenty-four → twenty-three.
- `spec/tel.md` §8 (lines 244–305): phrase count and positional rules replaced by §2.1 of this
  document; E122 reworded from "at most three phrases after `tel`" to the order/multiplicity
  rule; the `tel 1.0 %` positional note replaced per §2.4.
- `spec/tel.md` §8.1 (lines 307–333): replaced by §2.2–§2.6 of this document; E121 reworded to
  "phrase matches no pragma form" plus the reference/selector grammar; a new E1xx code for
  layer selections out of declaration order.
- `spec/tel.md` §8.2 (lines 335–458): matching rule, Resolution Protocol step 4, Signature
  Verification, and Caching replaced per §3; delete lines 403–408 (URL fetch, transport,
  redirects), 431–433 (URL-only binding), 438–440 (HTTP cache headers).
- `spec/tel.md` §8.3 (lines 460–480): unchanged in substance; re-examine wording that assumes
  the three-parameter positional form.
- `spec/tel.md` error taxonomy (E121/E122 entries near line 1249; E105 wording): update per
  the above.
- `spec/tel.md` §20.4 (lines 2307–2312) and §24.5 tooling note (lines 3891–3892): re-point
  "identification (§8.1)" wording at the new identifier forms.
- `spec/bintel.md` "Textual form" (lines 563–572): the pragma-token rationale for BASE-256
  stands; drop any mention of the URL fragment carriage.
- `readme.md` line 52: the example `tel 1.0 contact %` is already invalid under the *current*
  specification (a bare name is E121) and must be rewritten — under this proposal, e.g.
  `tel 1.0 specification.tel/contact:1.0.0 %` or a bare development reference.
- Reference implementation (a later round): `parse_pragma`/`is_valid_schema_id`
  (`ref/tel/src/lib.rs`) and `SchemaIdentifier::parse`/`Resolver`
  (`ref/tel/src/resolver.rs`) — replace URL handling with reference/layer/selector parsing;
  note the current divergences (any-scheme `://` acceptance; the test harness's URL-tail
  name-resolution convention, which this proposal obsoletes).
- LSP (a later round): pragma completion/diagnostics for references and layer selections; the
  rubber-stamp code action (§5); LIRA-deferred fetching (§3.3). The existing
  `SchemaCache` name-then-signature lookup already anticipates bare-reference resolution.
