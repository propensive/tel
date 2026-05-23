# Worked example: a sequence of machine operations

This example shows the §22.2 machine operations applied in sequence to a
semantic model, with the source-level reserialization preserved alongside.
It mirrors the way an automated editor or refactoring tool would update a
TEL document while keeping comments, remarks, and tabulation intact.

## Initial document

We start with a small slice of `demo/contact-document.tel`:

```tel
tel 1.0 https://example.org/contact

name  Alice Anderson
email alice@example.org   # personal address

home
  street   221B Baker Street
  city     London
  country  UK

active
```

Two presentation features matter for the operations below:

- The `email` line carries a **remark** (`# personal address`).
- The `home` block has three children aligned with hard-space padding so
  `street`, `city`, and `country` values column-align.

## Operation 1: `update-value` on the email scalar

The agent updates Alice's email to `alice@alice.example`:

```
update-value(target: <email scalar>, value: "alice@alice.example")
```

After the operation:

```tel
email alice@alice.example   # personal address
```

The remark is preserved (§22.2 `update-value`: "All other presentation
details of the compound are retained.").

## Operation 2: `insert` a new phone Field

The agent records Alice's phone number. The schema declares `phone` as
`repeatable`. Calling:

```
insert(parent: <root>, member: phone, value: <new compound>)
```

with a constructed compound:

```
Compound { keyword: "phone",
           atoms: [],
           children: [
             Compound { keyword: "country-code", atoms: ["44"] },
             Compound { keyword: "number",       atoms: ["020-7946-0100"] },
           ] }
```

The insertion uses the `construct` operation's canonical form (§22.2,
"Atom form escalation"). Both children are atom-form scalars; the result
is placed after the `email` line and before `home` (the natural position
for the `phone` member group, per the schema's member order):

```tel
email alice@alice.example   # personal address

phone
  country-code 44
  number 020-7946-0100

home
  …
```

Note the blank line preserved between `email` and the inserted `phone` —
the `insert` operation does not add or remove blank lines beyond the
construct rules; existing presentation framing is retained.

## Operation 3: `switch-variant` on the active/archived select

Bookkeeping update — Alice's record is archived. The schema's `select`
member has two `Flag` variants (`active`, `archived`). The agent calls:

```
switch-variant(target: <select compound>, new_variant: archived)
```

Since the existing compound's variant is being replaced (Flag to Flag,
same Select member), the keyword changes in place:

```tel
…

active
```

becomes

```tel
…

archived
```

No surrounding presentation detail changes.

## Operation 4: `attach-remark` on the home block's parent

The agent annotates the `home` block:

```
attach-remark(target: <home compound>, text: "primary residence")
```

After:

```tel
home   # primary residence
  street   221B Baker Street
  city     London
  country  UK
```

Any column alignment within the block is preserved; the remark is added
to the parent compound only.

## Operation 5: `delete` the `country` line

Alice withdraws her country. The schema makes `country` required with a
default value of `unknown`, so the deletion does not violate E307 — the
default fills the now-absent member:

```
delete(target: <country scalar>)
```

The presentation form drops the line:

```tel
home   # primary residence
  street   221B Baker Street
  city     London
```

Semantically, `country` is still present in the model, valued `unknown`
(per §20.2 default substitution). BinTEL encoding (§7) of the resulting
document will include `country = unknown` as if it had been written
verbatim — that is the canonicalization rule pinned in `spec/bintel.md`
§7.

## Why this matters

Each operation modifies the semantic model but leaves the surrounding
presentation invariants alone:

- Remarks travel with the compound (operations 1, 4).
- Blank lines and column alignment within blocks survive insertion,
  deletion, and variant switching (operations 2, 3, 5).
- Defaults make some deletions trivially safe (operation 5).

For the canonical (non-presentation-preserving) form, see §22.3 of the
TEL Specification: applied to the final state above, it would emit a
flat, comment-stripped, tabulation-free document whose BinTEL encoding
is byte-identical to the encoding produced from the same semantic model
in any other presentation.
