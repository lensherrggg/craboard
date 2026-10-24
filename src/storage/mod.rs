mod json;
mod memory;
mod sqlite;

use crate::state::TodoItem;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::PathBuf;

pub use json::JsonTaskStore;
pub use memory::MemoryTaskStore;
pub use sqlite::SqliteTaskStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError(String);

impl StoreError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for StoreError {}

/// 一次落盘只带发生变化的条目，后端可以按批处理
#[derive(Default, Debug, Clone)]
pub struct TaskChanges {
    pub upserts: Vec<TodoItem>,
    pub deletes: Vec<u64>,
}

impl TaskChanges {
    pub fn is_empty(&self) -> bool {
        self.upserts.is_empty() && self.deletes.is_empty()
    }
}

/// 任务数据的持久化接口。以后换数据库或接远端同步，只需再实现一份。
pub trait TaskStore {
    fn load(&mut self) -> Result<Vec<TodoItem>, StoreError>;
    fn apply(&mut self, changes: TaskChanges) -> Result<(), StoreError>;
    /// id 由存储层统一分配，GUI 和命令行同时写入也不会撞号
    fn allocate_id(&mut self) -> Result<u64, StoreError>;
    /// 供界面监听外部改动；内存后端不提供
    fn watch_paths(&self) -> Vec<PathBuf> {
        Vec::new()
    }
}

/// 在内存数组上执行一批变更，给 JSON / 内存后端复用
fn apply_to(tasks: &mut Vec<TodoItem>, changes: TaskChanges) {
    let deleted: HashSet<u64> = changes.deletes.into_iter().collect();
    tasks.retain(|t| !deleted.contains(&t.id));
    for item in changes.upserts {
        match tasks.iter_mut().find(|t| t.id == item.id) {
            Some(t) => *t = item,
            None => tasks.push(item),
        }
    }
}

pub struct TaskRepository {
    store: Box<dyn TaskStore>,
    persisted: HashMap<u64, TodoItem>,
}

impl TaskRepository {
    pub fn new(store: Box<dyn TaskStore>) -> Self {
        Self {
            store,
            persisted: HashMap::new(),
        }
    }

    /// 读取任务。后端还是空库时，把旧 JSON 存档里的任务导入一次。
    pub fn load(&mut self, legacy: &[TodoItem]) -> Result<Vec<TodoItem>, StoreError> {
        let mut tasks = self.store.load()?;
        if tasks.is_empty() && !legacy.is_empty() {
            self.store.apply(TaskChanges {
                upserts: legacy.to_vec(),
                deletes: Vec::new(),
            })?;
            tasks = legacy.to_vec();
        }
        self.persisted = tasks.iter().map(|t| (t.id, t.clone())).collect();
        Ok(tasks)
    }

    pub fn allocate_id(&mut self) -> Result<u64, StoreError> {
        self.store.allocate_id()
    }

    /// 外部（命令行 / MCP）改过库之后重新读取，并重建落盘快照
    pub fn reload(&mut self) -> Result<Vec<TodoItem>, StoreError> {
        let tasks = self.store.load()?;
        self.persisted = tasks.iter().map(|t| (t.id, t.clone())).collect();
        Ok(tasks)
    }

    pub fn watch_paths(&self) -> Vec<PathBuf> {
        self.store.watch_paths()
    }

    /// 和上次落盘的内容比对，只写有变化的条目。返回是否产生了写入。
    pub fn sync(&mut self, todos: &[TodoItem]) -> Result<bool, StoreError> {
        let ids: HashSet<u64> = todos.iter().map(|t| t.id).collect();
        let deletes: Vec<u64> = self
            .persisted
            .keys()
            .copied()
            .filter(|id| !ids.contains(id))
            .collect();
        let upserts: Vec<TodoItem> = todos
            .iter()
            .filter(|t| self.persisted.get(&t.id) != Some(*t))
            .cloned()
            .collect();
        let changes = TaskChanges { upserts, deletes };
        if changes.is_empty() {
            return Ok(false);
        }
        self.store.apply(changes)?;
        self.persisted = todos.iter().map(|t| (t.id, t.clone())).collect();
        Ok(true)
    }
}

/// 默认后端：优先 SQLite；打不开就退回独立 JSON 文件；再不行只放内存。
/// 返回读到的任务，以及需要写进日志的告警。
pub fn open_default(legacy: &[TodoItem]) -> (TaskRepository, Vec<TodoItem>, Option<String>) {
    let dir = crate::state::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let mut warnings = Vec::new();

    match SqliteTaskStore::open(&dir.join("tasks.db")) {
        Ok(store) => {
            let mut repo = TaskRepository::new(Box::new(store));
            match repo.load(legacy) {
                Ok(tasks) => return (repo, tasks, None),
                Err(e) => warnings.push(format!("SQLite 读取失败：{e}")),
            }
        }
        Err(e) => warnings.push(format!("SQLite 不可用：{e}")),
    }

    match JsonTaskStore::open(dir.join("tasks.json")) {
        Ok(store) => {
            let mut repo = TaskRepository::new(Box::new(store));
            match repo.load(legacy) {
                Ok(tasks) => {
                    warnings.push("任务改存 tasks.json".to_owned());
                    return (repo, tasks, Some(warnings.join("；")));
                }
                Err(e) => warnings.push(format!("tasks.json 读取失败：{e}")),
            }
        }
        Err(e) => warnings.push(format!("tasks.json 不可用：{e}")),
    }

    warnings.push("任务只保存在内存，退出后会丢失".to_owned());
    let mut repo = TaskRepository::new(Box::new(MemoryTaskStore::default()));
    let tasks = repo.load(legacy).unwrap_or_else(|_| legacy.to_vec());
    (repo, tasks, Some(warnings.join("；")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn item(id: u64, text: &str) -> TodoItem {
        TodoItem::new(id, text.to_owned(), "2026-10-20".to_owned())
    }

    struct SpyStore {
        tasks: Vec<TodoItem>,
        batches: Rc<RefCell<Vec<TaskChanges>>>,
    }

    impl TaskStore for SpyStore {
        fn load(&mut self) -> Result<Vec<TodoItem>, StoreError> {
            Ok(self.tasks.clone())
        }

        fn apply(&mut self, changes: TaskChanges) -> Result<(), StoreError> {
            self.batches.borrow_mut().push(changes.clone());
            apply_to(&mut self.tasks, changes);
            Ok(())
        }

        fn allocate_id(&mut self) -> Result<u64, StoreError> {
            let id = self.tasks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
            Ok(id)
        }
    }

    fn make_repo(tasks: Vec<TodoItem>) -> (TaskRepository, Rc<RefCell<Vec<TaskChanges>>>) {
        let batches = Rc::new(RefCell::new(Vec::new()));
        let store = SpyStore {
            tasks,
            batches: batches.clone(),
        };
        let mut repo = TaskRepository::new(Box::new(store));
        repo.load(&[]).unwrap();
        (repo, batches)
    }

    #[test]
    fn legacy_tasks_import_only_into_an_empty_backend() {
        let legacy = vec![item(1, "旧任务")];
        let (mut repo, batches) = make_repo(vec![]);
        assert_eq!(repo.load(&legacy).unwrap(), legacy);
        assert_eq!(batches.borrow().len(), 1);
        assert_eq!(batches.borrow()[0].upserts.len(), 1);

        let (mut repo, batches) = make_repo(vec![item(2, "库里的")]);
        let loaded = repo.load(&legacy).unwrap();
        assert_eq!(loaded[0].id, 2);
        assert!(batches.borrow().is_empty());
    }

    #[test]
    fn sync_writes_only_the_diff() {
        let (mut repo, batches) = make_repo(vec![item(1, "A"), item(2, "B")]);
        let mut changed = item(1, "A 改");
        changed.status = crate::state::TaskStatus::Done;
        let next = vec![changed.clone(), item(3, "C")];

        assert!(repo.sync(&next).unwrap());
        {
            let batches = batches.borrow();
            assert_eq!(batches.len(), 1);
            let mut ids: Vec<u64> = batches[0].upserts.iter().map(|t| t.id).collect();
            ids.sort_unstable();
            assert_eq!(ids, vec![1, 3]);
            assert_eq!(batches[0].deletes, vec![2]);
            assert_eq!(batches[0].upserts[0], changed);
        }

        assert!(!repo.sync(&next).unwrap());
        assert_eq!(batches.borrow().len(), 1);
    }

    #[test]
    fn json_backend_roundtrips_and_removes() {
        let dir = std::env::temp_dir().join(format!(
            "craboard-store-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join("tasks.json");

        let mut a = item(1, "带\n换行和\"引号\"");
        a.blocked_by = vec![9];
        let mut store = JsonTaskStore::open(path.clone()).unwrap();
        store
            .apply(TaskChanges {
                upserts: vec![a.clone(), item(2, "B")],
                deletes: vec![],
            })
            .unwrap();

        let mut again = JsonTaskStore::open(path.clone()).unwrap();
        assert_eq!(again.load().unwrap(), vec![a.clone(), item(2, "B")]);

        again
            .apply(TaskChanges {
                upserts: vec![],
                deletes: vec![1],
            })
            .unwrap();
        let loaded = JsonTaskStore::open(path).unwrap().load().unwrap();
        assert_eq!(loaded, vec![item(2, "B")]);
        let _ = std::fs::remove_dir_all(dir);
    }
}
