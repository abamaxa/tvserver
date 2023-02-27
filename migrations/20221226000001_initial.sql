CREATE TABLE IF NOT EXISTS users(
    id INTEGER PRIMARY KEY NOT NULL,
    name TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS videos (
    path TEXT PRIMARY KEY NOT NULL,
    last_viewed DATE,
    count_views INT DEFAULT 0
);

CREATE TABLE IF NOT EXISTS downloads (
    link TEXT PRIMARY KEY NOT NULL,
    engine INT NOT NULL,
    added DATETIME NOT NULL,
    process_id INT
);