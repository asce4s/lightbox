-- Sample rows written by a "V1" application, inserted into the frozen V1 schema.
INSERT INTO roots (id, path, mode, added_at) VALUES (1, '/photos', 'watch', '2026-01-01T00:00:00');
INSERT INTO folders (id, root_id, path, parent_id, name) VALUES
  (1, 1, '/photos', NULL, 'photos'),
  (2, 1, '/photos/2021', 1, '2021');
INSERT INTO photos (id, folder_id, file_name, media_type, format, file_size, file_mtime, quick_key,
                    width, height, taken_at, taken_at_src, caption, starred, rating, status, indexed_at)
VALUES
  (1, 2, 'beach_01.jpg', 'image', 'jpeg', 1000, 1577836800, 'aa', 640, 480,
   '2021-07-01T12:00:00', 'exif', 'At the beach', 1, 4, 'ok', '2026-01-01T00:00:00'),
  (2, 2, 'beach_02.jpg', 'image', 'jpeg', 2000, 1577836800, 'bb', 640, 480,
   NULL, 'mtime', NULL, 0, 0, 'ok', '2026-01-01T00:00:00');
INSERT INTO edits (photo_id, ops_json, updated_at) VALUES (1, '[{"op":"sepia"}]', '2026-01-02T00:00:00');
INSERT INTO albums (id, name, created_at) VALUES (1, 'Summer', '2026-01-01T00:00:00');
INSERT INTO album_items (album_id, photo_id, position) VALUES (1, 1, 1.0), (1, 2, 2.0);
INSERT INTO tags (id, name) VALUES (1, 'beach');
INSERT INTO photo_tags (photo_id, tag_id) VALUES (1, 1);
INSERT INTO settings (key, value_json) VALUES ('theme', '"dark"');
