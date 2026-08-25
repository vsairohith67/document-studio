# G04C2 balanced-compression corpus

This directory is the network-free acceptance corpus for `pdf.compress-balanced@1.0.0`. It contains six public-domain 1280px Wikimedia Commons JPEG derivatives and seven deterministic PDFs: one one-image page per source plus one six-page aggregate.

`corpus-manifest.json` is authoritative. It pins the exact Commons file title, permanent description-page revision, API request, original and resolved derivative URLs, author, PD-self evidence, dimensions, byte size and SHA-256. `CORPUS_MODE` is `reviewed-rebaseline`: five JPEGs reproduce the earlier frozen evidence exactly, while the current correct Uzh River derivative was independently reviewed and its superseded size/hash remain under `previousFrozenEvidence`.

The PDFs use fixed 612 x 792 point pages and contain no creation date, modification date or generated ID. Each JPEG is embedded once as an unchanged `/DCTDecode` stream; placement changes only the PDF drawing transform and does not resample pixels.

From the repository root:

```powershell
python -B scripts/g04c2_corpus.py check
npm run verify:g04c2-corpus --workspace @document-studio/desktop
```

The first command verifies identity, dimensions, bytes, hashes, strict JPEG framing, negative substitutions, two independent deterministic generations and embedded DCT-stream hashes. The second also opens every PDF with the reviewed qpdf 12.3.2 using recovery disabled and confirms its page count. Neither command uses the network.

Do not replace these files with preview screenshots, local recompressions, original-resolution images, or a photograph with a similar subject. Regeneration changes only the PDFs and always reads the committed JPEGs.
