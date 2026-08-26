CREATE TABLE job_balanced_compression_audits (
    job_id TEXT PRIMARY KEY
      REFERENCES jobs(id) ON DELETE CASCADE,

    profile TEXT NOT NULL
      CHECK (profile = 'balanced-v1'),

    source_bytes INTEGER NOT NULL CHECK (source_bytes > 0),
    candidate_bytes INTEGER NOT NULL CHECK (candidate_bytes > 0),
    selected_images INTEGER NOT NULL CHECK (selected_images >= 0),
    skipped_images INTEGER NOT NULL CHECK (skipped_images >= 0),
    affected_pages INTEGER NOT NULL CHECK (affected_pages >= 0 AND affected_pages <= 128),
    compared_pages INTEGER NOT NULL CHECK (compared_pages >= 0 AND compared_pages <= 128),

    minimum_ssim REAL,
    minimum_psnr_db REAL,
    psnr_is_infinite INTEGER NOT NULL CHECK (psnr_is_infinite IN (0, 1)),
    maximum_changed_pixels INTEGER NOT NULL CHECK (maximum_changed_pixels >= 0),
    maximum_total_pixels INTEGER NOT NULL CHECK (maximum_total_pixels >= 0),

    quality_passed INTEGER NOT NULL CHECK (quality_passed IN (0, 1)),
    size_gate_passed INTEGER NOT NULL CHECK (size_gate_passed IN (0, 1)),
    structural_proof_sha256 TEXT NOT NULL
      CHECK (length(structural_proof_sha256) = 64
        AND structural_proof_sha256 NOT GLOB '*[^0-9a-f]*'),
    created_at TEXT NOT NULL,

    CHECK (compared_pages = affected_pages),
    CHECK (
      (affected_pages = 0
        AND minimum_ssim IS NULL
        AND minimum_psnr_db IS NULL
        AND psnr_is_infinite = 0
        AND maximum_changed_pixels = 0
        AND maximum_total_pixels = 0)
      OR
      (affected_pages > 0
        AND minimum_ssim IS NOT NULL
        AND minimum_ssim >= -1.0 AND minimum_ssim <= 1.0
        AND ((psnr_is_infinite = 1 AND minimum_psnr_db IS NULL)
          OR (psnr_is_infinite = 0 AND minimum_psnr_db IS NOT NULL
            AND minimum_psnr_db >= 0.0))
        AND maximum_total_pixels > 0
        AND maximum_changed_pixels <= maximum_total_pixels)
    )
) STRICT;

CREATE TABLE job_balanced_compression_skip_counts (
    job_id TEXT NOT NULL
      REFERENCES job_balanced_compression_audits(job_id) ON DELETE CASCADE,
    reason TEXT NOT NULL CHECK (reason IN (
      'below-minimum',
      'unsupported-filter',
      'decode-parameters',
      'unsupported-colorspace',
      'non-rgb8',
      'mask-or-transparency',
      'external-or-alternate',
      'unsafe-resource-ancestry',
      'ambiguous-shared-use',
      'inline-image',
      'candidate-not-smaller',
      'candidate-quality',
      'candidate-decode'
    )),
    count INTEGER NOT NULL CHECK (count > 0),
    PRIMARY KEY (job_id, reason)
) STRICT;
