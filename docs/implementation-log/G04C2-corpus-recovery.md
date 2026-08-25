# G04C2 corpus recovery and freeze

## Scope and root cause

This corpus-only slice freezes the reviewed photographic fixtures and deterministic PDF test inputs required by G04C2B. It does not add or expose balanced-compression production logic.

- The frozen G04C1 corpus binaries were not committed to the repository.
- The evidence/harness lived beneath a local `.codex/visualizations` path.
- The later handoff retained hashes and descriptions but not a complete machine-readable acquisition manifest containing exact filenames, page revisions, derivative width, dimensions, size and SHA-256.
- The blocked attempt guessed source files and selected wrong variants and/or original-resolution files.
- This is a corpus-provenance/handoff failure, not evidence of Dell-to-MSI byte corruption.
- No production G04C2 code was written before the gate, so no production rollback is required.

The existing `C:\Dev\document-studio-g04c2b-corpus-probe` was inventoried read-only and retained. A size-prefiltered SHA-256 search of the approved local project/artifact roots found none of the six frozen binaries. No Dell backup, transferred profile or external drive was mounted. No recovered source or user file was moved or deleted.

## Acquisition and controlled rebaseline

All six exact permanent description-page revisions were retained during acquisition. The exact File title was resolved through the Wikimedia Commons MediaWiki API with `iiurlwidth=1280`; browser previews and screenshots were not used. Each page and live API metadata identified George Chernilevsky, PD-self/Public domain, and `image/jpeg`.

Five clean downloads reproduced the old frozen bytes. The exact Uzh River identity repeatedly resolved to the same new derivative bytes in the initial download, two clean redownloads and an independent reviewer download. Its file identity, permanent page, 1280px width, 1280 x 817 dimensions, author, licence, JPEG format and decoded RGB8 content all matched. The independently reviewed result therefore uses `CORPUS_MODE=reviewed-rebaseline`, changes only Uzh's current byte-size/SHA fields, and preserves its historical fields under `previousFrozenEvidence`.

| ID | Permanent revision | Old dimensions | Old bytes | Old SHA-256 | Final dimensions | Final bytes | Final SHA-256 |
|---|---:|---:|---:|---|---:|---:|---|
| sunflower-head | 1251267196 | 1280 x 1640 | 335586 | `65d68804b5aa34fd0235578a1640aa67fc7eba3ea40d2269ebdf5a15e054461c` | 1280 x 1640 | 335586 | `65d68804b5aa34fd0235578a1640aa67fc7eba3ea40d2269ebdf5a15e054461c` |
| folk-architecture | 1250325699 | 1280 x 1920 | 573944 | `6d456ef68231e17adf98f6be7e673e3baa887bbefc2f428a037ad11e93bbdcf2` | 1280 x 1920 | 573944 | `6d456ef68231e17adf98f6be7e673e3baa887bbefc2f428a037ad11e93bbdcf2` |
| lviv-church | 1098857210 | 1280 x 2065 | 390581 | `8e2ac2ca4b813ae997550c9383e782506fa7afddcef9fb805d7f91c424d31cd5` | 1280 x 2065 | 390581 | `8e2ac2ca4b813ae997550c9383e782506fa7afddcef9fb805d7f91c424d31cd5` |
| uzh-river | 1110946301 | 1280 x 817 | 296546 | `0fa88acf594e48c5a8e87e588056f66aad4cc00035b655648c37c1b54938e727` | 1280 x 817 | 294673 | `9701b25bed3fd169109e7ef564bd40f34f7dcec5dfb71c41dd0d85f9bb94eed8` |
| thorichthys-meeki | 1238638582 | 1280 x 987 | 301084 | `466643377947c17d8864d4041f550ff7381603d7cdb8510c4082f2a05512c0c6` | 1280 x 987 | 301084 | `466643377947c17d8864d4041f550ff7381603d7cdb8510c4082f2a05512c0c6` |
| fruit-on-plate | 1262411860 | 1280 x 1013 | 301998 | `f92dade021e25105fc3404d6cba98386ee30ef22861c7e7f4437b17f187e161b` | 1280 x 1013 | 301998 | `f92dade021e25105fc3404d6cba98386ee30ef22861c7e7f4437b17f187e161b` |

## Deterministic and offline evidence

The committed corpus contains only the six public-domain photographs and PDFs derived from them; it contains no private or user document. `scripts/g04c2_corpus.py` has a hard-coded six-entry identity allow-list, validates the manifest and source bytes, rejects named G1/G03/other-subject/original-resolution substitutions, and generates PDFs without metadata or resampling. It verifies that every embedded DCT stream has the same dimensions, byte count and SHA-256 as its committed source JPEG.

CI does not contact Commons. The cross-platform validation job verifies the manifest, JPEGs, negative probes and byte-identical double generation. The Windows job additionally uses the reviewed bundled qpdf 12.3.2 with recovery disabled to open all seven PDFs and confirm one page in each individual fixture and six pages in the aggregate.
