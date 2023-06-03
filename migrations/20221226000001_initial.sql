CREATE TABLE IF NOT EXISTS video_details (
    checksum INTEGER PRIMARY KEY NOT NULL,
    video TEXT NOT NULL,
    collection TEXT NOT NULL,
    description TEXT,
    series_title TEXT,
    season TEXT,
    episode TEXT,
    episode_title TEXT,
    thumbnail TEXT,
    duration REAL,
    width INTEGER,
    height INTEGER,
    audio_tracks INTEGER,
    search_phrase TEXT,
    added TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS history (
    path TEXT PRIMARY KEY NOT NULL,
    started TIMESTAMP,
    stopped TIMESTAMP
);
