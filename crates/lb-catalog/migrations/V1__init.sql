-- Schema V1 (SPEC §5.2). Connection pragmas (WAL, foreign_keys) are applied per connection in
-- `db.rs`; they cannot run inside a migration transaction. Migrations are append-only.

CREATE TABLE roots (               -- folders configured in Folder Manager
  id           INTEGER PRIMARY KEY,
  path         TEXT NOT NULL UNIQUE,     -- absolute, normalized (§6.1.3)
  mode         TEXT NOT NULL CHECK (mode IN ('watch','once','exclude')),
  added_at     TEXT NOT NULL
);

CREATE TABLE folders (
  id           INTEGER PRIMARY KEY,
  root_id      INTEGER NOT NULL REFERENCES roots(id) ON DELETE CASCADE,
  path         TEXT NOT NULL UNIQUE,
  parent_id    INTEGER REFERENCES folders(id) ON DELETE CASCADE,
  name         TEXT NOT NULL,
  caption      TEXT,                     -- folder description (Picasa "folder properties")
  sort_date    TEXT                      -- earliest taken_at in folder; maintained by trigger/job
);

CREATE TABLE photos (
  id            INTEGER PRIMARY KEY,
  folder_id     INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
  file_name     TEXT NOT NULL,
  media_type    TEXT NOT NULL CHECK (media_type IN ('image','video')),
  format        TEXT NOT NULL,           -- jpeg|png|gif|webp|tiff|bmp|heif|raw|mp4|mov|...
  file_size     INTEGER NOT NULL,
  file_mtime    INTEGER NOT NULL,        -- unix seconds
  quick_key     TEXT NOT NULL,           -- blake3(size || first 64KiB || last 64KiB), hex
  content_hash  TEXT,                    -- full blake3, hex; NULL until hashed
  width         INTEGER, height INTEGER, -- after EXIF orientation
  orientation   INTEGER NOT NULL DEFAULT 1,
  taken_at      TEXT,                    -- ISO-8601 local w/o tz if unknown; see §6.2.2
  taken_at_src  TEXT CHECK (taken_at_src IN ('exif','video','filename','mtime')),
  camera_make   TEXT, camera_model TEXT, lens TEXT,
  iso INTEGER, f_number REAL, exposure_s REAL, focal_mm REAL,
  gps_lat REAL, gps_lon REAL, gps_alt REAL,
  duration_ms   INTEGER,                 -- video
  caption       TEXT,
  starred       INTEGER NOT NULL DEFAULT 0,
  rating        INTEGER NOT NULL DEFAULT 0 CHECK (rating BETWEEN 0 AND 5),
  hidden        INTEGER NOT NULL DEFAULT 0,
  edit_version  INTEGER NOT NULL DEFAULT 0,   -- increments on every edit-stack change
  phash         INTEGER,                 -- 64-bit perceptual hash
  status        TEXT NOT NULL DEFAULT 'ok' CHECK (status IN ('ok','unreadable','missing')),
  error         TEXT,
  indexed_at    TEXT NOT NULL,
  UNIQUE (folder_id, file_name)
);
CREATE INDEX photos_taken_at ON photos(taken_at);
CREATE INDEX photos_quick_key ON photos(quick_key);
CREATE INDEX photos_content_hash ON photos(content_hash);
CREATE INDEX photos_starred ON photos(starred) WHERE starred = 1;

CREATE TABLE edits (
  photo_id    INTEGER PRIMARY KEY REFERENCES photos(id) ON DELETE CASCADE,
  ops_json    TEXT NOT NULL,             -- EditStack serialized (§6.6)
  redo_json   TEXT NOT NULL DEFAULT '[]',
  updated_at  TEXT NOT NULL
);

CREATE TABLE albums (
  id INTEGER PRIMARY KEY, name TEXT NOT NULL, description TEXT,
  cover_photo_id INTEGER REFERENCES photos(id) ON DELETE SET NULL,
  created_at TEXT NOT NULL, sort_order INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE album_items (
  album_id INTEGER NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
  photo_id INTEGER NOT NULL REFERENCES photos(id) ON DELETE CASCADE,
  position REAL NOT NULL,                -- fractional ordering for cheap reorder
  PRIMARY KEY (album_id, photo_id)
);

CREATE TABLE tags (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE COLLATE NOCASE);
CREATE TABLE photo_tags (
  photo_id INTEGER NOT NULL REFERENCES photos(id) ON DELETE CASCADE,
  tag_id   INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY (photo_id, tag_id)
);

CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT NOT NULL, hidden INTEGER NOT NULL DEFAULT 0);
CREATE TABLE faces (
  id INTEGER PRIMARY KEY,
  photo_id INTEGER NOT NULL REFERENCES photos(id) ON DELETE CASCADE,
  x REAL NOT NULL, y REAL NOT NULL, w REAL NOT NULL, h REAL NOT NULL,   -- normalized [0,1] in oriented image
  score REAL NOT NULL,
  embedding BLOB,                        -- f32 little-endian, L2-normalized
  cluster_id INTEGER,
  person_id INTEGER REFERENCES people(id) ON DELETE SET NULL,
  source TEXT NOT NULL CHECK (source IN ('detected','manual','picasa'))
);

CREATE TABLE settings (key TEXT PRIMARY KEY, value_json TEXT NOT NULL);

CREATE TABLE jobs_log (
  id INTEGER PRIMARY KEY, kind TEXT NOT NULL, started_at TEXT, finished_at TEXT,
  status TEXT NOT NULL, detail TEXT
);

CREATE VIRTUAL TABLE photos_fts USING fts5(
  file_name, caption, tags, folder_path, camera, people, album_names,
  content='', tokenize='unicode61 remove_diacritics 2'
);
