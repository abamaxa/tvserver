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
    state INTEGER DEFAULT 0,
    created_on TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_on TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_collection ON video_details(collection, video);

CREATE TABLE IF NOT EXISTS history (
    checksum INTEGER PRIMARY KEY NOT NULL,
    started TIMESTAMP,
    stopped TIMESTAMP
);
