CREATE TABLE face_scans (
  photo_id   INTEGER PRIMARY KEY REFERENCES photos(id) ON DELETE CASCADE,
  scanned_at TEXT NOT NULL
);
CREATE INDEX idx_faces_photo ON faces(photo_id);
CREATE INDEX idx_faces_person ON faces(person_id);
CREATE INDEX idx_faces_cluster ON faces(cluster_id);
