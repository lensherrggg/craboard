use super::{StoreError, TaskChanges, TaskStore};
use crate::state::{Repeat, TaskRecord, TaskStatus, TodoItem};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};
use std::time::Duration;

const SCHEMA_VERSION: i64 = 4;

pub struct SqliteTaskStore {
    conn: Connection,
    path: PathBuf,
}

impl SqliteTaskStore {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let conn = Connection::open(path).map_err(err)?;
        conn.busy_timeout(Duration::from_secs(3)).map_err(err)?;
        conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0))
            .map_err(err)?;
        conn.execute_batch("PRAGMA synchronous=NORMAL")
            .map_err(err)?;
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(err)?;
        if version > SCHEMA_VERSION {
            return Err(StoreError::new(format!(
                "任务库版本 {version} 高于程序支持的 {SCHEMA_VERSION}"
            )));
        }
        if version < SCHEMA_VERSION {
            migrate(&conn, version)?;
        }
        Ok(Self {
            conn,
            path: path.to_path_buf(),
        })
    }

    #[cfg(test)]
    fn open_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory().map_err(err)?;
        migrate(&conn, 0)?;
        Ok(Self {
            conn,
            path: PathBuf::new(),
        })
    }
}

fn migrate(conn: &Connection, version: i64) -> Result<(), StoreError> {
    if version < 1 {
        return conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS tasks (
                    id            INTEGER PRIMARY KEY,
                    text          TEXT NOT NULL,
                    status        TEXT NOT NULL,
                    due           TEXT,
                    parent        INTEGER,
                    blocked_by    TEXT NOT NULL DEFAULT '[]',
                    day           TEXT NOT NULL,
                    reminded_due  TEXT,
                    important     INTEGER NOT NULL DEFAULT 0,
                    repeat        TEXT,
                    repeat_anchor TEXT,
                    created_at    TEXT,
                    updated_at    TEXT,
                    completed_at  TEXT,
                    subtasks      TEXT NOT NULL DEFAULT '[]'
                );
                CREATE TABLE IF NOT EXISTS meta (
                    key   TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );
                PRAGMA user_version = 4;",
            )
            .map_err(err);
    }
    if version < 2 {
        conn.execute_batch(
            "ALTER TABLE tasks ADD COLUMN important INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE tasks ADD COLUMN repeat TEXT;
             ALTER TABLE tasks ADD COLUMN repeat_anchor TEXT;
             CREATE TABLE IF NOT EXISTS meta (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
             );
             PRAGMA user_version = 2;",
        )
        .map_err(err)?;
    }
    if version < 3 {
        // 旧版本没有时间戳：创建/更新时间用创建日补 00:00，完成时间无法还原
        conn.execute_batch(
            "ALTER TABLE tasks ADD COLUMN created_at TEXT;
             ALTER TABLE tasks ADD COLUMN updated_at TEXT;
             ALTER TABLE tasks ADD COLUMN completed_at TEXT;
             UPDATE tasks SET created_at = day || ' 00:00' WHERE created_at IS NULL;
             UPDATE tasks SET updated_at = created_at WHERE updated_at IS NULL;
             PRAGMA user_version = 3;",
        )
        .map_err(err)?;
    }
    if version < 4 {
        // 子任务从独立任务行改为任务内嵌清单；parent 列保留只为读旧数据
        conn.execute_batch(
            "ALTER TABLE tasks ADD COLUMN subtasks TEXT NOT NULL DEFAULT '[]';
             PRAGMA user_version = 4;",
        )
        .map_err(err)?;
    }
    Ok(())
}

fn parse_repeat(s: &str) -> Option<Repeat> {
    serde_json::from_str(s).ok().or_else(|| Repeat::parse(s))
}

impl TaskStore for SqliteTaskStore {
    fn load(&mut self) -> Result<Vec<TodoItem>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, text, status, due, parent, blocked_by, day, reminded_due,
                        important, repeat, repeat_anchor, created_at, updated_at, completed_at,
                        subtasks
                 FROM tasks ORDER BY id",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map([], |row| {
                let repeat: Option<String> = row.get(9)?;
                Ok(TodoItem::from_record(TaskRecord {
                    id: row.get::<_, i64>(0)? as u64,
                    text: row.get(1)?,
                    status: parse_status(&row.get::<_, String>(2)?),
                    due: row.get(3)?,
                    parent: row.get::<_, Option<i64>>(4)?.map(|p| p as u64),
                    blocked_by: serde_json::from_str(&row.get::<_, String>(5)?).unwrap_or_default(),
                    day: row.get(6)?,
                    created_at: row.get(11)?,
                    updated_at: row.get(12)?,
                    completed_at: row.get(13)?,
                    reminded_due: row.get(7)?,
                    important: row.get::<_, i64>(8)? != 0,
                    repeat: repeat.as_deref().and_then(parse_repeat),
                    repeat_anchor: row.get(10)?,
                    subtasks: serde_json::from_str(&row.get::<_, String>(14)?).unwrap_or_default(),
                }))
            })
            .map_err(err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(err)
    }

    fn apply(&mut self, changes: TaskChanges) -> Result<(), StoreError> {
        let tx = self.conn.transaction().map_err(err)?;
        {
            let mut delete = tx
                .prepare_cached("DELETE FROM tasks WHERE id = ?1")
                .map_err(err)?;
            for id in &changes.deletes {
                delete.execute(params![*id as i64]).map_err(err)?;
            }
            let mut upsert = tx
                .prepare_cached(
                    "INSERT INTO tasks (id, text, status, due, parent, blocked_by, day,
                                        reminded_due, important, repeat, repeat_anchor,
                                        created_at, updated_at, completed_at, subtasks)
                     VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                     ON CONFLICT(id) DO UPDATE SET
                        text = excluded.text,
                        status = excluded.status,
                        due = excluded.due,
                        parent = NULL,
                        blocked_by = excluded.blocked_by,
                        day = excluded.day,
                        reminded_due = excluded.reminded_due,
                        important = excluded.important,
                        repeat = excluded.repeat,
                        repeat_anchor = excluded.repeat_anchor,
                        created_at = excluded.created_at,
                        updated_at = excluded.updated_at,
                        completed_at = excluded.completed_at,
                        subtasks = excluded.subtasks",
                )
                .map_err(err)?;
            for t in &changes.upserts {
                let blocked_by = serde_json::to_string(&t.blocked_by)
                    .map_err(|e| StoreError::new(e.to_string()))?;
                let repeat = t
                    .repeat
                    .map(|r| serde_json::to_string(&r))
                    .transpose()
                    .map_err(|e| StoreError::new(e.to_string()))?;
                let subtasks = serde_json::to_string(&t.subtasks)
                    .map_err(|e| StoreError::new(e.to_string()))?;
                upsert
                    .execute(params![
                        t.id as i64,
                        t.text,
                        status_str(t.status),
                        t.due,
                        blocked_by,
                        t.day,
                        t.reminded_due,
                        t.important as i64,
                        repeat,
                        t.repeat_anchor,
                        t.created_at,
                        t.updated_at,
                        t.completed_at,
                        subtasks,
                    ])
                    .map_err(err)?;
            }
        }
        tx.commit().map_err(err)
    }

    fn allocate_id(&mut self) -> Result<u64, StoreError> {
        let tx = self.conn.transaction().map_err(err)?;
        let current: Option<i64> = tx
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'next_id'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
        let max: i64 = tx
            .query_row("SELECT COALESCE(MAX(id), 0) FROM tasks", [], |r| r.get(0))
            .map_err(err)?;
        let id = current.unwrap_or(1).max(max + 1).max(1);
        tx.execute(
            "INSERT INTO meta (key, value) VALUES ('next_id', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![(id + 1).to_string()],
        )
        .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(id as u64)
    }

    fn watch_paths(&self) -> Vec<PathBuf> {
        let mut wal = self.path.as_os_str().to_os_string();
        wal.push("-wal");
        let mut shm = self.path.as_os_str().to_os_string();
        shm.push("-shm");
        vec![self.path.clone(), PathBuf::from(wal), PathBuf::from(shm)]
    }
}

fn status_str(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Todo => "Todo",
        TaskStatus::Doing => "Doing",
        TaskStatus::Done => "Done",
    }
}

fn parse_status(s: &str) -> TaskStatus {
    match s {
        "Doing" => TaskStatus::Doing,
        "Done" => TaskStatus::Done,
        _ => TaskStatus::Todo,
    }
}

fn err(e: impl std::fmt::Display) -> StoreError {
    StoreError::new(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Subtask;

    fn item(id: u64, text: &str) -> TodoItem {
        TodoItem::new(id, text.to_owned(), "2026-10-20".to_owned())
    }

    #[test]
    fn roundtrip_keeps_all_fields() {
        let mut store = SqliteTaskStore::open_in_memory().unwrap();
        let mut a = item(7, "写周报\n第二行");
        a.status = TaskStatus::Doing;
        a.due = Some("2026-10-21 18:00".into());
        a.subtasks = vec![
            Subtask {
                text: "子项一".into(),
                done: true,
            },
            Subtask::new("子项二".into()),
        ];
        a.blocked_by = vec![4, 5];
        a.reminded_due = Some("2026-10-21 18:00".into());
        a.important = true;
        a.repeat = Some(Repeat::new(crate::state::RepeatUnit::Week, 2));
        a.repeat_anchor = Some("2026-10-21 18:00".into());
        a.created_at = Some("2026-10-20 08:00".into());
        a.updated_at = Some("2026-10-21 09:30".into());
        a.completed_at = Some("2026-10-21 09:30".into());
        store
            .apply(TaskChanges {
                upserts: vec![a.clone()],
                deletes: vec![],
            })
            .unwrap();
        assert_eq!(store.load().unwrap(), vec![a]);
    }

    #[test]
    fn update_and_delete() {
        let mut store = SqliteTaskStore::open_in_memory().unwrap();
        store
            .apply(TaskChanges {
                upserts: vec![item(1, "A"), item(2, "B")],
                deletes: vec![],
            })
            .unwrap();
        let mut a = item(1, "A 改");
        a.status = TaskStatus::Done;
        store
            .apply(TaskChanges {
                upserts: vec![a.clone()],
                deletes: vec![2],
            })
            .unwrap();
        assert_eq!(store.load().unwrap(), vec![a]);
    }

    #[test]
    fn broken_values_degrade_instead_of_failing_load() {
        let mut store = SqliteTaskStore::open_in_memory().unwrap();
        store
            .conn
            .execute(
                "INSERT INTO tasks (id, text, status, due, parent, blocked_by, day, reminded_due)
                 VALUES (1, 'x', '???', NULL, NULL, 'not json', '2026-10-20', NULL)",
                [],
            )
            .unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded[0].status, TaskStatus::Todo);
        assert!(loaded[0].blocked_by.is_empty());
        assert!(!loaded[0].important);
        assert!(loaded[0].repeat.is_none());
    }

    #[test]
    fn v1_database_is_migrated_in_place() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE tasks (
                id INTEGER PRIMARY KEY, text TEXT NOT NULL, status TEXT NOT NULL,
                due TEXT, parent INTEGER, blocked_by TEXT NOT NULL DEFAULT '[]',
                day TEXT NOT NULL, reminded_due TEXT
             );
             INSERT INTO tasks (id, text, status, blocked_by, day)
             VALUES (4, '旧任务', 'Doing', '[]', '2026-10-20');
             PRAGMA user_version = 1;",
        )
        .unwrap();
        migrate(&conn, 1).unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, 4);
        let mut store = SqliteTaskStore {
            conn,
            path: PathBuf::new(),
        };
        let tasks = store.load().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].text, "旧任务");
        assert!(!tasks[0].important);
        assert!(tasks[0].repeat.is_none());
        assert!(tasks[0].subtasks.is_empty());
        assert_eq!(tasks[0].created_at.as_deref(), Some("2026-10-20 00:00"));
        assert_eq!(tasks[0].updated_at.as_deref(), Some("2026-10-20 00:00"));
        assert!(tasks[0].completed_at.is_none());
    }

    #[test]
    fn v3_database_gets_subtasks_column() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE tasks (
                id INTEGER PRIMARY KEY, text TEXT NOT NULL, status TEXT NOT NULL,
                due TEXT, parent INTEGER, blocked_by TEXT NOT NULL DEFAULT '[]',
                day TEXT NOT NULL, reminded_due TEXT, important INTEGER NOT NULL DEFAULT 0,
                repeat TEXT, repeat_anchor TEXT, created_at TEXT, updated_at TEXT,
                completed_at TEXT
             );
             INSERT INTO tasks (id, text, status, blocked_by, day, parent, created_at, updated_at)
             VALUES (1, '根', 'Todo', '[]', '2026-10-20', NULL, '2026-10-20 00:00',
                     '2026-10-20 00:00');
             INSERT INTO tasks (id, text, status, blocked_by, day, parent, created_at, updated_at)
             VALUES (2, '旧子任务', 'Done', '[]', '2026-10-20', 1, '2026-10-20 00:00',
                     '2026-10-20 00:00');
             PRAGMA user_version = 3;",
        )
        .unwrap();
        migrate(&conn, 3).unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, 4);
        let mut store = SqliteTaskStore {
            conn,
            path: PathBuf::new(),
        };
        let tasks = store.load().unwrap();
        assert_eq!(tasks.len(), 2);
        assert!(tasks[0].subtasks.is_empty());
        assert!(tasks[1].has_legacy_parent());

        // 下一次保存把旧关系清掉，parent 列不再写
        store
            .apply(TaskChanges {
                upserts: tasks,
                deletes: vec![],
            })
            .unwrap();
        let parent: Option<i64> = store
            .conn
            .query_row("SELECT parent FROM tasks WHERE id = 2", [], |r| r.get(0))
            .unwrap();
        assert_eq!(parent, None);
    }

    #[test]
    fn v2_database_gets_timestamps_without_losing_fields() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE tasks (
                id INTEGER PRIMARY KEY, text TEXT NOT NULL, status TEXT NOT NULL,
                due TEXT, parent INTEGER, blocked_by TEXT NOT NULL DEFAULT '[]',
                day TEXT NOT NULL, reminded_due TEXT, important INTEGER NOT NULL DEFAULT 0,
                repeat TEXT, repeat_anchor TEXT
             );
             INSERT INTO tasks (id, text, status, blocked_by, day, important)
             VALUES (1, '待办', 'Todo', '[]', '2026-10-19', 1);
             INSERT INTO tasks (id, text, status, blocked_by, day, important)
             VALUES (2, '已完成', 'Done', '[]', '2026-10-18', 0);
             PRAGMA user_version = 2;",
        )
        .unwrap();
        migrate(&conn, 2).unwrap();
        let mut store = SqliteTaskStore {
            conn,
            path: PathBuf::new(),
        };
        let tasks = store.load().unwrap();
        assert_eq!(tasks.len(), 2);
        assert!(tasks[0].important);
        assert_eq!(tasks[0].created_at.as_deref(), Some("2026-10-19 00:00"));
        assert_eq!(tasks[1].completed_at, None);
    }

    #[test]
    fn allocated_ids_never_collide_with_existing_rows() {
        let mut store = SqliteTaskStore::open_in_memory().unwrap();
        assert_eq!(store.allocate_id().unwrap(), 1);
        store
            .apply(TaskChanges {
                upserts: vec![item(5, "已有")],
                deletes: vec![],
            })
            .unwrap();
        assert_eq!(store.allocate_id().unwrap(), 6);
        assert_eq!(store.allocate_id().unwrap(), 7);
    }
}
