# Hugging Face and Local Model Plan

Models are optional accelerators, not the foundation of ordinary PDF tools.

## Approved for evaluation

### IBM Granite Docling 258M

- Purpose: document layout, tables, formulas and structured conversion.
- License: Apache-2.0.
- Size: approximately 530 MB model repository.
- Caveat: model card identifies English as the language; do not assume Telugu/Hindi quality.
- Gate: benchmark against rule-based/Docling pipeline on owned test documents before enabling.

### intfloat/multilingual-e5-small

- Purpose: local semantic search/retrieval for Ask Document.
- License: MIT.
- Approximately 117M parameters, 384-dimensional embeddings and 512-token input limit.
- Supports a broad multilingual training setup, but Telugu/Hindi retrieval must be benchmarked.

### Tesseract language data

- `eng`, `hin`, `tel` are the initial OCR packs.
- Benchmark accuracy, speed and installation size on representative school documents.

### PaddleOCR Telugu recognition model

- Benchmark-only alternative for Telugu recognition.
- Do not ship until integration, accuracy, license and runtime footprint beat or complement Tesseract.

## Model governance

- Pin repository ID and immutable revision.
- Record model card, license, size and checksums.
- Download only after user action; support remove/update.
- Store under a dedicated model cache, not inside job folders.
- Run local models in a constrained worker process.
- Display model/provider and whether text/images leave the device.
- Evaluate accuracy by language and document type; never advertise universal accuracy.
