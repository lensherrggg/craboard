use super::{StoreError, TaskChanges, TaskStore, apply_to};
use crate::state::TodoItem;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 任务单独存一份 JSON；SQLite 打不开时兜底，也方便手工查看。
/// 新格式是 `{ next_id, tasks }`，仍能读旧的裸数组。
#[derive(Default, Serialize, Deserialize)]
struct JsonFile {
    #[serde(default)]
    next_id: u64,
    #[serde(default)]
    tasks: Vec<TodoItem>,
}

pub struct JsonTaskStore {
    path: PathBuf,
    next_id: u64,
    tasks: Vec<TodoItem>,
}

impl JsonTaskStore {
    pub fn open(path: PathBuf) -> Result<Self, StoreError> {
        let mut store = Self {
            path,
            next_id: 1,
            tasks: Vec::new(),
        };
        store.read()?;
        Ok(store)
    }

    fn read(&mut self) -> Result<(), StoreError> {
        let body = match std::fs::read_to_string(&self.path) {
            Ok(body) => body,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.tasks.clear();
                self.next_id = 1;
                return Ok(());
            }
            Err(e) => return Err(self.io_err(e)),
        };
        let value: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| StoreError::new(format!("{}: {e}", self.path.display())))?;
        if value.is_array() {
            self.tasks = serde_json::from_value(value)
                .map_err(|e| StoreError::new(format!("{}: {e}", self.path.display())))?;
            self.next_id = 1;
        } else {
            let file: JsonFile = serde_json::from_value(value)
                .map_err(|e| StoreError::new(format!("{}: {e}", self.path.display())))?;
            self.tasks = file.tasks;
            self.next_id = file.next_id;
        }
        self.next_id = self
            .next_id
            .max(self.tasks.iter().map(|t| t.id).max().unwrap_or(0) + 1)
            .max(1);
        Ok(())
    }

    fn write(&self) -> Result<(), StoreError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| self.io_err(e))?;
        }
        let tmp = self.path.with_extension("json.tmp");
        let body = serde_json::to_string_pretty(&JsonFile {
            next_id: self.next_id,
            tasks: self.tasks.clone(),
        })
        .map_err(|e| StoreError::new(e.to_string()))?;
        std::fs::write(&tmp, body).map_err(|e| self.io_err(e))?;
        if self.path.exists() {
            std::fs::remove_file(&self.path).map_err(|e| self.io_err(e))?;
        }
        std::fs::rename(&tmp, &self.path).map_err(|e| self.io_err(e))
    }

    fn io_err(&self, e: std::io::Error) -> StoreError {
        StoreError::new(format!("{}: {e}", self.path.display()))
    }
}

impl TaskStore for JsonTaskStore {
    fn load(&mut self) -> Result<Vec<TodoItem>, StoreError> {
        self.read()?;
        Ok(self.tasks.clone())
    }

    fn apply(&mut self, changes: TaskChanges) -> Result<(), StoreError> {
        apply_to(&mut self.tasks, changes);
        self.write()
    }

    fn allocate_id(&mut self) -> Result<u64, StoreError> {
        self.read()?;
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        self.write()?;
        Ok(id)
    }

    fn watch_paths(&self) -> Vec<PathBuf> {
        vec![self.path.clone()]
    }
}
