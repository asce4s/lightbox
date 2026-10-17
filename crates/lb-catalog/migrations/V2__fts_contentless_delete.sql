-- SPEC §5.2 requires `photos_fts` to be a contentless index maintained incrementally. A plain
-- `content=''` table cannot delete or replace rows, so recreate it with `contentless_delete=1`
-- (SQLite >= 3.43). `Catalog::open` repopulates it after this migration runs (see `fts.rs`).
DROP TABLE photos_fts;

CREATE VIRTUAL TABLE photos_fts USING fts5(
  file_name, caption, tags, folder_path, camera, people, album_names,
  content='', contentless_delete=1, tokenize='unicode61 remove_diacritics 2'
);

-- Rows leave the index whenever a photo row disappears, including FK cascades from folders/roots.
CREATE TRIGGER photos_fts_after_delete AFTER DELETE ON photos
BEGIN
  DELETE FROM photos_fts WHERE rowid = old.id;
END;
