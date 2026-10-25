CREATE TABLE playlists (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL CHECK(length(trim(name)) BETWEEN 1 AND 100),
    created_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE playlist_items (
    playlist_id TEXT NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    video_checksum INTEGER NOT NULL REFERENCES video_details(checksum) ON UPDATE CASCADE ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK(position >= 0),
    PRIMARY KEY (playlist_id, video_checksum)
);

CREATE INDEX idx_playlist_items_order ON playlist_items(playlist_id, position);
CREATE INDEX idx_playlist_items_video ON playlist_items(video_checksum);
