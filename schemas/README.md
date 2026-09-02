# `schemas/` — the ECMA-376 XSDs, vendored

The schema set `corpus/schema_gate.py` validates generated `.pptx` parts against
(Phase 15 increment 15-I; `docs/technical-analysis-presentations.md` §5.5).

Nothing in this directory is compiled into any binary. It is read by one Python
script, in CI and on a developer's machine, and by nothing else.

## Provenance

Everything under `ecma-376-part4-transitional/` and `ecma-376-part2-opc/` is
**byte-for-byte as published by Ecma International**. Do not edit these files:
the copyright notice they are distributed under permits redistribution but not
modification (see *Licence* below), and a locally patched schema would also make
the gate measure a standard nobody else has.

| directory | files | bytes | downloaded from | sha256 of the zip |
| --- | --- | --- | --- | --- |
| `ecma-376-part4-transitional/` | 26 | 941 846 | `ECMA-376-4_5th_edition_december_2016.zip` → `OfficeOpenXML-XMLSchema-Transitional.zip` | `d341875207…4ca04fc` (inner) |
| `ecma-376-part2-opc/` | 4 | 8 852 | `ECMA-376-2_5th_edition_december_2021.zip` → `OpenPackagingConventions-XMLSchema.zip` | `e5a0a7c1f4…7fd55df` (inner) |
| `w3c/` | 1 | 8 838 | `https://www.w3.org/2009/01/xml.xsd` | `cc701736c4…57aff829` (the file) |

The outer zips, both retrieved from `https://ecma-international.org/wp-content/uploads/`
on 2026-09-02:

- `ECMA-376-4_5th_edition_december_2016.zip`, 8 471 820 bytes,
  sha256 `bd25da1109f73762356596918bf5ff8b74a1331642dba5f1c1d1dfc6bed34ecd`
- `ECMA-376-2_5th_edition_december_2021.zip`, 1 899 204 bytes,
  sha256 `1d489dc491168ea1f9e9a59063acc8dd5f02b4ad1d21aa7ec19ba9a58d020c70`

The standalone `.../uploads/OfficeOpenXML-XMLSchema-Transitional.zip` URL that
older documentation points at is **dead** (404 on an HTML error page). The
schemas ship inside the standard's own distribution zip, next to the PDF, which
is why the table above names two levels of archive.

**Transitional, not Strict.** ECMA-376 Part 1 5th edition ships the *Strict*
schemas; Part 4 ships *Transitional*. Every deck any real application writes —
including every deck in `corpus/pptx`, generated with `python-pptx` — is
Transitional, and validating it against Strict would report the standard's own
migration features as errors. Strict is not vendored.

`w3c/xml.xsd` is the schema for the `xml:` namespace (`xml:lang`, `xml:space`).
The published OOXML schemas reference it and do not ship it; without it the
schema set does not compile. It is W3C's, unmodified.

## Licence

The `.xsd` files carry no copyright header of their own, and neither do the
ECMA-376 PDFs: the standard is from 2006 and Ecma began printing its copyright
notice into standards in 2009, saying that "in practice the same principle also
applies" to the older ones. The terms are therefore the ones in Ecma
International's text copyright policy, by policy rather than by inclusion. The
notice is reproduced in full, as the licence requires, in
[`ECMA-NOTICE.txt`](ECMA-NOTICE.txt) beside this file
(<https://ecma-international.org/policies/by-ipr/ecma-text-copyright-policy/>):

> This document may be copied, published and distributed to others, and certain
> derivative works of it may be prepared, copied, published, and distributed, in
> whole or in part, provided that the above copyright notice and this Copyright
> License and Disclaimer are included on all such copies and derivative works.

with the constraint that decides the "do not edit" rule above:

> the content of this document itself may not be modified in any way, including
> by removing the copyright notice or references to Ecma International, except
> as required to translate it into languages other than English or into a
> different format.

Ecma states that these permissions are perpetual and will not be revoked.
Verbatim redistribution inside this repository, with that notice travelling
beside the files, is exactly what the licence covers; clause (iv) separately
permits implementing the schemas in conformant products. Copyright © Ecma
International. `w3c/xml.xsd` is under the W3C Software and Document Notice and
License.

Patents are not the constraint. ECMA-376 predates Ecma's royalty-free patent
policy option, so Ecma's own terms are RAND — but Microsoft's patent declaration
to Ecma for ECMA-376 makes the same offer "on a royalty-free basis" and points
at the Open Specification Promise, which lists Office Open XML among its covered
specifications. The copyright no-modification rule is the clause that actually
binds this directory.

## Why vendored rather than fetched in CI

Both options were weighed:

- **Fetched.** No bytes in the repository, always current — but the gate then
  depends on a URL, and the URL this documentation used to name is already dead.
  A red build caused by `ecma-international.org` being slow is a red build that
  says nothing about the writer, and a gate nobody can run offline is a gate
  developers route around. Fetching the schemas means fetching **10 MB of zip,
  most of it a PDF**, on every run, or building a cache with its own failure
  modes.
- **Vendored.** 1.0 MB and 31 files added once, no network at all, the same
  bytes on every machine and in every future run, and the exact version the gate
  was written against is recoverable from `git log`. The cost is the 1.0 MB, in
  a repository whose `.git` is already 55 MB, and the duty to state the
  provenance — which is this file.

Vendored wins. The deciding argument is reproducibility: a validation gate whose
oracle can change under it without a commit is not a gate.

## The driver

`ooxml-transitional.xsd` is **this repository's** file, not Ecma's: it imports
the published schemas into one schema set so that a part can be matched to a
global element declaration by its root element's qualified name, whatever
namespace that is. Its header comment says what it deliberately leaves out and
what that costs.

## Refreshing the set

Download the two zips above, unpack the inner schema zips, and copy the `.xsd`
files over the two vendored directories without changing them. If Ecma
publishes a 6th edition, the version in this file's table is what says which one
the gate is measuring against.
