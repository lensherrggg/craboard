use super::{StoreError, TaskChanges, TaskStore, apply_to};
use crate::state::TodoItem;

/// 最后一道兜底：磁盘完全不可写时也让界面继续可用。
#[derive(Default)]
pub struct MemoryTaskStore {
    tasks: Vec<TodoItem>,
    next_id: u64,
}

impl TaskStore for MemoryTaskStore {
    fn load(&mut self) -> Result<Vec<TodoItem>, StoreError> {
        Ok(self.tasks.clone())
    }

    fn apply(&mut self, changes: TaskChanges) -> Result<(), StoreError> {
        apply_to(&mut self.tasks, changes);
        Ok(())
    }

    fn allocate_id(&mut self) -> Result<u64, StoreError> {
        let id = self
            .next_id
            .max(self.tasks.iter().map(|t| t.id).max().unwrap_or(0) + 1)
            .max(1);
        self.next_id = id + 1;
        Ok(id)
    }
}
