use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::models::{
    DetectionSource, FileChange, FileChangeKind, FileFingerprint, FileSearchResult, FileTombstone,
    SyncDelta, TemporalEntry, TemporalEventKind,
};

pub struct Database {
    pub conn: Connection,
}

impl Database {
    /// Open (or create) the THE GRID database at `path`.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path)
            .with_context(|| format!("Opening SQLite at {:?}", path))?;
        let db = Self { conn };
        db.initialize_schema()?;
        log::info!("[DB] Opened at {:?}", path);
        Ok(db)
    }

    // ── Schema ────────────────────────────────────────────────────────────

    fn initialize_schema(&self) -> Result<()> {
        self.conn.execute_batch(r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous  = NORMAL;
            PRAGMA cache_size   = -8000;

            CREATE TABLE IF NOT EXISTS files (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                device_id   TEXT    NOT NULL,
                device_name TEXT    NOT NULL DEFAULT '',
                path        TEXT    NOT NULL,
                name        TEXT    NOT NULL,
                ext         TEXT,
                size        INTEGER NOT NULL DEFAULT 0,
                modified    INTEGER,
                hash        TEXT,
                quick_hash  TEXT,
                ai_metadata TEXT,
                detected_by TEXT    NOT NULL DEFAULT 'full_scan',
                indexed_at  INTEGER NOT NULL,
                UNIQUE(device_id, path)
            );

            CREATE INDEX IF NOT EXISTS idx_files_device  ON files(device_id);
            CREATE INDEX IF NOT EXISTS idx_files_ext     ON files(ext);
            CREATE INDEX IF NOT EXISTS idx_files_quick_hash ON files(quick_hash);
            CREATE INDEX IF NOT EXISTS idx_files_modified ON files(modified DESC);

            CREATE VIRTUAL TABLE IF NOT EXISTS files_fts USING fts5(
                name,
                path,
                device_name,
                ext,
                content='files',
                content_rowid='id'
            );

            CREATE TRIGGER IF NOT EXISTS files_ai
            AFTER INSERT ON files BEGIN
                INSERT INTO files_fts(rowid, name, path, device_name, ext)
                VALUES (new.id, new.name, new.path, new.device_name, COALESCE(new.ext, ''));
            END;

            CREATE TRIGGER IF NOT EXISTS files_ad
            AFTER DELETE ON files BEGIN
                INSERT INTO files_fts(files_fts, rowid, name, path, device_name, ext)
                VALUES ('delete', old.id, old.name, old.path, old.device_name, COALESCE(old.ext, ''));
            END;

            CREATE TRIGGER IF NOT EXISTS files_au
            AFTER UPDATE ON files BEGIN
                INSERT INTO files_fts(files_fts, rowid, name, path, device_name, ext)
                VALUES ('delete', old.id, old.name, old.path, old.device_name, COALESCE(old.ext, ''));
                INSERT INTO files_fts(rowid, name, path, device_name, ext)
                VALUES (new.id, new.name, new.path, new.device_name, COALESCE(new.ext, ''));
            END;

            CREATE TABLE IF NOT EXISTS nodes (
                id           TEXT PRIMARY KEY,
                hostname     TEXT NOT NULL,
                last_sync_ts INTEGER DEFAULT 0,
                is_active    INTEGER DEFAULT 1
            );

            CREATE TABLE IF NOT EXISTS embeddings (
                file_id    INTEGER PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
                model      TEXT    NOT NULL,
                vector     BLOB    NOT NULL,
                created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS transfers (
                id        INTEGER PRIMARY KEY AUTOINCREMENT,
                direction TEXT    NOT NULL CHECK(direction IN ('sent','received')),
                peer_ip   TEXT    NOT NULL,
                filename  TEXT    NOT NULL,
                size      INTEGER,
                status    TEXT    NOT NULL DEFAULT 'pending',
                created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS index_checkpoints (
                root_path           TEXT PRIMARY KEY,
                last_indexed_path   TEXT NOT NULL,
                total_files         INTEGER NOT NULL DEFAULT 0,
                scanned_files       INTEGER NOT NULL DEFAULT 0,
                completed           BOOLEAN NOT NULL DEFAULT 0,
                updated_at          INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS index_queue (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                root_path  TEXT NOT NULL,
                dir_path   TEXT NOT NULL,
                UNIQUE(root_path, dir_path)
            );

            CREATE TABLE IF NOT EXISTS file_tombstones (
                device_id   TEXT    NOT NULL,
                path        TEXT    NOT NULL,
                size        INTEGER NOT NULL DEFAULT 0,
                modified    INTEGER,
                hash        TEXT,
                quick_hash  TEXT,
                deleted_at  INTEGER NOT NULL,
                detected_by TEXT    NOT NULL DEFAULT 'watcher',
                PRIMARY KEY(device_id, path)
            );

            CREATE INDEX IF NOT EXISTS idx_tombstones_deleted_at ON file_tombstones(deleted_at DESC);
            CREATE INDEX IF NOT EXISTS idx_tombstones_quick_hash ON file_tombstones(device_id, quick_hash, size);

            -- ── Phase 2: Rules & Smart Filters ──────────────────────────
            CREATE TABLE IF NOT EXISTS user_rules (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                name        TEXT    NOT NULL,
                pattern     TEXT    NOT NULL,  -- Glob or Regex
                project     TEXT,              -- Optional project association
                tag         TEXT,              -- Optional tag association
                is_active   INTEGER DEFAULT 1,
                created_at  INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS file_tags (
                file_id     INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
                tag         TEXT,
                project     TEXT,
                is_manual   INTEGER DEFAULT 0, -- 1 if set by user, 0 if by rule
                PRIMARY KEY(file_id, tag, project)
            );

            CREATE INDEX IF NOT EXISTS idx_file_tags_tag ON file_tags(tag);
            CREATE INDEX IF NOT EXISTS idx_file_tags_project ON file_tags(project);

            CREATE TABLE IF NOT EXISTS known_devices (
                device_id   TEXT PRIMARY KEY,
                device_name TEXT NOT NULL,
                last_seen   INTEGER NOT NULL
            );

            -- ── Hashing checkpoint (KV store for resumable hashing sessions) ──
            CREATE TABLE IF NOT EXISTS hashing_state (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            -- ── Embedding priority queue ──────────────────────────────────────
            CREATE TABLE IF NOT EXISTS embedding_queue (
                file_id    INTEGER PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
                priority   INTEGER NOT NULL DEFAULT 3,
                queued_at  INTEGER NOT NULL,
                attempts   INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_emb_queue_priority ON embedding_queue(priority ASC, queued_at ASC);

            -- ── Indexing audit log (classification decisions) ─────────────────
            CREATE TABLE IF NOT EXISTS indexing_log (
                id        INTEGER PRIMARY KEY AUTOINCREMENT,
                path      TEXT    NOT NULL,
                tier      TEXT    NOT NULL,
                reason    TEXT    NOT NULL,
                logged_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_indexing_log_logged_at ON indexing_log(logged_at DESC);

            -- ── Deletion audit (immutable safety record) ─────────────────────
            CREATE TABLE IF NOT EXISTS deletion_audit (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id  TEXT    NOT NULL,
                file_path   TEXT    NOT NULL,
                device_id   TEXT    NOT NULL,
                file_hash   TEXT,
                file_size   INTEGER,
                action      TEXT    NOT NULL,
                reason      TEXT,
                executed_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_deletion_audit_session ON deletion_audit(session_id);

            -- ── Persistent duplicate groups (cross-source, cross-device) ──────
            -- Groups survive restarts and re-scans; member review decisions are kept.
            CREATE TABLE IF NOT EXISTS duplicate_groups (
                hash             TEXT    NOT NULL,
                size             INTEGER NOT NULL,
                file_count       INTEGER NOT NULL DEFAULT 0,
                source_count     INTEGER NOT NULL DEFAULT 0,
                suggested_anchor TEXT,
                status           TEXT    NOT NULL DEFAULT 'pending',
                first_seen_at    INTEGER NOT NULL,
                last_seen_at     INTEGER NOT NULL,
                PRIMARY KEY (hash, size)
            );
            CREATE INDEX IF NOT EXISTS idx_dup_groups_status ON duplicate_groups(status);

            -- One row per file per group; stores the user's review decision.
            CREATE TABLE IF NOT EXISTS duplicate_group_members (
                hash       TEXT    NOT NULL,
                size       INTEGER NOT NULL,
                file_id    INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
                action     TEXT    NOT NULL DEFAULT 'undecided',
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (hash, size, file_id)
            );
            CREATE INDEX IF NOT EXISTS idx_dup_members_file ON duplicate_group_members(file_id);
        "#).context("Initializing database schema")?;

        // Migrate existing DBs: add new columns if missing
        self.add_column_if_missing("ALTER TABLE files ADD COLUMN quick_hash TEXT")?;
        self.add_column_if_missing("ALTER TABLE files ADD COLUMN detected_by TEXT NOT NULL DEFAULT 'full_scan'")?;
        self.add_column_if_missing("ALTER TABLE files ADD COLUMN source_type TEXT NOT NULL DEFAULT 'Local'")?;
        self.add_column_if_missing("ALTER TABLE files ADD COLUMN github_backed INTEGER NOT NULL DEFAULT 0")?;
        self.add_column_if_missing("ALTER TABLE files ADD COLUMN indexing_tier TEXT NOT NULL DEFAULT 'FullIndex'")?;
        self.add_column_if_missing("ALTER TABLE files ADD COLUMN md5_hash TEXT")?;
        log::info!("[DB] Schema ready");
        Ok(())
    }

    fn add_column_if_missing(&self, sql: &str) -> Result<()> {
        if let Err(err) = self.conn.execute(sql, []) {
            if !err.to_string().contains("duplicate column name") {
                return Err(err.into());
            }
        }
        Ok(())
    }

    // ── Device registry ───────────────────────────────────────────────────

    pub fn upsert_device(&self, device_id: &str, device_name: &str) -> Result<()> {
        let now = unix_now();
        self.conn.execute(
            "INSERT INTO known_devices (device_id, device_name, last_seen)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(device_id) DO UPDATE
               SET device_name = excluded.device_name,
                   last_seen   = excluded.last_seen",
            params![device_id, device_name, now],
        )?;
        Ok(())
    }

    // ── File indexing ─────────────────────────────────────────────────────

    pub fn index_file(
        &self,
        device_id:   &str,
        device_name: &str,
        path:        &Path,
        size:        u64,
        modified:    Option<i64>,
        hash:        Option<&str>,
    ) -> Result<i64> {
        self.index_file_with_source(
            device_id,
            device_name,
            path,
            size,
            modified,
            None,
            hash,
            DetectionSource::FullScan,
            unix_now(),
        )
    }

    pub fn index_file_with_source(
        &self,
        device_id:   &str,
        device_name: &str,
        path:        &Path,
        size:        u64,
        modified:    Option<i64>,
        quick_hash:  Option<&str>,
        hash:        Option<&str>,
        detected_by: DetectionSource,
        indexed_at:  i64,
    ) -> Result<i64> {
        let name = path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let ext = path.extension()
            .map(|e| e.to_string_lossy().to_lowercase());
        let path_str = path.to_string_lossy();

        self.clear_tombstone_if_older(device_id, path, indexed_at)?;

        self.conn.execute(
            "INSERT INTO files (device_id, device_name, path, name, ext, size, modified, hash, quick_hash, detected_by, indexed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(device_id, path) DO UPDATE
               SET name        = excluded.name,
                   ext         = excluded.ext,
                   size        = excluded.size,
                   modified    = excluded.modified,
                   hash        = CASE
                                   WHEN excluded.hash IS NOT NULL THEN excluded.hash
                                   WHEN files.size != excluded.size OR COALESCE(files.modified, -1) != COALESCE(excluded.modified, -1)
                                     THEN NULL
                                   ELSE files.hash
                                 END,
                   quick_hash  = COALESCE(excluded.quick_hash, files.quick_hash),
                   ai_metadata = CASE
                                   WHEN files.size != excluded.size OR COALESCE(files.modified, -1) != COALESCE(excluded.modified, -1)
                                     THEN NULL
                                   ELSE files.ai_metadata
                                 END,
                   detected_by = excluded.detected_by,
                   indexed_at  = excluded.indexed_at",
            params![
                device_id,
                device_name,
                path_str.as_ref(),
                name,
                ext.as_deref(),
                size as i64,
                modified,
                hash,
                quick_hash,
                detected_by.as_str(),
                indexed_at,
            ],
        )?;

        let id = self.conn.query_row(
            "SELECT id FROM files WHERE device_id = ?1 AND path = ?2",
            params![device_id, path_str.as_ref()],
            |row| row.get(0),
        )?;
        Ok(id)
    }

    pub fn update_file_hash(&self, id: i64, hash: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE files SET hash = ?1 WHERE id = ?2",
            params![hash, id]
        )?;
        Ok(())
    }

    pub fn update_ai_metadata(&self, id: i64, metadata_json: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE files SET ai_metadata = ?1 WHERE id = ?2",
            params![metadata_json, id]
        )?;
        Ok(())
    }

    pub fn get_duplicate_groups(&self) -> Result<Vec<(String, u64, Vec<FileSearchResult>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT hash, size FROM files 
             WHERE hash IS NOT NULL AND hash != '' AND hash NOT LIKE 'ERR_%'
             GROUP BY hash, size 
             HAVING COUNT(*) > 1"
        )?;

        let groups = stmt.query_map([], |row| {
            let hash: String = row.get(0)?;
            let size: i64    = row.get(1)?;
            Ok((hash, size as u64))
        })?;

        let mut results = Vec::new();
        for g in groups {
            let (hash, size) = g?;
            let mut file_stmt = self.conn.prepare(
                "SELECT id, device_id, device_name, path, name, ext, size, modified, hash, quick_hash, indexed_at, detected_by, 0.0 as rank 
                 FROM files WHERE hash = ?1 AND size = ?2"
            )?;
            let files = file_stmt.query_map(params![hash, size as i64], |r| self.map_search_result(r))?;
            let mut file_list = Vec::new();
            for f in files { file_list.push(f?); }
            results.push((hash, size, file_list));
        }
        Ok(results)
    }

    /// Cross-device duplicate stats for a specific machine vector.
    /// Uses cached mesh index data, so peers do not need to be online.
    pub fn crosscheck_duplicates_for_device(&self, target_device_id: &str) -> Result<(u64, u64, u64, u64)> {
        let mut stmt = self.conn.prepare(
            "SELECT hash, size,
                    COUNT(*) AS total_files,
                    SUM(CASE WHEN device_id = ?1 THEN 1 ELSE 0 END) AS target_files,
                    COUNT(DISTINCT device_id) AS distinct_devices
             FROM files
             WHERE hash IS NOT NULL AND hash != '' AND hash NOT LIKE 'ERR_%'
             GROUP BY hash, size
             HAVING target_files > 0 AND distinct_devices > 1 AND total_files > 1"
        )?;

        let rows = stmt.query_map(params![target_device_id], |row| {
            let size: i64 = row.get(1)?;
            let target_files: i64 = row.get(3)?;
            Ok((size as u64, target_files as u64))
        })?;

        let mut groups = 0u64;
        let mut files = 0u64;
        let mut bytes = 0u64;
        for row in rows {
            let (size, target_files) = row?;
            groups += 1;
            files = files.saturating_add(target_files);
            bytes = bytes.saturating_add(size.saturating_mul(target_files));
        }

        let known_devices = self.conn.query_row(
            "SELECT COUNT(DISTINCT device_id) FROM files",
            [],
            |r| r.get::<_, i64>(0),
        ).unwrap_or(0);

        Ok((groups, files, bytes, known_devices.max(0) as u64))
    }

    pub fn delete_file_by_id(&self, id: i64) -> Result<()> {
        let _ = self.conn.execute("DELETE FROM embeddings WHERE file_id = ?1", params![id]);
        self.conn.execute("DELETE FROM files WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn move_file(&self, id: i64, new_path: PathBuf) -> Result<()> {
        let path_str = new_path.to_string_lossy();
        let name = new_path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let ext = new_path.extension()
            .map(|e| e.to_string_lossy().to_lowercase());

        self.conn.execute(
            "UPDATE files SET path = ?1, name = ?2, ext = ?3 WHERE id = ?4",
            params![path_str.as_ref(), name, ext, id]
        )?;
        Ok(())
    }

    // ── Phase 2: Rules & Smart Filters ────────────────────────────────────

    pub fn add_rule(&self, name: &str, pattern: &str, project: Option<&str>, tag: Option<&str>) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO user_rules (name, pattern, project, tag, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![name, pattern, project, tag, unix_now()]
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_rules(&self) -> Result<Vec<(i64, String, String, Option<String>, Option<String>, bool)>> {
        let mut stmt = self.conn.prepare("SELECT id, name, pattern, project, tag, is_active FROM user_rules")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get::<_, i32>(5)? != 0
            ))
        })?;
        let mut out = Vec::new();
        for r in rows { out.push(r?); }
        Ok(out)
    }

    pub fn delete_rule(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM user_rules WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn add_file_tag(&self, file_id: i64, tag: Option<&str>, project: Option<&str>, is_manual: bool) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO file_tags (file_id, tag, project, is_manual) VALUES (?1, ?2, ?3, ?4)",
            params![file_id, tag, project, if is_manual { 1 } else { 0 }]
        )?;
        Ok(())
    }

    pub fn get_file_tags(&self, file_id: i64) -> Result<Vec<(Option<String>, Option<String>)>> {
        let mut stmt = self.conn.prepare("SELECT tag, project FROM file_tags WHERE file_id = ?1")?;
        let rows = stmt.query_map(params![file_id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
        let mut out = Vec::new();
        for r in rows { out.push(r?); }
        Ok(out)
    }

    pub fn remove_all_file_tags(&self, file_id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM file_tags WHERE file_id = ?1", params![file_id])?;
        Ok(())
    }

    pub fn get_file_id_by_path(&self, device_id: &str, path: &Path) -> Result<Option<i64>> {
        let path_str = path.to_string_lossy().to_string();
        let id = self.conn.query_row(
            "SELECT id FROM files WHERE device_id = ?1 AND path = ?2",
            params![device_id, path_str],
            |row| row.get(0),
        ).optional()?;
        Ok(id)
    }

    pub fn get_sync_delta_after(&self, after: i64) -> Result<SyncDelta> {
        self.get_sync_delta_after_filtered(after, None)
    }

    pub fn get_sync_delta_after_filtered(&self, after: i64, requester_device: Option<&str>) -> Result<SyncDelta> {
        let requester = requester_device.unwrap_or_default().trim();
        let mut stmt = self.conn.prepare(
            "SELECT id, device_id, device_name, path, name, ext, size, modified, hash, quick_hash, indexed_at, detected_by, 0.0 as rank \
             FROM files
             WHERE (indexed_at > ?1 OR modified > ?1)
               AND (?2 = '' OR device_id != ?2)"
        )?;
        let rows = stmt.query_map(params![after, requester], |row| self.map_search_result(row))?;
        let mut files = Vec::new();
        for r in rows { files.push(r?); }

        let mut tomb_stmt = self.conn.prepare(
            "SELECT device_id, path, size, modified, hash, quick_hash, deleted_at, detected_by
             FROM file_tombstones
             WHERE deleted_at > ?1
               AND (?2 = '' OR device_id != ?2)"
        )?;
        let tomb_rows = tomb_stmt.query_map(params![after, requester], |row| {
            Ok(FileTombstone {
                device_id: row.get(0)?,
                path: PathBuf::from(row.get::<_, String>(1)?),
                size: row.get::<_, i64>(2)? as u64,
                modified: row.get(3)?,
                hash: row.get(4)?,
                quick_hash: row.get(5)?,
                deleted_at: row.get(6)?,
                detected_by: row.get::<_, Option<String>>(7)?
                    .as_deref()
                    .map(DetectionSource::from_db)
                    .unwrap_or(DetectionSource::Watcher),
            })
        })?;
        let mut tombstones = Vec::new();
        for row in tomb_rows { tombstones.push(row?); }

        Ok(SyncDelta { files, tombstones })
    }

    fn map_search_result(&self, row: &rusqlite::Row) -> rusqlite::Result<FileSearchResult> {
        Ok(FileSearchResult {
            id:          row.get(0)?,
            device_id:   row.get(1)?,
            device_name: row.get(2)?,
            path:        PathBuf::from(row.get::<_, String>(3)?),
            name:        row.get(4)?,
            ext:         row.get(5)?,
            size:        row.get::<_, i64>(6)? as u64,
            modified:    row.get(7)?,
            hash:        row.get(8)?,
            quick_hash:  row.get(9)?,
            indexed_at:  row.get(10)?,
            detected_by: row.get::<_, Option<String>>(11)?
                .as_deref()
                .map(DetectionSource::from_db)
                .unwrap_or_default(),
            rank:        row.get(12)?,
        })
    }


    pub fn get_node_sync_ts(&self, node_id: &str) -> Result<i64> {
        let res: Result<i64, rusqlite::Error> = self.conn.query_row(
            "SELECT last_sync_ts FROM nodes WHERE id = ?1",
            params![node_id],
            |row| row.get(0),
        );
        
        match res {
            Ok(ts) => Ok(ts),
            Err(_) => Ok(0),
        }
    }

    pub fn update_node_sync_ts(&self, node_id: &str, hostname: &str, ts: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO nodes (id, hostname, last_sync_ts, is_active) VALUES (?1, ?2, ?3, 1)
             ON CONFLICT(id) DO UPDATE SET last_sync_ts = excluded.last_sync_ts, hostname = excluded.hostname, is_active = 1",
            params![node_id, hostname, ts],
        )?;
        Ok(())
    }

    pub fn remove_path(&self, device_id: &str, path: &Path, detected_by: DetectionSource) -> Result<usize> {
        let path_str = path.to_string_lossy().to_string();
        let rows = self.collect_file_rows_for_path(device_id, &path_str)?;
        let deleted_at = unix_now();

        for row in &rows {
            self.insert_tombstone(&FileTombstone {
                device_id: device_id.to_string(),
                path: PathBuf::from(&row.path),
                size: row.size,
                modified: row.modified,
                hash: row.hash.clone(),
                quick_hash: row.quick_hash.clone(),
                deleted_at,
                detected_by,
            })?;
        }

        let (exact, win_like, unix_like) = path_match_patterns(&path_str);
        let deleted = self.conn.execute(
            "DELETE FROM files
             WHERE device_id = ?1
               AND (path = ?2 OR path LIKE ?3 OR path LIKE ?4)",
            params![device_id, exact, win_like, unix_like],
        )?;
        Ok(deleted)
    }

    // ── Phase 4: Semantic AI ──────────────────────────────────────────────

    pub fn get_files_needing_embedding(&self, limit: usize) -> Result<Vec<(i64, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name || ' ' || COALESCE(ext, '') FROM files 
             WHERE id NOT IN (SELECT file_id FROM embeddings)
             LIMIT ?1"
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        
        let mut out = Vec::new();
        for r in rows { out.push(r?); }
        Ok(out)
    }

    pub fn count_unindexed_files(&self) -> Result<usize> {
        let n: usize = self.conn.query_row(
            "SELECT COUNT(*) FROM files WHERE id NOT IN (SELECT file_id FROM embeddings)",
            [],
            |row| row.get(0)
        )?;
        Ok(n)
    }

    pub fn count_files_needing_hash(&self) -> Result<usize> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM files WHERE (hash IS NULL OR hash = '') AND path IS NOT NULL",
            [],
            |r| r.get(0),
        )?;
        Ok(n.max(0) as usize)
    }

    /// Returns (total_files, total_size_bytes, device_count) for the storage overview command.
    pub fn get_storage_stats(&self) -> Result<(u64, u64, u64)> {
        let total_files: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM files", [], |r| r.get(0),
        ).unwrap_or(0);
        let total_size: i64 = self.conn.query_row(
            "SELECT COALESCE(SUM(size), 0) FROM files WHERE size IS NOT NULL", [], |r| r.get(0),
        ).unwrap_or(0);
        let device_count: i64 = self.conn.query_row(
            "SELECT COUNT(DISTINCT device_id) FROM files", [], |r| r.get(0),
        ).unwrap_or(0);
        Ok((total_files.max(0) as u64, total_size.max(0) as u64, device_count.max(0) as u64))
    }

    pub fn get_files_needing_hash(&self, limit: usize) -> Result<Vec<(i64, PathBuf)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path FROM files 
             WHERE hash IS NULL OR hash = ''
             LIMIT ?1"
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            let id: i64 = row.get(0)?;
            let path: String = row.get(1)?;
            Ok((id, PathBuf::from(path)))
        })?;
        let mut out = Vec::new();
        for r in rows { out.push(r?); }
        Ok(out)
    }

    pub fn get_files_needing_media_ai(&self, limit: usize) -> Result<Vec<(i64, PathBuf)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path FROM files 
             WHERE (ai_metadata IS NULL OR ai_metadata = '')
                             AND (ext IN ('jpg', 'jpeg', 'png', 'webp', 'gif', 'bmp', 'tif', 'tiff', 'mp4', 'mkv', 'mov', 'avi'))
             LIMIT ?1"
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            let id: i64 = row.get(0)?;
            let path_str: String = row.get(1)?;
            Ok((id, PathBuf::from(path_str)))
        })?;
        
        let mut out = Vec::new();
        for r in rows { out.push(r?); }
        Ok(out)
    }

    pub fn save_embedding(&self, file_id: i64, model: &str, vector: &[f32]) -> Result<()> {
        let blob = unsafe {
            std::slice::from_raw_parts(
                vector.as_ptr() as *const u8,
                vector.len() * std::mem::size_of::<f32>()
            )
        };
        self.conn.execute(
            "INSERT OR REPLACE INTO embeddings (file_id, model, vector, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![file_id, model, blob, unix_now()],
        )?;
        Ok(())
    }

    pub fn get_all_embeddings(&self) -> Result<Vec<(i64, Vec<f32>)>> {
        let mut stmt = self.conn.prepare("SELECT file_id, vector FROM embeddings")?;
        let rows = stmt.query_map([], |row| {
            let blob: Vec<u8> = row.get(1)?;
            let vector: Vec<f32> = unsafe {
                std::slice::from_raw_parts(
                    blob.as_ptr() as *const f32,
                    blob.len() / std::mem::size_of::<f32>()
                ).to_vec()
            };
            Ok((row.get(0)?, vector))
        })?;
        
        let mut out = Vec::new();
        for r in rows { out.push(r?); }
        Ok(out)
    }

    pub fn get_files_by_ids(&self, ids: &[i64]) -> Result<Vec<FileSearchResult>> {
        if ids.is_empty() { return Ok(vec![]); }
        
        let id_params: Vec<String> = (0..ids.len()).map(|_| "?".to_string()).collect();
        let sql = format!(
            "SELECT id, device_id, device_name, path, name, ext, size, modified, hash, quick_hash, indexed_at, detected_by, 0.0 as rank
             FROM files WHERE id IN ({})",
            id_params.join(",")
        );
        
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(ids), |row| self.map_search_result(row))?;
        
        let mut out = Vec::new();
        for r in rows { out.push(r?); }
        
        let mut sorted = Vec::with_capacity(ids.len());
        for &id in ids {
            if let Some(found) = out.iter().find(|f| f.id == id) {
                sorted.push(found.clone());
            }
        }
        Ok(sorted)
    }

    pub fn enqueue_index_root(&self, root: &Path) -> Result<()> {
        if self.has_pending_index_root(root)? {
            return Ok(());
        }
        self.conn.execute(
            "INSERT INTO index_queue (root_path, dir_path) VALUES (?, ?)",
            [root.to_string_lossy(), root.to_string_lossy()]
        )?;
        self.conn.execute(
            "INSERT OR REPLACE INTO index_checkpoints (root_path, last_indexed_path, completed, updated_at) VALUES (?, ?, 0, ?)",
            params![root.to_string_lossy(), root.to_string_lossy(), SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64]
        )?;
        Ok(())
    }

    pub fn has_pending_index_tasks(&self) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(1) FROM index_queue",
            [],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn pending_index_task_count(&self) -> Result<u64> {
        self.pending_index_task_count_for_root(None)
    }

    pub fn pending_index_task_count_for_root(&self, root: Option<&str>) -> Result<u64> {
        let (sql, use_param) = if root.is_some() {
            ("SELECT COUNT(1) FROM index_queue WHERE root_path = ?1", true)
        } else {
            ("SELECT COUNT(1) FROM index_queue", false)
        };

        let count: i64 = if use_param {
            self.conn.query_row(sql, params![root.unwrap_or_default()], |row| row.get(0))?
        } else {
            self.conn.query_row(sql, [], |row| row.get(0))?
        };

        Ok(count.max(0) as u64)
    }

    pub fn has_pending_index_root(&self, root: &Path) -> Result<bool> {
        let root_str = root.to_string_lossy().to_string();
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(1) FROM index_queue WHERE root_path = ?1",
            [root_str],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn get_next_index_task(&self) -> Result<Option<(String, String)>> {
        self.get_next_index_task_for_root(None)
    }

    pub fn claim_next_index_task(&self) -> Result<Option<(String, String)>> {
        self.claim_next_index_task_for_root(None)
    }

    pub fn get_next_index_task_for_root(&self, root: Option<&str>) -> Result<Option<(String, String)>> {
        let (sql, use_param) = if root.is_some() {
            ("SELECT root_path, dir_path FROM index_queue WHERE root_path = ?1 LIMIT 1", true)
        } else {
            ("SELECT root_path, dir_path FROM index_queue LIMIT 1", false)
        };

        let mut stmt = self.conn.prepare(sql)?;
        let mut rows = if use_param {
            stmt.query(params![root.unwrap_or_default()])?
        } else {
            stmt.query([])?
        };

        if let Some(row) = rows.next()? {
            Ok(Some((row.get(0)?, row.get(1)?)))
        } else {
            Ok(None)
        }
    }

    pub fn claim_next_index_task_for_root(&self, root: Option<&str>) -> Result<Option<(String, String)>> {
        let (sql, use_param) = if root.is_some() {
            ("SELECT rowid, root_path, dir_path FROM index_queue WHERE root_path = ?1 LIMIT 1", true)
        } else {
            ("SELECT rowid, root_path, dir_path FROM index_queue LIMIT 1", false)
        };

        let mut stmt = self.conn.prepare(sql)?;
        let mut rows = if use_param {
            stmt.query(params![root.unwrap_or_default()])?
        } else {
            stmt.query([])?
        };

        if let Some(row) = rows.next()? {
            let rowid: i64 = row.get(0)?;
            let root_path: String = row.get(1)?;
            let dir_path: String = row.get(2)?;
            self.conn.execute("DELETE FROM index_queue WHERE rowid = ?1", params![rowid])?;
            Ok(Some((root_path, dir_path)))
        } else {
            Ok(None)
        }
    }

    pub fn complete_index_task(&self, root: &str, dir: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM index_queue WHERE root_path = ? AND dir_path = ?",
            [root, dir]
        )?;
        Ok(())
    }

    pub fn index_directory<F>(
        &self,
        device_id:   &str,
        device_name: &str,
        root_dir:    &Path,
        mut progress_cb: F,
    ) -> Result<u64>
    where
        F: FnMut(u64, &Path, Option<&str>),
    {
        // For 'spawn_index_directory' which calls this directly for full crawl:
        // We ensure the root is in the queue if it's the first run, but for simplicity
        // this method still uses a stack but yields to persistence every N files?
        // No, let's keep this as the WORKER'S INNER LOOP.
        
        let mut count = 0u64;
        let root_str = root_dir.to_string_lossy();
        
        // Load checkpoint if exists
        let mut stack = vec![root_dir.to_path_buf()];
        
        while let Some(dir) = stack.pop() {
            let entries = match std::fs::read_dir(&dir) {
                Ok(e)  => e,
                Err(e) => {
                    log::warn!("[DB] Skipping unreadable dir {:?}: {}", dir, e);
                    continue;
                }
            };

            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if should_skip_path(&path) {
                    continue;
                }
                let meta = match entry.metadata() {
                    Ok(m)  => m,
                    Err(_) => continue,
                };

                if meta.is_dir() {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    if should_skip_dir(&name) { continue; }
                    stack.push(path);
                } else if meta.is_file() {
                    let size = meta.len();
                    let modified = meta.modified().ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map(|d| d.as_secs() as i64);

                    let quick_hash = crate::utils::quick_hash_file(&path).ok();
                    if let Err(e) = self.index_file_with_source(
                        device_id,
                        device_name,
                        &path,
                        size,
                        modified,
                        quick_hash.as_deref(),
                        None,
                        DetectionSource::FullScan,
                        unix_now(),
                    ) {
                        log::warn!("[DB] Failed to index {:?}: {}", path, e);
                    }

                    count += 1;
                    let ext_str = path.extension().and_then(|e| e.to_str());
                    progress_cb(count, &path, ext_str);
                    
                    // Persistence Point: Update checkpoint every 100 files
                    if count % 100 == 0 {
                        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
                        self.conn.execute(
                            "INSERT OR REPLACE INTO index_checkpoints (root_path, last_indexed_path, scanned_files, updated_at) VALUES (?, ?, ?, ?)",
                            params![root_str.as_ref(), path.to_string_lossy(), count, now]
                        )?;
                    }
                }
            }
        }

        // Final completion mark
        self.conn.execute(
            "UPDATE index_checkpoints SET completed = 1, updated_at = ? WHERE root_path = ?",
            params![SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64, root_str.as_ref()]
        )?;
        
        log::info!("[DB] Indexed {} files under {:?}", count, root_dir);
        Ok(count)
    }

    pub fn index_changed_paths(
        &self,
        device_id:   &str,
        device_name: &str,
        changes:     &[FileChange],
    ) -> Result<(usize, usize, usize)> {
        let mut updated = 0;
        let mut deleted = 0;
        let mut renamed = 0;

        for change in changes {
            match change.kind {
                FileChangeKind::Deleted => {
                    deleted += self.remove_path(device_id, &change.path, DetectionSource::Watcher)?;
                }
                FileChangeKind::Renamed => {
                    if let Some(old_path) = change.old_path.as_ref() {
                        let new_path = change.new_path.as_ref().unwrap_or(&change.path);
                        renamed += self.rename_path_tree(device_id, old_path, new_path, DetectionSource::Watcher)?;
                        if new_path.exists() && new_path.is_file() {
                            let fingerprint = change.fingerprint.clone().or_else(|| crate::utils::fingerprint_file(new_path).ok());
                            if let Some(identity) = fingerprint {
                                if self.index_file_with_source(
                                    device_id,
                                    device_name,
                                    new_path,
                                    identity.size,
                                    identity.modified,
                                    identity.quick_hash.as_deref(),
                                    None,
                                    DetectionSource::Watcher,
                                    unix_now(),
                                ).is_ok() {
                                    updated += 1;
                                }
                            }
                        }
                    }
                }
                FileChangeKind::Created | FileChangeKind::Modified => {
                    let path = &change.path;
                    if !path.exists() {
                        deleted += self.remove_path(device_id, path, DetectionSource::Watcher)?;
                        continue;
                    }

                    if !path.is_file() {
                        continue;
                    }

                    let fingerprint = change.fingerprint.clone().or_else(|| crate::utils::fingerprint_file(path).ok());
                    let Some(identity) = fingerprint else {
                        continue;
                    };

                    let correlated_tombstone = if change.kind == FileChangeKind::Created {
                        self.find_recent_tombstone(device_id, &identity, 8)?
                    } else {
                        None
                    };

                    if let Some(ref tombstone) = correlated_tombstone {
                        if tombstone.path != *path {
                            renamed += 1;
                        }
                        self.remove_tombstone(device_id, &tombstone.path)?;
                    }

                    if self.index_file_with_source(
                        device_id,
                        device_name,
                        path,
                        identity.size,
                        identity.modified,
                        identity.quick_hash.as_deref(),
                        correlated_tombstone.as_ref().and_then(|t| t.hash.as_deref()),
                        DetectionSource::Watcher,
                        unix_now(),
                    ).is_ok() {
                        updated += 1;
                    }
                }
            }
        }

        Ok((updated, deleted, renamed))
    }

    // ── Search ────────────────────────────────────────────────────────────

    pub fn search_fts(
        &self,
        query: &str,
        limit: usize,
        device_filter: Option<&str>,
    ) -> Result<Vec<FileSearchResult>> {
        if query.trim().is_empty() { return Ok(vec![]); }

        let safe_query = sanitize_fts_query(query);

        let sql = if device_filter.is_some() {
            "SELECT f.id, f.device_id, f.device_name, f.path, f.name,
                                        f.ext, f.size, f.modified, f.hash, f.quick_hash, f.indexed_at, f.detected_by, fts.rank
             FROM files_fts fts
             JOIN files f ON f.id = fts.rowid
             WHERE files_fts MATCH ?1
               AND f.device_id = ?2
             ORDER BY fts.rank
             LIMIT ?3"
        } else {
            "SELECT f.id, f.device_id, f.device_name, f.path, f.name,
                    f.ext, f.size, f.modified, f.hash, f.quick_hash, f.indexed_at, f.detected_by, fts.rank
             FROM files_fts fts
             JOIN files f ON f.id = fts.rowid
             WHERE files_fts MATCH ?1
             ORDER BY fts.rank
             LIMIT ?2"
        };

        let mut stmt = self.conn.prepare(sql)?;

        let map_row = |row: &rusqlite::Row| -> rusqlite::Result<FileSearchResult> {
            let path_str: String = row.get(3)?;
            Ok(FileSearchResult {
                id:          row.get(0)?,
                device_id:   row.get(1)?,
                device_name: row.get(2)?,
                path:        PathBuf::from(path_str),
                name:        row.get(4)?,
                ext:         row.get(5)?,
                size:        row.get::<_, i64>(6)? as u64,
                modified:    row.get(7)?,
                hash:        row.get(8)?,
                quick_hash:  row.get(9)?,
                indexed_at:  row.get(10)?,
                detected_by: row.get::<_, Option<String>>(11)?
                    .as_deref()
                    .map(DetectionSource::from_db)
                    .unwrap_or_default(),
                rank:        row.get(12)?,
            })
        };

        let results: Vec<FileSearchResult> = if let Some(dev) = device_filter {
            stmt.query_map(params![safe_query, dev, limit as i64], map_row)?
                .filter_map(|r: rusqlite::Result<FileSearchResult>| r.ok())
                .collect()
        } else {
            stmt.query_map(params![safe_query, limit as i64], map_row)?
                .filter_map(|r: rusqlite::Result<FileSearchResult>| r.ok())
                .collect()
        };

        Ok(results)
    }

    pub fn search_fts_with_media_filters(
        &self,
        query: &str,
        limit: usize,
        device_filter: Option<&str>,
        in_focus: Option<bool>,
        min_quality: Option<f32>,
        min_focus_score: Option<f32>,
        min_megapixels: Option<f32>,
    ) -> Result<Vec<FileSearchResult>> {
        let focus_param: Option<i64> = in_focus.map(|v| if v { 1 } else { 0 });

        let mut out = Vec::new();
        if query.trim().is_empty() {
            // Filter-only mode: no FTS text query required.
            let sql = if device_filter.is_some() {
                "SELECT id, device_id, device_name, path, name, ext, size, modified, hash, quick_hash, indexed_at, detected_by, 0.0 as rank
                 FROM files
                 WHERE device_id = ?1
                   AND ai_metadata IS NOT NULL AND ai_metadata != ''
                   AND (?2 IS NULL OR CAST(json_extract(ai_metadata, '$.in_focus') AS INTEGER) = ?2)
                   AND (?3 IS NULL OR CAST(json_extract(ai_metadata, '$.quality_score') AS REAL) >= ?3)
                   AND (?4 IS NULL OR CAST(json_extract(ai_metadata, '$.focus_score') AS REAL) >= ?4)
                   AND (?5 IS NULL OR CAST(json_extract(ai_metadata, '$.megapixels') AS REAL) >= ?5)
                 ORDER BY indexed_at DESC
                 LIMIT ?6"
            } else {
                "SELECT id, device_id, device_name, path, name, ext, size, modified, hash, quick_hash, indexed_at, detected_by, 0.0 as rank
                 FROM files
                 WHERE ai_metadata IS NOT NULL AND ai_metadata != ''
                   AND (?1 IS NULL OR CAST(json_extract(ai_metadata, '$.in_focus') AS INTEGER) = ?1)
                   AND (?2 IS NULL OR CAST(json_extract(ai_metadata, '$.quality_score') AS REAL) >= ?2)
                   AND (?3 IS NULL OR CAST(json_extract(ai_metadata, '$.focus_score') AS REAL) >= ?3)
                   AND (?4 IS NULL OR CAST(json_extract(ai_metadata, '$.megapixels') AS REAL) >= ?4)
                 ORDER BY indexed_at DESC
                 LIMIT ?5"
            };

            let mut stmt = self.conn.prepare(sql)?;
            if let Some(dev) = device_filter {
                let rows = stmt.query_map(
                    params![dev, focus_param, min_quality, min_focus_score, min_megapixels, limit as i64],
                    |row| self.map_search_result(row),
                )?;
                for r in rows {
                    out.push(r?);
                }
            } else {
                let rows = stmt.query_map(
                    params![focus_param, min_quality, min_focus_score, min_megapixels, limit as i64],
                    |row| self.map_search_result(row),
                )?;
                for r in rows {
                    out.push(r?);
                }
            }
            return Ok(out);
        }

        let safe_query = sanitize_fts_query(query);
        let sql = if device_filter.is_some() {
            "SELECT f.id, f.device_id, f.device_name, f.path, f.name,
                    f.ext, f.size, f.modified, f.hash, f.quick_hash, f.indexed_at, f.detected_by, fts.rank
             FROM files_fts fts
             JOIN files f ON f.id = fts.rowid
             WHERE files_fts MATCH ?1
               AND f.device_id = ?2
               AND f.ai_metadata IS NOT NULL AND f.ai_metadata != ''
               AND (?3 IS NULL OR CAST(json_extract(f.ai_metadata, '$.in_focus') AS INTEGER) = ?3)
               AND (?4 IS NULL OR CAST(json_extract(f.ai_metadata, '$.quality_score') AS REAL) >= ?4)
               AND (?5 IS NULL OR CAST(json_extract(f.ai_metadata, '$.focus_score') AS REAL) >= ?5)
               AND (?6 IS NULL OR CAST(json_extract(f.ai_metadata, '$.megapixels') AS REAL) >= ?6)
             ORDER BY fts.rank
             LIMIT ?7"
        } else {
            "SELECT f.id, f.device_id, f.device_name, f.path, f.name,
                    f.ext, f.size, f.modified, f.hash, f.quick_hash, f.indexed_at, f.detected_by, fts.rank
             FROM files_fts fts
             JOIN files f ON f.id = fts.rowid
             WHERE files_fts MATCH ?1
               AND f.ai_metadata IS NOT NULL AND f.ai_metadata != ''
               AND (?2 IS NULL OR CAST(json_extract(f.ai_metadata, '$.in_focus') AS INTEGER) = ?2)
               AND (?3 IS NULL OR CAST(json_extract(f.ai_metadata, '$.quality_score') AS REAL) >= ?3)
               AND (?4 IS NULL OR CAST(json_extract(f.ai_metadata, '$.focus_score') AS REAL) >= ?4)
               AND (?5 IS NULL OR CAST(json_extract(f.ai_metadata, '$.megapixels') AS REAL) >= ?5)
             ORDER BY fts.rank
             LIMIT ?6"
        };

        let mut stmt = self.conn.prepare(sql)?;
        if let Some(dev) = device_filter {
            let rows = stmt.query_map(
                params![safe_query, dev, focus_param, min_quality, min_focus_score, min_megapixels, limit as i64],
                |row| self.map_search_result(row),
            )?;
            for r in rows {
                out.push(r?);
            }
        } else {
            let rows = stmt.query_map(
                params![safe_query, focus_param, min_quality, min_focus_score, min_megapixels, limit as i64],
                |row| self.map_search_result(row),
            )?;
            for r in rows {
                out.push(r?);
            }
        }
        Ok(out)
    }

    // ── Temporal View ─────────────────────────────────────────────────────

    pub fn get_recent_files(
        &self,
        limit:         usize,
        device_filter: Option<&str>,
    ) -> Result<Vec<TemporalEntry>> {
        let sql = if device_filter.is_some() {
            "SELECT id, device_id, device_name, path, name, ext, size, modified, hash
             FROM files
             WHERE device_id = ?1 AND modified IS NOT NULL
             ORDER BY modified DESC
             LIMIT ?2"
        } else {
            "SELECT id, device_id, device_name, path, name, ext, size, modified, hash
             FROM files
             WHERE modified IS NOT NULL
             ORDER BY modified DESC
             LIMIT ?1"
        };

        let mut stmt = self.conn.prepare(sql)?;

        let map_row = |row: &rusqlite::Row| -> rusqlite::Result<TemporalEntry> {
            let path_str: String = row.get(3)?;
            let modified: i64    = row.get(7)?;
            Ok(TemporalEntry {
                file_id:     row.get(0)?,
                device_id:   row.get(1)?,
                device_name: row.get(2)?,
                path:        PathBuf::from(path_str),
                name:        row.get(4)?,
                ext:         row.get(5)?,
                size:        row.get::<_, i64>(6)? as u64,
                modified,
                hash:        row.get(8)?,
                event_kind:  TemporalEventKind::Modified,
            })
        };

        let entries: Vec<TemporalEntry> = if let Some(dev) = device_filter {
            stmt.query_map(params![dev, limit as i64], map_row)?
                .filter_map(|r: rusqlite::Result<TemporalEntry>| r.ok())
                .collect()
        } else {
            stmt.query_map(params![limit as i64], map_row)?
                .filter_map(|r: rusqlite::Result<TemporalEntry>| r.ok())
                .collect()
        };

        Ok(entries)
    }

    pub fn upsert_remote_file(&self, file: FileSearchResult) -> Result<()> {
        if let Some(tombstone_ts) = self.get_tombstone_timestamp(&file.device_id, &file.path)? {
            let incoming_ts = file.modified.unwrap_or(file.indexed_at);
            if tombstone_ts >= incoming_ts {
                return Ok(());
            }
        }

        if let Some(existing_ts) = self.get_existing_index_timestamp(&file.device_id, &file.path)? {
            if existing_ts > file.indexed_at {
                return Ok(());
            }
        }

        let path_str = file.path.to_string_lossy();
        self.clear_tombstone_if_older(&file.device_id, &file.path, file.indexed_at)?;
        self.conn.execute(
            "INSERT INTO files (device_id, device_name, path, name, ext, size, modified, hash, quick_hash, detected_by, indexed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(device_id, path) DO UPDATE SET
                device_name = excluded.device_name,
                name       = excluded.name,
                ext        = excluded.ext,
                size       = excluded.size,
                modified   = excluded.modified,
                hash       = excluded.hash,
                quick_hash = excluded.quick_hash,
                detected_by = excluded.detected_by,
                indexed_at = excluded.indexed_at",
            params![
                file.device_id,
                file.device_name,
                path_str.as_ref(),
                file.name,
                file.ext,
                file.size as i64,
                file.modified,
                file.hash,
                file.quick_hash,
                file.detected_by.as_str(),
                file.indexed_at,
            ],
        )?;
        Ok(())
    }

    pub fn apply_remote_tombstone(&self, tombstone: &FileTombstone) -> Result<bool> {
        if let Some(existing_ts) = self.get_existing_index_timestamp(&tombstone.device_id, &tombstone.path)? {
            if existing_ts > tombstone.deleted_at {
                return Ok(false);
            }
        }

        self.insert_tombstone(tombstone)?;
        let path = tombstone.path.to_string_lossy().to_string();
        let (exact, win_like, unix_like) = path_match_patterns(&path);
        let deleted = self.conn.execute(
            "DELETE FROM files
             WHERE device_id = ?1
               AND (path = ?2 OR path LIKE ?3 OR path LIKE ?4)",
            params![tombstone.device_id, exact, win_like, unix_like],
        )?;
        Ok(deleted > 0)
    }

    pub fn file_count(&self, device_id: Option<&str>) -> Result<u64> {
        let n: i64 = if let Some(id) = device_id {
            self.conn.query_row(
                "SELECT COUNT(*) FROM files WHERE device_id = ?1",
                params![id], |r| r.get(0)
            )?
        } else {
            self.conn.query_row(
                "SELECT COUNT(*) FROM files", [], |r| r.get(0)
            )?
        };
        Ok(n as u64)
    }


    pub fn log_transfer(
        &self,
        direction: &str,
        peer_ip:   &str,
        filename:  &str,
        size:      u64,
        status:    &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO transfers (direction, peer_ip, filename, size, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![direction, peer_ip, filename, size as i64, status, unix_now()],
        )?;
        Ok(())
    }

    fn collect_file_rows_for_path(&self, device_id: &str, path: &str) -> Result<Vec<StoredFileRow>> {
        let (exact, win_like, unix_like) = path_match_patterns(path);
        let mut stmt = self.conn.prepare(
            "SELECT id, path, size, modified, hash, quick_hash, indexed_at
             FROM files
             WHERE device_id = ?1
               AND (path = ?2 OR path LIKE ?3 OR path LIKE ?4)"
        )?;
        let rows = stmt.query_map(params![device_id, exact, win_like, unix_like], |row| {
            Ok(StoredFileRow {
                id: row.get(0)?,
                path: row.get(1)?,
                size: row.get::<_, i64>(2)? as u64,
                modified: row.get(3)?,
                hash: row.get(4)?,
                quick_hash: row.get(5)?,
            })
        })?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    fn insert_tombstone(&self, tombstone: &FileTombstone) -> Result<()> {
        self.conn.execute(
            "INSERT INTO file_tombstones (device_id, path, size, modified, hash, quick_hash, deleted_at, detected_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(device_id, path) DO UPDATE SET
                size = excluded.size,
                modified = excluded.modified,
                hash = excluded.hash,
                quick_hash = excluded.quick_hash,
                deleted_at = excluded.deleted_at,
                detected_by = excluded.detected_by",
            params![
                tombstone.device_id,
                tombstone.path.to_string_lossy().to_string(),
                tombstone.size as i64,
                tombstone.modified,
                tombstone.hash,
                tombstone.quick_hash,
                tombstone.deleted_at,
                tombstone.detected_by.as_str(),
            ],
        )?;
        Ok(())
    }

    fn find_recent_tombstone(
        &self,
        device_id: &str,
        fingerprint: &FileFingerprint,
        window_secs: i64,
    ) -> Result<Option<FileTombstone>> {
        let Some(quick_hash) = fingerprint.quick_hash.as_deref() else {
            return Ok(None);
        };

        self.conn.query_row(
            "SELECT device_id, path, size, modified, hash, quick_hash, deleted_at, detected_by
             FROM file_tombstones
             WHERE device_id = ?1
               AND quick_hash = ?2
               AND size = ?3
               AND deleted_at >= ?4
             ORDER BY deleted_at DESC
             LIMIT 1",
            params![device_id, quick_hash, fingerprint.size as i64, unix_now() - window_secs],
            |row| {
                Ok(FileTombstone {
                    device_id: row.get(0)?,
                    path: PathBuf::from(row.get::<_, String>(1)?),
                    size: row.get::<_, i64>(2)? as u64,
                    modified: row.get(3)?,
                    hash: row.get(4)?,
                    quick_hash: row.get(5)?,
                    deleted_at: row.get(6)?,
                    detected_by: row.get::<_, Option<String>>(7)?
                        .as_deref()
                        .map(DetectionSource::from_db)
                        .unwrap_or(DetectionSource::Watcher),
                })
            },
        ).optional().map_err(Into::into)
    }

    fn remove_tombstone(&self, device_id: &str, path: &Path) -> Result<()> {
        self.conn.execute(
            "DELETE FROM file_tombstones WHERE device_id = ?1 AND path = ?2",
            params![device_id, path.to_string_lossy().to_string()],
        )?;
        Ok(())
    }

    fn clear_tombstone_if_older(&self, device_id: &str, path: &Path, indexed_at: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM file_tombstones WHERE device_id = ?1 AND path = ?2 AND deleted_at <= ?3",
            params![device_id, path.to_string_lossy().to_string(), indexed_at],
        )?;
        Ok(())
    }

    fn get_tombstone_timestamp(&self, device_id: &str, path: &Path) -> Result<Option<i64>> {
        let path = path.to_string_lossy().to_string();
        let (exact, win_like, unix_like) = path_match_patterns(&path);
        self.conn.query_row(
            "SELECT MAX(deleted_at)
             FROM file_tombstones
             WHERE device_id = ?1
               AND (path = ?2 OR path LIKE ?3 OR path LIKE ?4)",
            params![device_id, exact, win_like, unix_like],
            |row| row.get::<_, Option<i64>>(0),
        ).map_err(Into::into)
    }

    fn get_existing_index_timestamp(&self, device_id: &str, path: &Path) -> Result<Option<i64>> {
        let path = path.to_string_lossy().to_string();
        let (exact, win_like, unix_like) = path_match_patterns(&path);
        self.conn.query_row(
            "SELECT MAX(COALESCE(modified, indexed_at))
             FROM files
             WHERE device_id = ?1
               AND (path = ?2 OR path LIKE ?3 OR path LIKE ?4)",
            params![device_id, exact, win_like, unix_like],
            |row| row.get::<_, Option<i64>>(0),
        ).map_err(Into::into)
    }

    // ── SKIP_UNIQUE quick-hash pre-filter ─────────────────────────────────

    /// Returns true if any *other* file (different id) shares (quick_hash, size).
    /// Used by the hashing worker to skip full-hash on files with no plausible duplicates.
    pub fn has_quick_hash_peer(&self, file_id: i64, quick_hash: &str, size: u64) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM files
             WHERE quick_hash = ?1 AND size = ?2 AND id != ?3",
            params![quick_hash, size as i64, file_id],
            |r| r.get(0),
        )?;
        Ok(count > 0)
    }

    /// Mark a file as unique (no quick_hash peer found); skips full hashing.
    pub fn mark_file_skip_unique(&self, file_id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE files SET hash = 'SKIP_UNIQUE' WHERE id = ?1",
            params![file_id],
        )?;
        Ok(())
    }

    /// When a new file is indexed that matches (quick_hash, size) of a SKIP_UNIQUE file,
    /// reset the SKIP_UNIQUE so it gets re-hashed next cycle.
    pub fn reset_skip_unique_peers(&self, quick_hash: &str, size: u64) -> Result<()> {
        self.conn.execute(
            "UPDATE files SET hash = NULL WHERE quick_hash = ?1 AND size = ?2 AND hash = 'SKIP_UNIQUE'",
            params![quick_hash, size as i64],
        )?;
        Ok(())
    }

    /// Count files still needing a hash (excludes SKIP_UNIQUE and ERR_ markers).
    pub fn count_files_needing_hash_v2(&self) -> Result<usize> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM files
             WHERE (hash IS NULL OR hash = '') AND path IS NOT NULL",
            [],
            |r| r.get(0),
        )?;
        Ok(n.max(0) as usize)
    }

    // ── Hashing checkpoint ────────────────────────────────────────────────

    pub fn get_hashing_checkpoint(&self) -> Result<Option<i64>> {
        let val: Option<String> = self.conn.query_row(
            "SELECT value FROM hashing_state WHERE key = 'last_processed_id'",
            [],
            |r| r.get(0),
        ).optional()?;
        Ok(val.and_then(|v| v.parse::<i64>().ok()))
    }

    pub fn set_hashing_checkpoint(&self, last_id: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO hashing_state (key, value) VALUES ('last_processed_id', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![last_id.to_string()],
        )?;
        Ok(())
    }

    // ── Embedding queue ───────────────────────────────────────────────────

    pub fn queue_embedding_for_file(&self, file_id: i64, priority: u8) -> Result<()> {
        self.conn.execute(
            "INSERT INTO embedding_queue (file_id, priority, queued_at, attempts)
             VALUES (?1, ?2, ?3, 0)
             ON CONFLICT(file_id) DO UPDATE SET
               priority  = MIN(priority, excluded.priority)",
            params![file_id, priority as i64, unix_now()],
        )?;
        Ok(())
    }

    /// Return the next batch from the embedding queue ordered by priority then queued_at.
    /// Returns (file_id, text_for_embedding).
    pub fn get_embedding_queue_batch(&self, limit: usize) -> Result<Vec<(i64, String, u8)>> {
        let mut stmt = self.conn.prepare(
            "SELECT q.file_id, f.name || ' ' || COALESCE(f.ext, ''), q.priority
             FROM embedding_queue q
             JOIN files f ON f.id = q.file_id
             ORDER BY q.priority ASC, q.queued_at ASC
             LIMIT ?1"
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)? as u8))
        })?;
        let mut out = Vec::new();
        for r in rows { out.push(r?); }
        Ok(out)
    }

    pub fn dequeue_embedding(&self, file_id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM embedding_queue WHERE file_id = ?1",
            params![file_id],
        )?;
        Ok(())
    }

    pub fn increment_embedding_attempts(&self, file_id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE embedding_queue SET attempts = attempts + 1 WHERE file_id = ?1",
            params![file_id],
        )?;
        Ok(())
    }

    pub fn count_embedding_queue(&self) -> Result<usize> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM embedding_queue", [], |r| r.get(0)
        )?;
        Ok(n.max(0) as usize)
    }

    // ── Indexing audit log ────────────────────────────────────────────────

    pub fn log_indexing_decision(&self, path: &str, tier: &str, reason: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO indexing_log (path, tier, reason, logged_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![path, tier, reason, unix_now()],
        )?;
        Ok(())
    }

    pub fn get_indexing_log(&self, limit: usize) -> Result<Vec<(String, String, String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, tier, reason, logged_at FROM indexing_log
             ORDER BY logged_at DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?;
        let mut out = Vec::new();
        for r in rows { out.push(r?); }
        Ok(out)
    }

    // ── Deletion audit ────────────────────────────────────────────────────

    pub fn log_deletion(
        &self,
        session_id: &str,
        file_path: &str,
        device_id: &str,
        file_hash: Option<&str>,
        file_size: Option<u64>,
        action: &str,
        reason: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO deletion_audit
               (session_id, file_path, device_id, file_hash, file_size, action, reason, executed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                session_id,
                file_path,
                device_id,
                file_hash,
                file_size.map(|s| s as i64),
                action,
                reason,
                unix_now(),
            ],
        )?;
        Ok(())
    }

    pub fn get_deletion_audit(&self, session_id: &str) -> Result<Vec<crate::models::DeletionRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, file_path, device_id, file_hash, file_size, action, reason, executed_at
             FROM deletion_audit
             WHERE session_id = ?1
             ORDER BY id ASC"
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok(crate::models::DeletionRecord {
                id: row.get(0)?,
                session_id: row.get(1)?,
                file_path: row.get(2)?,
                device_id: row.get(3)?,
                file_hash: row.get(4)?,
                file_size: row.get::<_, Option<i64>>(5)?.map(|s| s as u64),
                action: row.get(6)?,
                reason: row.get(7)?,
                executed_at: row.get(8)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows { out.push(r?); }
        Ok(out)
    }

    // ── Cross-source duplicate groups ─────────────────────────────────────

    /// Returns rich duplicate groups with per-source breakdowns.
    /// Excludes SKIP_UNIQUE and ERR_ sentinels. Respects github_backed filter.
    pub fn get_cross_source_duplicate_groups(
        &self,
        filter: &crate::models::DuplicateScanFilter,
        include_github_backed: bool,
    ) -> Result<Vec<crate::models::DuplicateGroup>> {
        let github_filter = if include_github_backed { "" } else { "AND github_backed = 0" };

        let sql = format!(
            "SELECT hash, size, COUNT(*) as cnt, COUNT(DISTINCT device_id) as srcs
             FROM files
             WHERE hash IS NOT NULL
               AND hash != ''
               AND hash NOT LIKE 'ERR_%'
               AND hash != 'SKIP_UNIQUE'
               {}
             GROUP BY hash, size
             HAVING cnt > 1",
            github_filter
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let candidates: Vec<(String, u64, u64, u64)> = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)? as u64,
                row.get::<_, i64>(2)? as u64,
                row.get::<_, i64>(3)? as u64,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();

        let mut groups = Vec::new();
        for (hash, size, file_count, source_count) in candidates {
            if filter.min_size_bytes > 0 && size < filter.min_size_bytes {
                continue;
            }
            if groups.len() >= filter.max_groups {
                break;
            }

            let mut file_stmt = self.conn.prepare(
                "SELECT id, device_id, device_name, path, name, ext, size, modified, hash, quick_hash, indexed_at, detected_by, 0.0 as rank
                 FROM files WHERE hash = ?1 AND size = ?2"
            )?;
            let files: Vec<crate::models::FileSearchResult> = file_stmt
                .query_map(params![hash, size as i64], |r| self.map_search_result(r))?
                .filter_map(|r| r.ok())
                .collect();

            if !filter.include_extensions.is_empty() {
                let any_match = files.iter().any(|f| {
                    f.ext.as_ref().map_or(false, |e| {
                        filter.include_extensions.iter().any(|fe| fe.eq_ignore_ascii_case(e))
                    })
                });
                if !any_match { continue; }
            }

            // Build per-source summaries
            let mut source_map: std::collections::HashMap<String, (String, u32)> =
                std::collections::HashMap::new();
            for f in &files {
                let e = source_map.entry(f.device_id.clone())
                    .or_insert_with(|| (f.device_name.clone(), 0));
                e.1 += 1;
            }

            let sources: Vec<crate::models::SourceSummary> = source_map
                .into_iter()
                .map(|(device_id, (device_name, cnt))| {
                    let source_type = if device_id.starts_with("gdrive:") {
                        crate::models::SourceType::GoogleDrive
                    } else if device_id.starts_with("nas:") {
                        crate::models::SourceType::Nas
                    } else {
                        crate::models::SourceType::Local
                    };
                    crate::models::SourceSummary { device_id, device_name, file_count: cnt, source_type }
                })
                .collect();

            // Suggest anchor: prefer Local source with most files
            let suggested_anchor = sources.iter()
                .filter(|s| s.source_type == crate::models::SourceType::Local)
                .max_by_key(|s| s.file_count)
                .map(|s| s.device_id.clone());

            groups.push(crate::models::DuplicateGroup {
                hash: hash.clone(),
                size,
                file_count: file_count as u32,
                source_count: source_count as u32,
                sources,
                files,
                suggested_anchor,
            });
        }

        Ok(groups)
    }

    fn rename_path_tree(
        &self,
        device_id: &str,
        old_path: &Path,
        new_path: &Path,
        detected_by: DetectionSource,
    ) -> Result<usize> {
        let rows = self.collect_file_rows_for_path(device_id, &old_path.to_string_lossy())?;
        let mut renamed = 0;
        let indexed_at = unix_now();

        for row in rows {
            let current_path = PathBuf::from(&row.path);
            let target_path = match current_path.strip_prefix(old_path) {
                Ok(suffix) if !suffix.as_os_str().is_empty() => new_path.join(suffix),
                Ok(_) => new_path.to_path_buf(),
                Err(_) => {
                    // Fallback for mixed separator/path normalization cases.
                    let current_norm = current_path.to_string_lossy().replace('\\', "/");
                    let old_norm = old_path.to_string_lossy().replace('\\', "/");
                    let new_norm = new_path.to_string_lossy().replace('\\', "/");
                    let old_prefix = format!("{}/", old_norm.trim_end_matches('/'));

                    if current_norm == old_norm {
                        PathBuf::from(new_path)
                    } else if let Some(suffix) = current_norm.strip_prefix(&old_prefix) {
                        let merged = format!("{}/{}", new_norm.trim_end_matches('/'), suffix);
                        PathBuf::from(merged)
                    } else {
                        new_path.to_path_buf()
                    }
                }
            };

            let name = target_path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let ext = target_path.extension()
                .map(|e| e.to_string_lossy().to_lowercase());

            self.conn.execute(
                "UPDATE files
                 SET path = ?1,
                     name = ?2,
                     ext = ?3,
                     detected_by = ?4,
                     indexed_at = ?5
                 WHERE id = ?6",
                params![
                    target_path.to_string_lossy().to_string(),
                    name,
                    ext,
                    detected_by.as_str(),
                    indexed_at,
                    row.id,
                ],
            )?;
            renamed += 1;
        }

        Ok(renamed)
    }

    // ── Persistent duplicate group registry ───────────────────────────────────

    /// Persist detected duplicate groups to the database.
    /// Group metadata is updated on re-scan; existing member review actions are preserved
    /// (INSERT OR IGNORE on members so prior Keep/Delete decisions survive).
    pub fn upsert_duplicate_groups(&self, groups: &[crate::models::DuplicateGroup]) -> Result<()> {
        let now = unix_now();
        for g in groups {
            self.conn.execute(
                "INSERT INTO duplicate_groups
                    (hash, size, file_count, source_count, suggested_anchor, status, first_seen_at, last_seen_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6, ?6)
                 ON CONFLICT(hash, size) DO UPDATE
                   SET file_count       = excluded.file_count,
                       source_count     = excluded.source_count,
                       suggested_anchor = excluded.suggested_anchor,
                       last_seen_at     = excluded.last_seen_at
                   WHERE status != 'resolved'",
                params![
                    g.hash,
                    g.size as i64,
                    g.file_count as i64,
                    g.source_count as i64,
                    g.suggested_anchor,
                    now,
                ],
            )?;
            for file in &g.files {
                self.conn.execute(
                    "INSERT OR IGNORE INTO duplicate_group_members
                        (hash, size, file_id, action, updated_at)
                     VALUES (?1, ?2, ?3, 'undecided', ?4)",
                    params![g.hash, g.size as i64, file.id, now],
                )?;
            }
        }
        Ok(())
    }

    /// Load all non-resolved duplicate groups from the database, joining back to the files
    /// table to reconstruct full `DuplicateGroup` objects.
    /// Returns `(groups, actions_map)` where `actions_map` is `file_id → action string`.
    pub fn load_persisted_duplicate_groups(
        &self,
    ) -> Result<(Vec<crate::models::DuplicateGroup>, std::collections::HashMap<i64, String>)> {
        use crate::models::{DuplicateGroup, FileSearchResult, SourceSummary, SourceType, DetectionSource};
        use std::collections::HashMap;

        let mut stmt = self.conn.prepare(
            "SELECT
                dg.hash, dg.size, dg.file_count, dg.source_count, dg.suggested_anchor,
                f.id, f.device_id, f.device_name, f.path, f.name, f.ext,
                f.size AS file_size, f.modified, f.hash AS file_hash, f.quick_hash,
                f.indexed_at, f.detected_by,
                COALESCE(f.source_type, 'Local') AS source_type,
                dgm.action
             FROM duplicate_groups dg
             JOIN duplicate_group_members dgm ON dgm.hash = dg.hash AND dgm.size = dg.size
             JOIN files f ON f.id = dgm.file_id
             WHERE dg.status != 'resolved'
             ORDER BY dg.size DESC, dg.hash, f.device_id, f.path"
        )?;

        // Collect raw rows grouped by (hash, size)
        struct RawRow {
            hash: String, size: u64,
            file_count: u32, source_count: u32, suggested_anchor: Option<String>,
            file: FileSearchResult, source_type: String, action: String,
        }

        let mut raw_rows: Vec<RawRow> = Vec::new();
        let iter = stmt.query_map([], |row| {
            Ok(RawRow {
                hash:             row.get::<_, String>(0)?,
                size:             row.get::<_, i64>(1)? as u64,
                file_count:       row.get::<_, i64>(2)? as u32,
                source_count:     row.get::<_, i64>(3)? as u32,
                suggested_anchor: row.get::<_, Option<String>>(4)?,
                file: FileSearchResult {
                    id:          row.get::<_, i64>(5)?,
                    device_id:   row.get::<_, String>(6)?,
                    device_name: row.get::<_, String>(7)?,
                    path:        std::path::PathBuf::from(row.get::<_, String>(8)?),
                    name:        row.get::<_, String>(9)?,
                    ext:         row.get::<_, Option<String>>(10)?,
                    size:        row.get::<_, i64>(11)? as u64,
                    modified:    row.get::<_, Option<i64>>(12)?,
                    hash:        row.get::<_, Option<String>>(13)?,
                    quick_hash:  row.get::<_, Option<String>>(14)?,
                    indexed_at:  row.get::<_, i64>(15)?,
                    detected_by: row.get::<_, Option<String>>(16)?
                        .as_deref()
                        .and_then(|s| serde_json::from_str(&format!("\"{}\"", s)).ok())
                        .unwrap_or(DetectionSource::FullScan),
                    rank: None,
                },
                source_type: row.get::<_, String>(17)?,
                action:      row.get::<_, String>(18)?,
            })
        })?;
        for r in iter { raw_rows.push(r?); }

        // Group by (hash, size)
        let mut groups_map: std::collections::BTreeMap<(String, u64), DuplicateGroup> =
            std::collections::BTreeMap::new();
        let mut actions_map: HashMap<i64, String> = HashMap::new();

        for row in raw_rows {
            let key = (row.hash.clone(), row.size);

            let group = groups_map.entry(key).or_insert_with(|| DuplicateGroup {
                hash:             row.hash.clone(),
                size:             row.size,
                file_count:       row.file_count,
                source_count:     row.source_count,
                suggested_anchor: row.suggested_anchor.clone(),
                sources:          Vec::new(),
                files:            Vec::new(),
            });

            actions_map.insert(row.file.id, row.action.clone());

            // Build / update SourceSummary for this device
            let src_type = match row.source_type.as_str() {
                "GoogleDrive" => SourceType::GoogleDrive,
                "Nas"         => SourceType::Nas,
                _             => SourceType::Local,
            };
            if let Some(existing) = group.sources.iter_mut()
                .find(|s| s.device_id == row.file.device_id) {
                existing.file_count += 1;
            } else {
                group.sources.push(SourceSummary {
                    device_id:   row.file.device_id.clone(),
                    device_name: row.file.device_name.clone(),
                    file_count:  1,
                    source_type: src_type,
                });
            }

            group.files.push(row.file);
        }

        let groups: Vec<DuplicateGroup> = groups_map.into_values().collect();
        Ok((groups, actions_map))
    }

    /// Batch-save per-file review decisions back to the DB.
    /// Each item is `(hash, size, file_id, action)` where action is "keep"|"delete"|"undecided".
    pub fn save_dedup_actions_batch(
        &self,
        items: &[(String, u64, i64, String)],
    ) -> Result<()> {
        let now = unix_now();
        for (hash, size, file_id, action) in items {
            self.conn.execute(
                "INSERT INTO duplicate_group_members (hash, size, file_id, action, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(hash, size, file_id) DO UPDATE
                   SET action = excluded.action, updated_at = excluded.updated_at",
                params![hash, *size as i64, file_id, action, now],
            )?;
        }
        Ok(())
    }

    /// Mark a duplicate group as resolved (all decisions executed).
    /// Resolved groups are not returned by `load_persisted_duplicate_groups`.
    pub fn mark_duplicate_group_resolved(&self, hash: &str, size: u64) -> Result<()> {
        self.conn.execute(
            "UPDATE duplicate_groups SET status = 'resolved' WHERE hash = ?1 AND size = ?2",
            params![hash, size as i64],
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct StoredFileRow {
    id: i64,
    path: String,
    size: u64,
    modified: Option<i64>,
    hash: Option<String>,
    quick_hash: Option<String>,
}

pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn should_skip_dir(name: &str) -> bool {
    matches!(
        name,
        ".git" | ".svn" | ".hg"
        | "node_modules" | ".pnpm-store"
        | "target"
        | "__pycache__" | ".mypy_cache" | ".pytest_cache"
        | ".cargo" | ".rustup"
        | "vendor"
        | "$RECYCLE.BIN" | "System Volume Information"
        | "WindowsApps" | "Windows" | "ProgramData"
    ) || name.starts_with('.')
}

pub fn should_skip_path(path: &Path) -> bool {
    let component_names: Vec<String> = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();

    // Skip by component names (case-insensitive), covering common system and dev-noise folders.
    let blocked_components = [
        "windows",
        "program files",
        "program files (x86)",
        "programdata",
        "perflogs",
        "$recycle.bin",
        "system volume information",
        "recovery",
        "msocache",
        "intel",
        "nvidia",
        ".venv",
        "venv",
        ".cache",
        "dist",
        "build",
    ];

    for raw in &component_names {
        let name = raw.to_lowercase();
        if should_skip_dir(raw) {
            return true;
        }
        if blocked_components.iter().any(|b| *b == name) {
            return true;
        }
    }

    // Linux/macOS runtime/system paths (only relevant on those targets).
    let normalized = path.to_string_lossy().replace('\\', "/").to_lowercase();
    let blocked_prefixes = ["/proc", "/sys", "/dev", "/run", "/snap", "/var/lib/docker"];
    blocked_prefixes
        .iter()
        .any(|prefix| normalized == *prefix || normalized.starts_with(&format!("{}/", prefix)))
}

fn path_match_patterns(path: &str) -> (String, String, String) {
    (
        path.to_string(),
        format!("{}\\%", path),
        format!("{}/%", path),
    )
}

fn sanitize_fts_query(q: &str) -> String {
    let mut out = String::with_capacity(q.len() + 2);
    let mut in_quote = false;
    for ch in q.chars() {
        if ch == '"' { in_quote = !in_quote; }
        out.push(ch);
    }
    if in_quote { out.push('"'); }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_remote_file(path: &str, indexed_at: i64, modified: i64) -> FileSearchResult {
        FileSearchResult {
            id: 0,
            device_id: "dev-a".to_string(),
            device_name: "DEV-A".to_string(),
            path: PathBuf::from(path),
            name: Path::new(path)
                .file_name()
                .map(|v| v.to_string_lossy().to_string())
                .unwrap_or_else(|| "file.bin".to_string()),
            ext: Path::new(path)
                .extension()
                .map(|v| v.to_string_lossy().to_string()),
            size: 1024,
            modified: Some(modified),
            hash: Some("hash-1".to_string()),
            quick_hash: Some("qh-1".to_string()),
            indexed_at,
            detected_by: DetectionSource::Sync,
            rank: None,
        }
    }

    fn sample_tombstone(path: &str, deleted_at: i64) -> FileTombstone {
        FileTombstone {
            device_id: "dev-a".to_string(),
            path: PathBuf::from(path),
            size: 1024,
            modified: Some(deleted_at),
            hash: Some("hash-1".to_string()),
            quick_hash: Some("qh-1".to_string()),
            deleted_at,
            detected_by: DetectionSource::Sync,
        }
    }

    fn path_count(db: &Database, device_id: &str, path: &str) -> i64 {
        db.conn
            .query_row(
                "SELECT COUNT(*) FROM files WHERE device_id = ?1 AND path = ?2",
                params![device_id, path],
                |row| row.get(0),
            )
            .expect("path count")
    }

    fn os_path_string(path: &str) -> String {
        PathBuf::from(path).to_string_lossy().to_string()
    }

    fn prefix_count(db: &Database, device_id: &str, prefix: &str) -> i64 {
        let normalized = os_path_string(prefix);
        let (exact, win_like, unix_like) = path_match_patterns(&normalized);
        db.conn
            .query_row(
                "SELECT COUNT(*) FROM files WHERE device_id = ?1 AND (path = ?2 OR path LIKE ?3 OR path LIKE ?4)",
                params![device_id, exact, win_like, unix_like],
                |row| row.get(0),
            )
            .expect("prefix count")
    }

    #[test]
    fn tombstone_blocks_stale_remote_file_resurrection() {
        let db = Database::open(":memory:").expect("open db");
        let tombstone = sample_tombstone("C:/mesh/doc.txt", 200);
        db.apply_remote_tombstone(&tombstone).expect("apply tombstone");

        let stale_file = sample_remote_file("C:/mesh/doc.txt", 150, 150);
        db.upsert_remote_file(stale_file).expect("upsert stale file");

        let count = db.file_count(Some("dev-a")).expect("file count");
        assert_eq!(count, 0, "stale file should not resurrect deleted entry");

        let delta = db.get_sync_delta_after(0).expect("sync delta");
        assert_eq!(delta.tombstones.len(), 1, "tombstone must remain visible in sync delta");
    }

    #[test]
    fn stale_remote_tombstone_does_not_override_newer_file() {
        let db = Database::open(":memory:").expect("open db");
        let fresh_file = sample_remote_file("C:/mesh/report.txt", 300, 300);
        db.upsert_remote_file(fresh_file).expect("upsert fresh file");

        let stale_tombstone = sample_tombstone("C:/mesh/report.txt", 200);
        let applied = db
            .apply_remote_tombstone(&stale_tombstone)
            .expect("apply stale tombstone");

        assert!(!applied, "older tombstone must be ignored when file is newer");
        let count = db.file_count(Some("dev-a")).expect("file count");
        assert_eq!(count, 1, "newer file should remain present");
    }

    #[test]
    fn rename_storm_keeps_single_final_path() {
        let db = Database::open(":memory:").expect("open db");
        let initial = sample_remote_file("C:/storm/a.txt", 100, 100);
        db.upsert_remote_file(initial).expect("seed file");

        let mut current = "C:/storm/a.txt".to_string();
        for i in 0..25 {
            let next = format!("C:/storm/a_{}.txt", i);
            let changes = vec![FileChange {
                kind: FileChangeKind::Renamed,
                path: PathBuf::from(&next),
                old_path: Some(PathBuf::from(&current)),
                new_path: Some(PathBuf::from(&next)),
                fingerprint: None,
            }];

            let (_u, _d, renamed) = db
                .index_changed_paths("dev-a", "DEV-A", &changes)
                .expect("rename storm step");
            assert_eq!(renamed, 1, "each rename step should move one file row");
            current = next;
        }

        assert_eq!(db.file_count(Some("dev-a")).expect("count"), 1);
        assert_eq!(path_count(&db, "dev-a", "C:/storm/a.txt"), 0);
        assert_eq!(path_count(&db, "dev-a", &current), 1);
    }

    #[test]
    fn recursive_move_updates_entire_subtree() {
        let db = Database::open(":memory:").expect("open db");
        let seed_paths = [
            "C:/root_a/proj/readme.md",
            "C:/root_a/proj/src/main.rs",
            "C:/root_a/proj/assets/logo.png",
        ];

        for (i, p) in seed_paths.iter().enumerate() {
            db.upsert_remote_file(sample_remote_file(p, 200 + i as i64, 200 + i as i64))
                .expect("seed subtree file");
        }

        let changes = vec![FileChange {
            kind: FileChangeKind::Renamed,
            path: PathBuf::from("D:/root_b/proj"),
            old_path: Some(PathBuf::from("C:/root_a/proj")),
            new_path: Some(PathBuf::from("D:/root_b/proj")),
            fingerprint: None,
        }];

        let (_u, _d, renamed) = db
            .index_changed_paths("dev-a", "DEV-A", &changes)
            .expect("recursive move");
        assert_eq!(renamed, 3, "all subtree files should be moved");

        assert_eq!(prefix_count(&db, "dev-a", "C:/root_a/proj"), 0);
        assert_eq!(prefix_count(&db, "dev-a", "D:/root_b/proj"), 3);

        let mut stmt = db
            .conn
            .prepare(
                "SELECT name FROM files
                 WHERE device_id = ?1
                   AND (path = ?2 OR path LIKE ?3 OR path LIKE ?4)",
            )
            .expect("prepare names query");
        let new_prefix = os_path_string("D:/root_b/proj");
        let (exact, win_like, unix_like) = path_match_patterns(&new_prefix);
        let mut names: Vec<String> = stmt
            .query_map(params!["dev-a", exact, win_like, unix_like], |row| row.get(0))
            .expect("query moved names")
            .filter_map(|r: rusqlite::Result<String>| r.ok())
            .collect();
        names.sort();
        assert_eq!(names, vec!["logo.png", "main.rs", "readme.md"]);
    }

    #[test]
    fn reconnect_replay_respects_event_ordering() {
        let db = Database::open(":memory:").expect("open db");

        // Newer delete arrives first during reconnect replay.
        let t_new = sample_tombstone("C:/replay/state.txt", 500);
        db.apply_remote_tombstone(&t_new).expect("apply fresh tombstone");

        // Older file replay must be ignored (no stale resurrection).
        let stale = sample_remote_file("C:/replay/state.txt", 450, 450);
        db.upsert_remote_file(stale).expect("apply stale file replay");
        assert_eq!(db.file_count(Some("dev-a")).expect("count after stale replay"), 0);

        // Newer file replay should resurrect and clear outdated tombstone.
        let fresh = sample_remote_file("C:/replay/state.txt", 600, 600);
        db.upsert_remote_file(fresh).expect("apply fresh file replay");
        assert_eq!(db.file_count(Some("dev-a")).expect("count after fresh replay"), 1);

        // Late arrival of older tombstone must not remove newer file.
        let t_old = sample_tombstone("C:/replay/state.txt", 500);
        let applied = db
            .apply_remote_tombstone(&t_old)
            .expect("apply stale tombstone replay");
        assert!(!applied, "stale tombstone replay must be ignored");
        assert_eq!(db.file_count(Some("dev-a")).expect("final count"), 1);
    }

    // ── New testability / fixture tests ───────────────────────────────────────

    #[test]
    fn dedup_query_finds_same_hash_files() {
        let db = Database::open(":memory:").expect("open db");

        // Two files on same device with same hash → one duplicate group
        db.index_file_with_source("dev-a", "A", &PathBuf::from("C:/a/photo.jpg"), 1000, None, None, Some("abc123"), DetectionSource::FullScan, 100).unwrap();
        db.index_file_with_source("dev-a", "A", &PathBuf::from("C:/b/photo.jpg"), 1000, None, None, Some("abc123"), DetectionSource::FullScan, 100).unwrap();
        // Unique file
        db.index_file_with_source("dev-a", "A", &PathBuf::from("C:/c/unique.jpg"), 500, None, None, Some("xyz999"), DetectionSource::FullScan, 100).unwrap();

        let groups = db.get_duplicate_groups().expect("dedup query");
        assert_eq!(groups.len(), 1, "expected exactly one duplicate group");
        assert_eq!(groups[0].0, "abc123");
        assert_eq!(groups[0].2.len(), 2);
    }

    #[test]
    fn dedup_query_excludes_skip_unique_sentinel() {
        let db = Database::open(":memory:").expect("open db");

        let id1 = db.index_file_with_source("dev-a", "A", &PathBuf::from("C:/a/file.txt"), 100, None, Some("qh1"), None, DetectionSource::FullScan, 100).unwrap();
        db.mark_file_skip_unique(id1).unwrap();

        // SKIP_UNIQUE files must never appear in a duplicate group
        let groups = db.get_duplicate_groups().expect("dedup query");
        assert!(groups.is_empty(), "SKIP_UNIQUE files must not appear in dedup groups");
    }

    #[test]
    fn cross_device_dedup_detected() {
        let db = Database::open(":memory:").expect("open db");

        // Same file on two different devices
        db.index_file_with_source("dev-a", "A", &PathBuf::from("C:/a/photo.jpg"), 2000, None, None, Some("shared_hash"), DetectionSource::FullScan, 100).unwrap();
        db.index_file_with_source("dev-b", "B", &PathBuf::from("D:/b/photo.jpg"), 2000, None, None, Some("shared_hash"), DetectionSource::Sync, 100).unwrap();

        let (groups, files, bytes, devices) = db.crosscheck_duplicates_for_device("dev-a").unwrap();
        assert_eq!(groups, 1, "one cross-device duplicate group");
        assert_eq!(files, 1, "one file from dev-a in the group");
        assert!(bytes > 0);
        assert!(devices >= 2);
    }

    #[test]
    fn embedding_queue_enqueue_dequeue_cycle() {
        let db = Database::open(":memory:").expect("open db");

        let id = db.index_file_with_source("dev-a", "A", &PathBuf::from("C:/a/doc.txt"), 100, None, None, None, DetectionSource::FullScan, 100).unwrap();
        db.queue_embedding_for_file(id, 2).unwrap();

        let batch = db.get_embedding_queue_batch(10).unwrap();
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].0, id);
        assert_eq!(batch[0].2, 2); // priority

        db.dequeue_embedding(id).unwrap();
        let after = db.get_embedding_queue_batch(10).unwrap();
        assert!(after.is_empty(), "queue should be empty after dequeue");
    }

    #[test]
    fn embedding_queue_priority_order() {
        let db = Database::open(":memory:").expect("open db");

        let id1 = db.index_file_with_source("dev-a", "A", &PathBuf::from("C:/a/low.txt"), 10, None, None, None, DetectionSource::FullScan, 100).unwrap();
        let id2 = db.index_file_with_source("dev-a", "A", &PathBuf::from("C:/a/high.txt"), 10, None, None, None, DetectionSource::FullScan, 100).unwrap();
        db.queue_embedding_for_file(id1, 3).unwrap(); // low priority
        db.queue_embedding_for_file(id2, 1).unwrap(); // high priority

        let batch = db.get_embedding_queue_batch(10).unwrap();
        assert_eq!(batch[0].0, id2, "high priority item should come first");
    }

    #[test]
    fn has_quick_hash_peer_detects_same_file_pair() {
        let db = Database::open(":memory:").expect("open db");

        let id1 = db.index_file_with_source("dev-a", "A", &PathBuf::from("C:/a/a.bin"), 500, None, Some("qhXYZ"), None, DetectionSource::FullScan, 100).unwrap();
        let _id2 = db.index_file_with_source("dev-a", "A", &PathBuf::from("C:/a/b.bin"), 500, None, Some("qhXYZ"), None, DetectionSource::FullScan, 100).unwrap();

        let has_peer = db.has_quick_hash_peer(id1, "qhXYZ", 500).unwrap();
        assert!(has_peer, "should detect the peer file with same quick_hash+size");
    }

    #[test]
    fn has_quick_hash_peer_returns_false_for_unique_file() {
        let db = Database::open(":memory:").expect("open db");
        let id = db.index_file_with_source("dev-a", "A", &PathBuf::from("C:/a/alone.bin"), 500, None, Some("qhUNIQ"), None, DetectionSource::FullScan, 100).unwrap();

        let has_peer = db.has_quick_hash_peer(id, "qhUNIQ", 500).unwrap();
        assert!(!has_peer, "lone file should have no quick_hash peer");
    }

    #[test]
    fn hashing_checkpoint_roundtrip() {
        let db = Database::open(":memory:").expect("open db");
        assert!(db.get_hashing_checkpoint().unwrap().is_none());
        db.set_hashing_checkpoint(42).unwrap();
        assert_eq!(db.get_hashing_checkpoint().unwrap(), Some(42));
        db.set_hashing_checkpoint(100).unwrap();
        assert_eq!(db.get_hashing_checkpoint().unwrap(), Some(100));
    }

    #[test]
    fn deletion_audit_records_are_persisted() {
        let db = Database::open(":memory:").expect("open db");
        db.log_deletion("sess-1", "C:/a/file.jpg", "dev-a", Some("hashX"), Some(1024), "deleted", Some("duplicate")).unwrap();
        db.log_deletion("sess-1", "C:/b/file.jpg", "dev-b", None, None, "skipped", None).unwrap();

        let records = db.get_deletion_audit("sess-1").unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].action, "deleted");
        assert_eq!(records[1].action, "skipped");
    }

    #[test]
    fn cross_source_dedup_groups_produced() {
        let db = Database::open(":memory:").expect("open db");

        db.index_file_with_source("dev-a", "A", &PathBuf::from("C:/a/pic.jpg"), 4096, None, None, Some("h1"), DetectionSource::FullScan, 100).unwrap();
        db.index_file_with_source("gdrive:user@gmail.com", "Drive", &PathBuf::from("drive:/photos/pic.jpg"), 4096, None, None, Some("h1"), DetectionSource::Sync, 100).unwrap();

        let filter = crate::models::DuplicateScanFilter::default();
        let groups = db.get_cross_source_duplicate_groups(&filter, true).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].source_count, 2);
        let has_drive = groups[0].sources.iter().any(|s| s.source_type == crate::models::SourceType::GoogleDrive);
        assert!(has_drive, "should detect Drive as a separate source type");
    }
}
