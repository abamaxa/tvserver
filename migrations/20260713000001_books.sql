CREATE TABLE IF NOT EXISTS books (
    checksum INTEGER PRIMARY KEY NOT NULL,
    file_name TEXT NOT NULL,
    collection TEXT NOT NULL,
    title TEXT NOT NULL,
    authors TEXT,
    description TEXT,
    publisher TEXT,
    published_date TEXT,
    language TEXT,
    isbn TEXT,
    format TEXT NOT NULL,
    page_count INTEGER,
    thumbnail TEXT NOT NULL,
    metadata TEXT,
    search_phrase TEXT,
    state INTEGER DEFAULT 0,
    created_on TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_on TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_books_collection_file
    ON books(collection, file_name);

CREATE INDEX IF NOT EXISTS idx_books_title
    ON books(title);

CREATE INDEX IF NOT EXISTS idx_books_authors
    ON books(authors);
