CREATE TABLE book_progress (
    checksum INTEGER PRIMARY KEY NOT NULL REFERENCES books(checksum) ON DELETE CASCADE,
    locator_type TEXT NOT NULL CHECK (locator_type IN ('epub-cfi', 'pdf-page')),
    locator_value TEXT NOT NULL CHECK (length(trim(locator_value, char(9, 10, 11, 12, 13, 32, 133, 160, 5760, 8192, 8193, 8194, 8195, 8196, 8197, 8198, 8199, 8200, 8201, 8202, 8232, 8233, 8239, 8287, 12288))) > 0),
    progression REAL CHECK (progression IS NULL OR (progression >= 0.0 AND progression <= 1.0)),
    updated_on TEXT NOT NULL
);
