use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::models::{FileSearchResult, TemporalEntry, TemporalEventKind};

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
                ai_metadata TEXT,
                indexed_at  INTEGER NOT NULL,
                UNIQUE(device_id, path)
            );

            CREATE INDEX IF NOT EXISTS idx_files_device  ON files(device_id);
            CREATE INDEX IF NOT EXISTS idx_files_ext     ON files(ext);
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
        "#).context("Initializing database schema")?;
        log::info!("[DB] Schema ready");
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
        let name = path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let ext = path.extension()
            .map(|e| e.to_string_lossy().to_lowercase());
        let path_str = path.to_string_lossy();
        let now = unix_now();

        self.conn.execute(
            "INSERT INTO files (device_id, device_name, path, name, ext, size, modified, hash, indexed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(device_id, path) DO UPDATE
               SET name        = excluded.name,
                   ext         = excluded.ext,
                   size        = excluded.size,
                   modified    = excluded.modified,
                   hash        = COALESCE(excluded.hash, files.hash),
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
                now,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
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
                "SELECT id, device_id, device_name, path, name, ext, size, modified, hash, 0.0 as rank 
                 FROM files WHERE hash = ?1 AND size = ?2"
            )?;
            let files = file_stmt.query_map(params![hash, size as i64], |r| self.map_search_result(r))?;
            let mut file_list = Vec::new();
            for f in files { file_list.push(f?); }
            results.push((hash, size, file_list));
        }
        Ok(results)
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

    pub fn get_files_after(&self, after: i64) -> Result<Vec<FileSearchResult>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, device_id, device_name, path, name, ext, size, modified, hash, 0.0 as rank \
             FROM files WHERE indexed_at > ?1 OR modified > ?1"
        )?;
        let rows = stmt.query_map(params![after], |row| self.map_search_result(row))?;
        let mut results = Vec::new();
        for r in rows { results.push(r?); }
        Ok(results)
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
            rank:        row.get(9)?,
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

    pub fn remove_path(&self, device_id: &str, path: &Path) -> Result<usize> {
        let path_str = path.to_string_lossy();
        let n = self.conn.execute(
            "DELETE FROM files WHERE device_id = ?1 AND path = ?2",
            params![device_id, path_str.as_ref()],
        )?;
        Ok(n)
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
               AND (ext IN ('jpg', 'jpeg', 'png', 'webp', 'mp4', 'mkv', 'mov', 'avi'))
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
            "SELECT id, device_id, device_name, path, name, ext, size, modified, 0.0 as rank
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

    pub fn index_directory<F>(
        &self,
        device_id:   &str,
        device_name: &str,
        root_dir:    &Path,
        mut progress_cb: F,
    ) -> Result<u64>
    where
        F: FnMut(u64, &Path),
    {
        let mut count = 0u64;
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
                let meta = match entry.metadata() {
                    Ok(m)  => m,
                    Err(_) => continue,
                };

                if meta.is_dir() {
                    let name = path.file_name()
                        .unwrap_or_default()
                        .to_string_lossy();
                    if should_skip_dir(&name) { continue; }
                    stack.push(path);
                } else if meta.is_file() {
                    let size     = meta.len();
                    let modified = meta.modified().ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map(|d| d.as_secs() as i64);

                    if let Err(e) = self.index_file(device_id, device_name, &path, size, modified, None) {
                        log::warn!("[DB] Failed to index {:?}: {}", path, e);
                    }

                    count += 1;
                    progress_cb(count, &path);
                }
            }
        }

        log::info!("[DB] Indexed {} files under {:?}", count, root_dir);
        Ok(count)
    }

    pub fn index_changed_paths(
        &self,
        device_id:   &str,
        device_name: &str,
        paths:       &[PathBuf],
    ) -> Result<(usize, usize)> {
        let mut updated = 0;
        let mut deleted = 0;

        for path in paths {
            if path.exists() {
                if path.is_file() {
                    if let Ok(meta) = std::fs::metadata(path) {
                        let size = meta.len();
                        let modified = meta.modified().ok()
                            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                            .map(|d| d.as_secs() as i64);
                        if self.index_file(device_id, device_name, path, size, modified, None).is_ok() {
                            updated += 1;
                        }
                    }
                }
            } else {
                if self.remove_path(device_id, path)? > 0 {
                    deleted += 1;
                }
            }
        }

        Ok((updated, deleted))
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
                    f.ext, f.size, f.modified, f.hash, fts.rank
             FROM files_fts fts
             JOIN files f ON f.id = fts.rowid
             WHERE files_fts MATCH ?1
               AND f.device_id = ?2
             ORDER BY fts.rank
             LIMIT ?3"
        } else {
            "SELECT f.id, f.device_id, f.device_name, f.path, f.name,
                    f.ext, f.size, f.modified, f.hash, fts.rank
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
                rank:        row.get(9)?,
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
        let path_str = file.path.to_string_lossy();
        self.conn.execute(
            "INSERT INTO files (device_id, device_name, path, name, ext, size, modified, hash, indexed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(device_id, path) DO UPDATE SET
                device_name = excluded.device_name,
                name       = excluded.name,
                ext        = excluded.ext,
                size       = excluded.size,
                modified   = excluded.modified,
                hash       = excluded.hash,
                indexed_at = excluded.indexed_at",
            params![
                file.device_id,
                file.device_name,
                path_str.as_ref(),
                file.name,
                file.ext,
                file.size as i64,
                file.modified.unwrap_or(0),
                "", // hash
                unix_now(),
            ],
        )?;
        Ok(())
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
            "INSERT INTO transfers (direction, peer_ip, filename, size, status, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![direction, peer_ip, filename, size as i64, status, unix_now()],
        )?;
        Ok(())
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn should_skip_dir(name: &str) -> bool {
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
