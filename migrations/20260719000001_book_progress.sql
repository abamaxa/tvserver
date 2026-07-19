CREATE TABLE book_progress (
    checksum INTEGER PRIMARY KEY NOT NULL REFERENCES books(checksum) ON DELETE CASCADE,
    locator_type TEXT NOT NULL CHECK (locator_type IN ('epub-cfi', 'pdf-page')),
    locator_value TEXT NOT NULL CHECK (length(trim(locator_value)) > 0),
    progression REAL CHECK (progression IS NULL OR (progression >= 0.0 AND progression <= 1.0)),
    updated_on TEXT NOT NULL
);
