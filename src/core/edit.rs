//! 任务编辑规则：关系校验、周期基准和状态迁移。
//!
//! 保存走「先校验、再在副本上改、最后整体提交」，出错时原状态一个字段都不动。

use crate::state::{
    Repeat, Settings, Subtask, TaskStatus, TodoItem, format_due, normalize_due, parse_due,
};
use chrono::{Duration, NaiveDateTime};

/// 表单里的子项可能带着空行；保存时去掉首尾空白并丢弃空项。
pub fn clean_subtasks(items: &[Subtask]) -> Vec<Subtask> {
    items
        .iter()
        .filter_map(|s| {
            let text = s.text.trim().to_owned();
            (!text.is_empty()).then_some(Subtask { text, done: s.done })
        })
        .collect()
}

pub fn validate_dep(st: &Settings, id: u64, dep: u64) -> Result<(), String> {
    if dep == id {
        return Err("任务不能以自己为前置".to_owned());
    }
    if st.task(dep).is_none() {
        return Err(format!("前置任务 #{dep} 不存在"));
    }
    if st.dep_would_cycle(id, dep) {
        return Err("前置关系会形成环".to_owned());
    }
    Ok(())
}

/// GUI 编辑弹窗、将来的 TUI/Web 表单共用的草稿。
#[derive(Clone, Debug)]
pub struct TaskEditDraft {
    /// 新建任务时为 0，保存时由 Core 分配
    pub id: u64,
    pub text: String,
    pub due: String,
    pub status: TaskStatus,
    pub subtasks: Vec<Subtask>,
    pub blocked_by: Vec<u64>,
    pub important: bool,
    pub repeat: Option<Repeat>,
    /// 用快捷延期改过时间：保存时保留周期基准
    pub postponed: bool,
}

impl TaskEditDraft {
    pub fn from_task(t: &TodoItem) -> Self {
        Self {
            id: t.id,
            text: t.text.clone(),
            due: t.due.clone().unwrap_or_default(),
            status: t.status,
            subtasks: t.subtasks.clone(),
            blocked_by: t.blocked_by.clone(),
            important: t.important,
            repeat: t.repeat,
            postponed: false,
        }
    }

    pub fn new_task(text: String) -> Self {
        Self {
            id: 0,
            text,
            due: String::new(),
            status: TaskStatus::Todo,
            subtasks: Vec::new(),
            blocked_by: Vec::new(),
            important: false,
            repeat: None,
            postponed: false,
        }
    }

    pub fn postpone_days(&mut self, days: i64, now: NaiveDateTime) {
        let base = parse_due(&self.due).filter(|d| *d > now).unwrap_or(now);
        self.due = format_due(base + Duration::days(days));
        self.postponed = true;
    }

    /// 返回周期任务本轮完成后的下一轮时间。
    pub fn save(
        &self,
        st: &mut Settings,
        now: NaiveDateTime,
    ) -> Result<Option<NaiveDateTime>, String> {
        let text = self.text.trim();
        if text.is_empty() {
            return Err("任务内容不能为空".to_owned());
        }
        let due = normalize_due(&self.due)
            .map_err(|_| "截止时间格式不对，示例：2026-10-21 或 2026-10-21 18:00".to_owned())?;
        if self.repeat.is_some() && due.is_none() {
            return Err("周期任务需要设置截止时间".to_owned());
        }
        for dep in &self.blocked_by {
            validate_dep(st, self.id, *dep)?;
        }

        let mut next = st.clone();
        let Some(t) = next.todos.iter_mut().find(|t| t.id == self.id) else {
            return Err("任务不存在".to_owned());
        };
        let old_status = t.status;
        let due_changed = t.due != due;
        let valid_anchor = t.repeat_anchor.as_deref().and_then(parse_due).is_some();
        t.text = text.to_owned();
        if due_changed {
            t.reminded_due = None;
        }
        t.due = due.clone();
        t.important = self.important;
        t.repeat = self.repeat;
        match t.repeat {
            None => t.repeat_anchor = None,
            Some(_) => {
                // 手工改截止时间等于调整周期计划；快捷延期只改本轮
                if !valid_anchor || (due_changed && !self.postponed) {
                    t.repeat_anchor = due;
                }
            }
        }
        t.subtasks = clean_subtasks(&self.subtasks);
        t.blocked_by = self.blocked_by.clone();
        t.blocked_by.sort_unstable();
        t.blocked_by.dedup();
        t.updated_at = Some(format_due(now));

        let next_due = if self.status != old_status {
            next.set_status(self.id, self.status, now)?
        } else {
            next.check_status(self.id, self.status)?;
            None
        };
        *st = next;
        Ok(next_due)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{RepeatUnit, TaskStatus};

    fn at(day: u32, h: u32, m: u32) -> NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 10, day)
            .unwrap()
            .and_hms_opt(h, m, 0)
            .unwrap()
    }

    fn task(id: u64, text: &str) -> TodoItem {
        TodoItem::new(id, text.to_owned(), "2026-10-20".to_owned())
    }

    #[test]
    fn cycle_error_leaves_every_field_untouched() {
        let a = task(1, "A");
        let mut b = task(2, "B");
        b.blocked_by = vec![1];
        let mut st = Settings::default();
        st.todos = vec![a, b];

        let mut draft = TaskEditDraft::from_task(&st.todos[0]);
        draft.text = "A 改".into();
        draft.blocked_by = vec![2];
        assert!(draft.save(&mut st, at(20, 10, 0)).is_err());

        assert_eq!(st.todos[0].text, "A");
        assert!(st.todos[0].blocked_by.is_empty());
    }

    #[test]
    fn quick_postpone_keeps_anchor_but_manual_due_change_moves_it() {
        let mut t = task(1, "周期");
        t.due = Some("2026-10-20 09:00".into());
        t.repeat = Some(Repeat::new(RepeatUnit::Day, 1));
        t.repeat_anchor = Some("2026-10-20 09:00".into());
        let mut st = Settings::default();
        st.todos = vec![t];

        let mut draft = TaskEditDraft::from_task(&st.todos[0]);
        draft.postpone_days(1, at(20, 8, 0));
        draft.save(&mut st, at(20, 8, 0)).unwrap();
        assert_eq!(st.todos[0].due.as_deref(), Some("2026-10-21 09:00"));
        assert_eq!(
            st.todos[0].repeat_anchor.as_deref(),
            Some("2026-10-20 09:00")
        );

        let mut draft = TaskEditDraft::from_task(&st.todos[0]);
        draft.due = "2026-10-23 09:00".into();
        draft.save(&mut st, at(20, 8, 0)).unwrap();
        assert_eq!(
            st.todos[0].repeat_anchor.as_deref(),
            Some("2026-10-23 09:00")
        );
    }

    #[test]
    fn recurring_task_saves_and_cleans_subtasks() {
        let t = task(1, "周期");
        let mut st = Settings::default();
        st.todos = vec![t];

        let mut draft = TaskEditDraft::from_task(&st.todos[0]);
        draft.due = "2026-10-21 09:00".into();
        draft.repeat = Some(Repeat::new(RepeatUnit::Day, 1));
        draft.subtasks = vec![
            Subtask {
                text: " 写提纲 ".into(),
                done: false,
            },
            Subtask::new("   ".into()),
            Subtask {
                text: "复核".into(),
                done: true,
            },
        ];
        draft.save(&mut st, at(20, 8, 0)).unwrap();
        assert!(st.todos[0].repeat.is_some());
        assert_eq!(st.todos[0].subtasks.len(), 2);
        assert_eq!(st.todos[0].subtasks[0].text, "写提纲");
        assert!(st.todos[0].subtasks[1].done);
    }

    #[test]
    fn invalid_due_or_repeat_does_not_touch_state() {
        let mut t = task(1, "任务");
        t.due = Some("2026-10-21 09:00".into());
        let mut st = Settings::default();
        st.todos = vec![t];

        let mut draft = TaskEditDraft::from_task(&st.todos[0]);
        draft.due = "明天".into();
        assert!(draft.save(&mut st, at(20, 8, 0)).is_err());
        assert_eq!(st.todos[0].due.as_deref(), Some("2026-10-21 09:00"));

        let mut draft = TaskEditDraft::from_task(&st.todos[0]);
        draft.due.clear();
        draft.repeat = Some(Repeat::new(RepeatUnit::Week, 1));
        assert!(draft.save(&mut st, at(20, 8, 0)).is_err());
        assert_eq!(st.todos[0].due.as_deref(), Some("2026-10-21 09:00"));
        assert!(st.todos[0].repeat.is_none());
    }

    #[test]
    fn completing_in_editor_advances_recurring_task_and_resets_subtasks() {
        let mut t = task(1, "周期");
        t.due = Some("2026-10-20 09:00".into());
        t.repeat = Some(Repeat::new(RepeatUnit::Day, 1));
        t.repeat_anchor = Some("2026-10-20 09:00".into());
        t.subtasks = vec![Subtask {
            text: "本轮子项".into(),
            done: true,
        }];
        let mut st = Settings::default();
        st.todos = vec![t];

        let mut draft = TaskEditDraft::from_task(&st.todos[0]);
        draft.status = TaskStatus::Done;
        let next = draft.save(&mut st, at(20, 8, 0)).unwrap();
        assert_eq!(next, parse_due("2026-10-21 09:00"));
        assert_eq!(st.todos[0].status, TaskStatus::Todo);
        assert!(!st.todos[0].subtasks[0].done);
    }
}
