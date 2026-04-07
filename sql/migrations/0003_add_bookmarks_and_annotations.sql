CREATE TABLE IF NOT EXISTS bookmarks (
    id TEXT PRIMARY KEY,
    entity_type TEXT NOT NULL CHECK(entity_type IN ('run', 'comparison', 'source')),
    entity_id TEXT NOT NULL,
    title TEXT NOT NULL,
    note TEXT,
    target_path TEXT NOT NULL,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL,
    UNIQUE(entity_type, entity_id)
);

CREATE TABLE IF NOT EXISTS source_annotations (
    id TEXT PRIMARY KEY,
    source_id TEXT NOT NULL,
    run_id TEXT NOT NULL,
    selected_text TEXT NOT NULL,
    annotation_markdown TEXT NOT NULL,
    tag TEXT,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL,
    FOREIGN KEY(source_id) REFERENCES sources(id) ON DELETE CASCADE,
    FOREIGN KEY(run_id) REFERENCES runs(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_bookmarks_entity ON bookmarks(entity_type, entity_id);
CREATE INDEX IF NOT EXISTS idx_bookmarks_created_at ON bookmarks(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_source_annotations_source_id ON source_annotations(source_id);
CREATE INDEX IF NOT EXISTS idx_source_annotations_run_id ON source_annotations(run_id);
