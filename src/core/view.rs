//! 与具体前端无关的派生视图：今日口径、列表可见行、看板分列、四象限分组。
//!
//! 这里只接收领域数据并返回索引/只读引用，不碰 egui，也不写回状态，
//! 后续 TUI/Web 可以直接复用同一套规则。

use crate::state::{Quadrant, Repeat, Settings, Subtask, TaskStatus, View};
use chrono::{Datelike, NaiveDateTime};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ListFilter {
    #[default]
    Active,
    All,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ListScope {
    Today,
    All,
}

impl ListScope {
    pub fn of(view: View) -> Self {
        match view {
            View::Today => Self::Today,
            _ => Self::All,
        }
    }
}

/// 每帧算一次的派生信息，避免列表/看板里反复扫描任务关系。
#[derive(Clone, Debug)]
pub struct TaskInfo {
    pub id: u64,
    pub text: String,
    pub status: TaskStatus,
    pub due: Option<NaiveDateTime>,
    pub overdue: bool,
    /// 截止今天；无截止的任务看创建日
    pub today: bool,
    pub blocked_left: usize,
    pub subtasks: Vec<Subtask>,
    pub important: bool,
    pub repeat: Option<Repeat>,
    pub quadrant: Quadrant,
}

impl TaskInfo {
    pub fn is_done(&self) -> bool {
        self.status == TaskStatus::Done
    }

    pub fn subtask_done(&self) -> usize {
        self.subtasks.iter().filter(|s| s.done).count()
    }

    pub fn subtask_total(&self) -> usize {
        self.subtasks.len()
    }
}

pub fn task_infos(st: &Settings) -> Vec<TaskInfo> {
    task_infos_at(st, chrono::Local::now().naive_local())
}

pub fn task_infos_at(st: &Settings, now: NaiveDateTime) -> Vec<TaskInfo> {
    let statuses: HashMap<u64, TaskStatus> = st.todos.iter().map(|t| (t.id, t.status)).collect();
    let today = now.date();
    let today_str = today.format("%Y-%m-%d").to_string();
    let urgent_hours = st.urgent_within_hours;
    st.todos
        .iter()
        .map(|t| {
            let due = t.due_at();
            TaskInfo {
                id: t.id,
                text: t.text.clone(),
                status: t.status,
                due,
                overdue: t.past_due(now),
                today: due.map_or(t.day == today_str, |d| d.date() == today),
                blocked_left: t
                    .blocked_by
                    .iter()
                    .filter(|id| statuses.get(id).is_some_and(|s| *s != TaskStatus::Done))
                    .count(),
                subtasks: t.subtasks.clone(),
                important: t.important,
                repeat: t.repeat,
                quadrant: t.quadrant(now, urgent_hours),
            }
        })
        .collect()
}

pub fn due_label(due: NaiveDateTime, now: NaiveDateTime) -> String {
    if due.date() == now.date() {
        format!("今天 {}", due.format("%H:%M"))
    } else if due.year() == now.year() {
        due.format("%m-%d %H:%M").to_string()
    } else {
        due.format("%Y-%m-%d %H:%M").to_string()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NavStats {
    pub open: usize,
    pub overdue: usize,
    pub today_total: usize,
    pub today_done: usize,
}

pub fn nav_stats(infos: &[TaskInfo]) -> NavStats {
    NavStats {
        open: infos.iter().filter(|i| !i.is_done()).count(),
        overdue: infos.iter().filter(|i| i.overdue).count(),
        today_total: infos.iter().filter(|i| i.today).count(),
        today_done: infos.iter().filter(|i| i.today && i.is_done()).count(),
    }
}

/// 「今天」当前口径：截止日在今天，或无截止且今天创建。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TodayProgress {
    pub done: usize,
    pub total: usize,
}

impl TodayProgress {
    pub fn fraction(self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            self.done as f32 / self.total as f32
        }
    }

    pub fn percent(self) -> usize {
        (self.done * 100).checked_div(self.total).unwrap_or(0)
    }
}

pub fn today_progress(infos: &[TaskInfo]) -> TodayProgress {
    TodayProgress {
        done: infos.iter().filter(|i| i.today && i.is_done()).count(),
        total: infos.iter().filter(|i| i.today).count(),
    }
}

/// 不调用任何模型，按当前任务数据现场拼一份总结，
/// 用来覆盖悬浮页里的总结正文；定时总结的开关和间隔不受影响。
pub fn instant_summary(infos: &[TaskInfo], now: NaiveDateTime) -> String {
    let progress = today_progress(infos);
    let open_today: Vec<&TaskInfo> = infos.iter().filter(|i| i.today && !i.is_done()).collect();
    let doing: Vec<&TaskInfo> = open_today
        .iter()
        .copied()
        .filter(|i| i.status == TaskStatus::Doing)
        .collect();
    let overdue: Vec<&TaskInfo> = infos.iter().filter(|i| i.overdue && !i.is_done()).collect();
    let important: Vec<&TaskInfo> = open_today
        .iter()
        .copied()
        .filter(|i| i.important && !i.overdue)
        .collect();
    let next_due = open_today
        .iter()
        .copied()
        .filter(|i| i.due.is_some() && !i.overdue)
        .min_by_key(|i| i.due);

    let mut lines = Vec::new();
    if progress.total == 0 {
        let open = infos.iter().filter(|i| !i.is_done()).count();
        if open == 0 {
            lines.push("今天没有安排任务，手头也没有未完成的任务。".to_owned());
        } else {
            lines.push(format!("今天没有安排任务，但还有 {open} 项未完成。"));
        }
    } else if progress.done == progress.total {
        lines.push(format!("今天的 {} 项任务已全部完成。", progress.total));
    } else {
        lines.push(format!(
            "今天共 {} 项任务，已完成 {} 项（{}%），还剩 {} 项。",
            progress.total,
            progress.done,
            progress.percent(),
            progress.total - progress.done
        ));
    }
    if !doing.is_empty() {
        lines.push(format!("进行中：{}。", task_names(&doing, 3)));
    }
    if !overdue.is_empty() {
        lines.push(format!(
            "已逾期 {} 项：{}。",
            overdue.len(),
            task_names(&overdue, 3)
        ));
    }
    if !important.is_empty() {
        lines.push(format!("重要任务待推进：{}。", task_names(&important, 3)));
    }
    if let Some(task) = next_due {
        if let Some(due) = task.due {
            lines.push(format!(
                "最近截止：{} · {}。",
                short_task_text(&task.text, 16),
                due_label(due, now)
            ));
        }
    }
    lines.join("\n")
}

fn task_names(tasks: &[&TaskInfo], limit: usize) -> String {
    let mut names = tasks
        .iter()
        .take(limit)
        .map(|t| short_task_text(&t.text, 16))
        .collect::<Vec<_>>()
        .join("、");
    if tasks.len() > limit {
        names.push_str(&format!(" 等 {} 项", tasks.len()));
    }
    names
}

/// 总结里只放任务首行，避免长文本把悬浮页撑爆
fn short_task_text(text: &str, max_chars: usize) -> String {
    let first = text.lines().next().unwrap_or("").trim();
    if first.chars().count() <= max_chars {
        return first.to_owned();
    }
    let mut out: String = first.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ListStats {
    pub scope_total: usize,
    pub open: usize,
    pub doing: usize,
    pub overdue: usize,
}

#[derive(Clone, Debug, Default)]
pub struct ListRows {
    /// infos 下标，按任务创建顺序
    pub rows: Vec<usize>,
    pub stats: ListStats,
}

/// 今天/全部任务视图的最终可见行；未完成筛选只影响行不影响统计。
pub fn list_rows(infos: &[TaskInfo], scope: ListScope, filter: ListFilter) -> ListRows {
    let in_scope = |info: &TaskInfo| match scope {
        ListScope::Today => info.today,
        ListScope::All => true,
    };
    let stats = ListStats {
        scope_total: infos.iter().filter(|i| in_scope(i)).count(),
        open: infos.iter().filter(|i| in_scope(i) && !i.is_done()).count(),
        doing: infos
            .iter()
            .filter(|i| in_scope(i) && i.status == TaskStatus::Doing)
            .count(),
        overdue: infos.iter().filter(|i| in_scope(i) && i.overdue).count(),
    };
    let rows = infos
        .iter()
        .enumerate()
        .filter(|(_, info)| in_scope(info))
        .filter(|(_, info)| filter == ListFilter::All || !info.is_done())
        .map(|(i, _)| i)
        .collect();
    ListRows { rows, stats }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KanbanColumn {
    Todo,
    Doing,
    Done,
    Overdue,
}

impl KanbanColumn {
    pub const ALL: [KanbanColumn; 4] = [
        KanbanColumn::Todo,
        KanbanColumn::Doing,
        KanbanColumn::Done,
        KanbanColumn::Overdue,
    ];

    pub fn index(self) -> usize {
        match self {
            KanbanColumn::Todo => 0,
            KanbanColumn::Doing => 1,
            KanbanColumn::Done => 2,
            KanbanColumn::Overdue => 3,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            KanbanColumn::Todo => "TODO",
            KanbanColumn::Doing => "DOING",
            KanbanColumn::Done => "DONE",
            KanbanColumn::Overdue => "OVERDUE",
        }
    }
}

pub fn in_column(info: &TaskInfo, col: KanbanColumn) -> bool {
    match col {
        KanbanColumn::Todo => info.status == TaskStatus::Todo && !info.overdue,
        KanbanColumn::Doing => info.status == TaskStatus::Doing,
        KanbanColumn::Done => info.status == TaskStatus::Done,
        KanbanColumn::Overdue => info.status == TaskStatus::Todo && info.overdue,
    }
}

pub fn column_tasks<'a>(infos: &'a [TaskInfo], col: KanbanColumn) -> Vec<&'a TaskInfo> {
    let mut tasks: Vec<&TaskInfo> = infos.iter().filter(|i| in_column(i, col)).collect();
    if col == KanbanColumn::Overdue {
        tasks.sort_by(|a, b| a.due.cmp(&b.due).then(a.id.cmp(&b.id)));
    }
    tasks
}

/// 四象限只显示未完成任务，按截止时间升序、无截止的排后。
pub fn quadrant_tasks<'a>(infos: &'a [TaskInfo], q: Quadrant) -> Vec<&'a TaskInfo> {
    let mut tasks: Vec<&TaskInfo> = infos
        .iter()
        .filter(|i| !i.is_done() && i.quadrant == q)
        .collect();
    tasks.sort_by(|a, b| {
        let order = match (a.due, b.due) {
            (Some(x), Some(y)) => x.cmp(&y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        };
        order.then(a.id.cmp(&b.id))
    });
    tasks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Quadrant, TodoItem};

    fn task(id: u64, text: &str, day: &str) -> TodoItem {
        TodoItem::new(id, text.to_owned(), day.to_owned())
    }

    fn at(h: u32, m: u32) -> NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 10, 20)
            .unwrap()
            .and_hms_opt(h, m, 0)
            .unwrap()
    }

    fn state(todos: Vec<TodoItem>) -> Settings {
        let mut st = Settings::default();
        st.todos = todos;
        st
    }

    #[test]
    fn today_scope_uses_due_or_creation_day() {
        let mut due_today = task(1, "截止今天", "2026-10-19");
        due_today.due = Some("2026-10-20 18:00".into());
        let created_today = task(2, "今天创建", "2026-10-20");
        let far = task(3, "别处", "2026-10-10");
        let infos = task_infos_at(&state(vec![due_today, created_today, far]), at(10, 0));

        assert!(infos[0].today);
        assert!(infos[1].today);
        assert!(!infos[2].today);
        let today = list_rows(&infos, ListScope::Today, ListFilter::Active);
        assert_eq!(today.rows, vec![0, 1]);
        assert_eq!(today.stats.scope_total, 2);
    }

    #[test]
    fn task_info_exposes_subtask_counts() {
        let mut t = task(1, "带子项", "2026-10-20");
        t.subtasks = vec![
            Subtask {
                text: "A".into(),
                done: true,
            },
            Subtask::new("B".into()),
        ];
        let infos = task_infos_at(&state(vec![t]), at(10, 0));
        assert_eq!(infos[0].subtask_total(), 2);
        assert_eq!(infos[0].subtask_done(), 1);
    }

    #[test]
    fn active_filter_hides_done_rows_but_keeps_counts() {
        let mut done = task(1, "完成", "2026-10-20");
        done.status = TaskStatus::Done;
        let open = task(2, "未完成", "2026-10-20");
        let infos = task_infos_at(&state(vec![done, open]), at(10, 0));

        let active = list_rows(&infos, ListScope::All, ListFilter::Active);
        assert_eq!(active.rows, vec![1]);
        assert_eq!(active.stats.scope_total, 2);
        assert_eq!(active.stats.open, 1);

        let all = list_rows(&infos, ListScope::All, ListFilter::All);
        assert_eq!(all.rows.len(), 2);
    }

    #[test]
    fn kanban_columns_separate_overdue_from_todo() {
        let mut overdue = task(1, "逾期", "2026-10-19");
        overdue.due = Some("2026-10-19 09:00".into());
        let todo = task(2, "待办", "2026-10-20");
        let infos = task_infos_at(&state(vec![overdue, todo]), at(10, 0));

        assert_eq!(column_tasks(&infos, KanbanColumn::Overdue).len(), 1);
        assert_eq!(column_tasks(&infos, KanbanColumn::Todo).len(), 1);
        assert!(in_column(&infos[0], KanbanColumn::Overdue));
        assert!(!in_column(&infos[0], KanbanColumn::Todo));
    }

    #[test]
    fn quadrant_tasks_exclude_done_and_sort_by_due() {
        let mut late = task(1, "晚", "2026-10-20");
        late.due = Some("2026-10-20 18:00".into());
        let mut early = task(2, "早", "2026-10-20");
        early.due = Some("2026-10-20 12:00".into());
        let mut done = task(3, "完成", "2026-10-20");
        done.status = TaskStatus::Done;
        done.due = Some("2026-10-20 11:00".into());
        let infos = task_infos_at(&state(vec![late, early, done]), at(10, 0));

        let urgent = quadrant_tasks(&infos, Quadrant::Urgent);
        assert_eq!(urgent.iter().map(|t| t.id).collect::<Vec<_>>(), vec![2, 1]);
    }

    #[test]
    fn instant_summary_uses_current_tasks() {
        let mut done = task(1, "已完成", "2026-10-20");
        done.status = TaskStatus::Done;
        let mut doing = task(2, "进行中的任务", "2026-10-20");
        doing.status = TaskStatus::Doing;
        doing.due = Some("2026-10-20 18:00".into());
        let mut overdue = task(3, "逾期任务", "2026-10-20");
        overdue.due = Some("2026-10-20 09:00".into());
        let mut important = task(4, "重要任务", "2026-10-20");
        important.important = true;
        important.due = Some("2026-10-20 20:00".into());
        let infos = task_infos_at(&state(vec![done, doing, overdue, important]), at(10, 0));

        let text = instant_summary(&infos, at(10, 0));
        assert!(text.contains("今天共 4 项任务，已完成 1 项（25%），还剩 3 项。"));
        assert!(text.contains("进行中：进行中的任务。"));
        assert!(text.contains("已逾期 1 项：逾期任务。"));
        assert!(text.contains("重要任务待推进：重要任务。"));
        assert!(text.contains("最近截止：进行中的任务 · 今天 18:00。"));
    }

    #[test]
    fn instant_summary_handles_empty_day() {
        let text = instant_summary(&[], at(10, 0));
        assert_eq!(text, "今天没有安排任务，手头也没有未完成的任务。");
    }

    #[test]
    fn nav_and_today_progress_share_the_same_today_rule() {
        let mut today_done = task(1, "今天完成", "2026-10-20");
        today_done.status = TaskStatus::Done;
        let mut today_open = task(2, "今天待办", "2026-10-20");
        today_open.due = Some("2026-10-20 18:00".into());
        let old = task(3, "旧任务", "2026-10-10");
        let infos = task_infos_at(&state(vec![today_done, today_open, old]), at(10, 0));

        let nav = nav_stats(&infos);
        assert_eq!(nav.today_total, 2);
        assert_eq!(nav.today_done, 1);
        assert_eq!(nav.open, 2);
        let progress = today_progress(&infos);
        assert_eq!(progress.total, 2);
        assert_eq!(progress.percent(), 50);
    }
}
