CREATE TABLE job_completion_outcomes (
    job_id TEXT PRIMARY KEY
      REFERENCES jobs(id) ON DELETE CASCADE,

    completion_kind TEXT NOT NULL
      CHECK (completion_kind IN ('published', 'no-benefit')),

    reason TEXT,

    created_at TEXT NOT NULL,

    CHECK (
      (completion_kind = 'published' AND reason IS NULL)
      OR
      (
        completion_kind = 'no-benefit'
        AND reason IS 'savings-threshold-not-met'
      )
    )
) STRICT;
