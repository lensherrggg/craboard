//! 无界面的应用核心：任务/便签/设置的所有写操作、查询和落盘都在这里。
//!
//! GUI 只负责画界面和收集交互，不再直接改 `Settings` 或 `TaskRepository`；
//! CLI、MCP、将来的 TUI/Web 共用同一套 `Core` 命令与 `view` 派生规则。

pub mod edit;
pub mod view;

pub use edit::{TaskEditDraft, clean_subtasks, validate_dep};
pub use view::{
    KanbanColumn, ListFilter, ListRows, ListScope, ListStats, NavStats, TaskInfo, TodayProgress,
    column_tasks, due_label, in_column, instant_summary, list_rows, nav_stats, quadrant_tasks,
    task_infos, task_infos_at, today_progress,
};

use crate::state::{
    self, AiSummary, CloseBehavior, FontSize, Quadrant, Repeat, Settings, Subtask, SummarySource,
    TaskStatus, ThemeKind, TodoItem, View, format_due, normalize_due, parse_due,
};
use crate::storage::{self, StoreError, TaskRepository};
use chrono::{Duration, NaiveDateTime};
use serde::Serialize;
use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct CoreError(String);

impl CoreError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CoreError {}

impl From<StoreError> for CoreError {
    fn from(e: StoreError) -> Self {
        Self(e.to_string())
    }
}

impl From<std::io::Error> for CoreError {
    fn from(e: std::io::Error) -> Self {
        Self(e.to_string())
    }
}

impl From<String> for CoreError {
    fn from(e: String) -> Self {
        Self(e)
    }
}

impl From<&str> for CoreError {
    fn from(e: &str) -> Self {
        Self(e.to_owned())
    }
}

pub type Result<T> = std::result::Result<T, CoreError>;

fn now() -> NaiveDateTime {
    chrono::Local::now().naive_local()
}

pub fn parse_status(input: &str) -> Option<TaskStatus> {
    match input.trim().to_ascii_lowercase().as_str() {
        "todo" | "open" | "pending" | "待办" => Some(TaskStatus::Todo),
        "doing" | "start" | "started" | "进行中" => Some(TaskStatus::Doing),
        "done" | "complete" | "completed" | "已完成" => Some(TaskStatus::Done),
        _ => None,
    }
}

pub fn parse_repeat(input: &str) -> Result<Option<Repeat>> {
    let s = input.trim();
    if s.is_empty()
        || matches!(
            s.to_ascii_lowercase().as_str(),
            "none" | "off" | "no" | "false" | "不重复" | "无"
        )
    {
        return Ok(None);
    }
    Repeat::parse(s).map(Some).ok_or_else(|| {
        CoreError::new(format!(
            "无法识别重复规则「{s}」，可用 daily、2w、weekly、monthly 等写法"
        ))
    })
}

/// 支持 `1d`、`2h`、`30m`、`1w`，纯数字按天
pub fn parse_shift(input: &str) -> Result<Duration> {
    let s = input.trim().to_ascii_lowercase();
    if s.is_empty() {
        return Err(CoreError::new("顺延时长不能为空"));
    }
    let split = s
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit())
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let n: i64 = num
        .parse()
        .map_err(|_| CoreError::new(format!("无法识别顺延时长「{input}」")))?;
    if n <= 0 {
        return Err(CoreError::new("顺延时长必须大于 0"));
    }
    let unit = unit.trim();
    let seconds = match unit {
        "" | "d" | "day" | "days" | "天" | "日" => 86_400,
        "h" | "hour" | "hours" | "小时" | "时" => 3_600,
        "m" | "min" | "mins" | "minute" | "minutes" | "分钟" | "分" => 60,
        "w" | "week" | "weeks" | "周" => 604_800,
        _ => return Err(CoreError::new(format!("无法识别顺延单位「{unit}」"))),
    };
    let secs = n
        .checked_mul(seconds)
        .filter(|v| *v <= 3650 * 86_400)
        .ok_or_else(|| CoreError::new("顺延时长过大"))?;
    Ok(Duration::seconds(secs))
}

pub fn parse_due_input(input: &str) -> Result<Option<String>> {
    normalize_due(input).map_err(|_| {
        CoreError::new(format!(
            "无法识别截止时间「{input}」，可用 2026-10-21 或 2026-10-21 18:00"
        ))
    })
}

#[derive(Serialize, Clone, Debug)]
pub struct TaskView {
    pub id: u64,
    pub text: String,
    pub status: &'static str,
    pub status_label: &'static str,
    pub due: Option<String>,
    pub important: bool,
    pub repeat: Option<String>,
    pub repeat_unit: Option<&'static str>,
    pub repeat_interval: Option<u32>,
    pub repeat_anchor: Option<String>,
    pub blocked_by: Vec<u64>,
    pub blocked_left: usize,
    pub subtasks: Vec<Subtask>,
    pub subtask_done: usize,
    pub subtask_total: usize,
    pub overdue: bool,
    pub urgent: bool,
    pub quadrant: &'static str,
    pub quadrant_label: &'static str,
    pub day: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub completed_at: Option<String>,
}

fn status_code(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Todo => "todo",
        TaskStatus::Doing => "doing",
        TaskStatus::Done => "done",
    }
}

fn repeat_unit_code(unit: state::RepeatUnit) -> &'static str {
    match unit {
        state::RepeatUnit::Day => "day",
        state::RepeatUnit::Week => "week",
        state::RepeatUnit::Month => "month",
        state::RepeatUnit::Year => "year",
    }
}

fn quadrant_code(q: Quadrant) -> &'static str {
    match q {
        Quadrant::ImportantUrgent => "important_urgent",
        Quadrant::Important => "important_not_urgent",
        Quadrant::Urgent => "urgent_not_important",
        Quadrant::Normal => "neither",
    }
}

impl TaskView {
    pub fn line(&self) -> String {
        let mut s = format!("#{} [{}] {}", self.id, self.status_label, self.text);
        if self.important {
            s.push_str(" · 重要");
        }
        if let Some(due) = &self.due {
            s.push_str(&format!(" · 截止 {due}"));
        }
        if self.overdue {
            s.push_str(" · 已逾期");
        }
        if let Some(repeat) = &self.repeat {
            s.push_str(&format!(" · 重复 {repeat}"));
        }
        if self.subtask_total > 0 {
            s.push_str(&format!(
                " · 子项 {}/{}",
                self.subtask_done, self.subtask_total
            ));
        }
        s
    }
}

/// 悬浮页和 MCP 共用的总结状态。`available` 只表示有没有收到过总结，
/// 内容是否过期看 `stale`，避免旧总结突然从界面上消失。
/// 定时总结关闭后 `next_due_at` 为空且不再判过期，但最近内容仍保留。
#[derive(Serialize, Clone, Debug)]
pub struct AiSummaryView {
    pub enabled: bool,
    pub available: bool,
    pub source: Option<SummarySource>,
    pub daily_enabled: bool,
    pub daily_times: Vec<String>,
    pub interval_enabled: bool,
    pub interval_hours: u32,
    pub text: Option<String>,
    pub generated_at: Option<String>,
    pub age_minutes: Option<i64>,
    pub stale: bool,
    pub next_due_at: Option<String>,
    pub window_start: Option<String>,
    pub window_end: Option<String>,
    /// 所有已到点但还没被新总结覆盖的触发窗口（按触发时间升序）
    pub pending_windows: Vec<SummaryWindowView>,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SummaryMode {
    Daily,
    Interval,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct SummaryWindowView {
    pub mode: SummaryMode,
    pub due_at: String,
    pub window_start: String,
    pub window_end: String,
}

#[derive(Default)]
struct SummarySchedule {
    stale: bool,
    next_due: Option<NaiveDateTime>,
    window: Option<(NaiveDateTime, NaiveDateTime)>,
    pending: Vec<SummaryWindowView>,
}

/// 两种定时模式可同时开启。每日定点每次覆盖当天 0 点到触发时刻；
/// 固定间隔以上一次任意来源的总结为锚点，窗口从锚点到触发时刻。
fn summary_schedule(
    st: &Settings,
    generated: Option<NaiveDateTime>,
    now: NaiveDateTime,
) -> SummarySchedule {
    let mut schedule = SummarySchedule::default();
    if !st.ai_summary_enabled {
        return schedule;
    }

    // (触发时刻, 窗口起点, 窗口终点, 模式)
    let mut pending: Vec<(NaiveDateTime, NaiveDateTime, NaiveDateTime, SummaryMode)> = Vec::new();
    let mut upcoming: Vec<(NaiveDateTime, NaiveDateTime, NaiveDateTime, SummaryMode)> = Vec::new();
    let today = now.date();

    if st.ai_summary_daily_enabled {
        let times: Vec<chrono::NaiveTime> = st
            .ai_summary_daily_times
            .iter()
            .filter_map(|s| state::parse_clock(s))
            .collect();
        if !times.is_empty() {
            let today_times: Vec<NaiveDateTime> =
                times.iter().map(|t| today.and_time(*t)).collect();
            let last_passed = today_times
                .iter()
                .copied()
                .filter(|d| *d <= now)
                .max()
                .or_else(|| {
                    let yesterday = today - Duration::days(1);
                    times.iter().map(|t| yesterday.and_time(*t)).max()
                });
            let next = today_times
                .iter()
                .copied()
                .filter(|d| *d > now)
                .min()
                .or_else(|| {
                    let tomorrow = today + Duration::days(1);
                    times.iter().map(|t| tomorrow.and_time(*t)).min()
                });
            if let Some(due) = last_passed {
                if generated.map_or(true, |g| g < due) {
                    let start = due.date().and_hms_opt(0, 0, 0).unwrap();
                    pending.push((due, start, due, SummaryMode::Daily));
                }
            }
            if let Some(due) = next {
                let start = due.date().and_hms_opt(0, 0, 0).unwrap();
                upcoming.push((due, start, due, SummaryMode::Daily));
            }
        }
    }

    if st.ai_summary_interval_enabled {
        if let Some(g) = generated {
            let due = g + Duration::hours(st.ai_summary_interval_hours as i64);
            if now >= due {
                pending.push((due, g, due, SummaryMode::Interval));
            } else {
                upcoming.push((due, g, due, SummaryMode::Interval));
            }
        }
    }

    if !pending.is_empty() {
        pending.sort_by_key(|p| p.0);
        let due = pending.iter().map(|p| p.0).max().unwrap();
        let start = pending.iter().map(|p| p.1).min().unwrap();
        schedule.stale = true;
        schedule.next_due = Some(due);
        schedule.window = Some((start, due));
        schedule.pending = pending
            .into_iter()
            .map(|(due, start, end, mode)| SummaryWindowView {
                mode,
                due_at: format_due(due),
                window_start: format_due(start),
                window_end: format_due(end),
            })
            .collect();
    } else if let Some((due, start, end, _)) = upcoming.into_iter().min_by_key(|p| p.0) {
        schedule.next_due = Some(due);
        schedule.window = Some((start, end));
    }

    schedule
}

pub fn ai_summary_view(st: &Settings, now: NaiveDateTime) -> AiSummaryView {
    let enabled = st.ai_summary_enabled;
    let interval = st.ai_summary_interval_hours;
    let generated = st
        .ai_summary
        .as_ref()
        .and_then(|s| parse_due(&s.generated_at));
    let schedule = summary_schedule(st, generated, now);
    AiSummaryView {
        enabled,
        available: st.ai_summary.is_some(),
        source: st.ai_summary.as_ref().map(|s| s.source),
        daily_enabled: st.ai_summary_daily_enabled,
        daily_times: st.ai_summary_daily_times.clone(),
        interval_enabled: st.ai_summary_interval_enabled,
        interval_hours: interval,
        text: st.ai_summary.as_ref().map(|s| s.text.clone()),
        generated_at: st.ai_summary.as_ref().map(|s| s.generated_at.clone()),
        age_minutes: generated.map(|g| (now - g).num_minutes().max(0)),
        stale: schedule.stale,
        next_due_at: schedule.next_due.map(format_due),
        window_start: schedule.window.map(|w| format_due(w.0)),
        window_end: schedule.window.map(|w| format_due(w.1)),
        pending_windows: schedule.pending,
    }
}

fn task_view(t: &TodoItem, all: &[TodoItem], now: NaiveDateTime, hours: u32) -> TaskView {
    let statuses: std::collections::HashMap<u64, TaskStatus> =
        all.iter().map(|x| (x.id, x.status)).collect();
    let subtask_total = t.subtasks.len();
    let subtask_done = t.subtask_done();
    let urgent = t.is_urgent(now, hours);
    let quadrant = t.quadrant(now, hours);
    TaskView {
        id: t.id,
        text: t.text.clone(),
        status: status_code(t.status),
        status_label: t.status.label(),
        due: t.due.clone(),
        important: t.important,
        repeat: t.repeat.map(|r| r.label()),
        repeat_unit: t.repeat.map(|r| repeat_unit_code(r.unit)),
        repeat_interval: t.repeat.map(|r| r.interval),
        repeat_anchor: t.repeat_anchor.clone(),
        blocked_by: t.blocked_by.clone(),
        blocked_left: t
            .blocked_by
            .iter()
            .filter(|id| statuses.get(id).is_some_and(|s| *s != TaskStatus::Done))
            .count(),
        subtasks: t.subtasks.clone(),
        subtask_done,
        subtask_total,
        overdue: t.past_due(now),
        urgent,
        quadrant: quadrant_code(quadrant),
        quadrant_label: quadrant.label(),
        day: t.day.clone(),
        created_at: t.created_at.clone(),
        updated_at: t.updated_at.clone(),
        completed_at: t.completed_at.clone(),
    }
}

#[derive(Clone, Debug, Default)]
pub struct ListOptions {
    pub include_done: bool,
    pub important: Option<bool>,
    pub overdue: bool,
    /// 只看未来这么多小时内到期（含逾期）
    pub within_hours: Option<u32>,
    pub text_contains: Option<String>,
}

pub struct Core {
    settings: Settings,
    repo: TaskRepository,
    tasks_dirty: bool,
    settings_dirty: bool,
}

impl Core {
    /// GUI 用：打不开 SQLite 也会退回 JSON/内存，不中断启动。
    pub fn open_local() -> (Self, Option<String>) {
        let mut settings = state::load();
        let (repo, loaded, warning) = storage::open_default(&settings.todos);
        settings.todos = loaded;
        // 旧库存的是独立子任务行；并入子项清单后要回写一次，迁移才算落盘
        let tasks_dirty = settings.todos.iter().any(TodoItem::has_legacy_parent);
        settings.sanitize();
        (
            Self {
                settings,
                repo,
                tasks_dirty,
                settings_dirty: false,
            },
            warning,
        )
    }

    /// CLI/MCP 用：保持「每次操作重新打开存储」的短生命周期用法。
    pub fn open() -> Result<Self> {
        let (core, warning) = Self::open_local();
        if let Some(w) = warning {
            eprintln!("craboard: {w}");
        }
        Ok(core)
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn tasks(&self) -> &[TodoItem] {
        &self.settings.todos
    }

    pub fn task(&self, id: u64) -> Result<&TodoItem> {
        self.settings
            .task(id)
            .ok_or_else(|| CoreError::new(format!("任务 #{id} 不存在")))
    }

    pub fn data_dir(&self) -> PathBuf {
        state::data_dir()
    }

    pub fn watch_paths(&self) -> Vec<PathBuf> {
        self.repo.watch_paths()
    }

    pub fn is_dirty(&self) -> bool {
        self.tasks_dirty || self.settings_dirty
    }

    /// 启动时把迁移/规范化后的设置落盘（包括旧存档的字段迁移）。
    pub fn touch_settings(&mut self) {
        self.settings_dirty = true;
    }

    pub fn view(&self, id: u64) -> Result<TaskView> {
        let t = self.task(id)?;
        Ok(task_view(
            t,
            &self.settings.todos,
            now(),
            self.settings.urgent_within_hours,
        ))
    }

    pub fn list(&self, opts: &ListOptions) -> Vec<TaskView> {
        let now = now();
        let hours = self.settings.urgent_within_hours;
        let horizon = opts
            .within_hours
            .map(|h| now + Duration::hours(h.min(24 * 365) as i64));
        let query = opts
            .text_contains
            .as_ref()
            .map(|q| q.trim().to_lowercase())
            .filter(|q| !q.is_empty());
        let mut out: Vec<TaskView> = self
            .settings
            .todos
            .iter()
            .filter(|t| opts.include_done || t.status != TaskStatus::Done)
            .filter(|t| opts.important.is_none_or(|v| t.important == v))
            .filter(|t| !opts.overdue || t.past_due(now))
            .filter(|t| horizon.is_none_or(|h| t.due_at().is_some_and(|d| d <= h)))
            .filter(|t| {
                query
                    .as_ref()
                    .is_none_or(|q| t.text.to_lowercase().contains(q))
            })
            .map(|t| task_view(t, &self.settings.todos, now, hours))
            .collect();
        out.sort_by(|a, b| {
            let ad = a.due.as_deref().and_then(parse_due);
            let bd = b.due.as_deref().and_then(parse_due);
            (a.status == "done")
                .cmp(&(b.status == "done"))
                .then(ad.is_none().cmp(&bd.is_none()))
                .then(ad.cmp(&bd))
                .then(a.id.cmp(&b.id))
        });
        out
    }

    pub fn add(&mut self, input: NewTask) -> Result<TaskView> {
        let text = input.text.trim();
        if text.is_empty() {
            return Err(CoreError::new("任务内容不能为空"));
        }
        let due = parse_due_input(input.due.as_deref().unwrap_or(""))?;
        if input.repeat.is_some() && due.is_none() {
            return Err(CoreError::new("周期任务必须有截止时间"));
        }
        for dep in &input.blocked_by {
            validate_dep(&self.settings, 0, *dep)?;
        }
        let id = self.next_task_id();
        let now = now();
        let mut t = TodoItem::new(id, text.to_owned(), now.format("%Y-%m-%d").to_string());
        t.created_at = Some(format_due(now));
        t.updated_at = t.created_at.clone();
        t.due = due.clone();
        t.important = input.important;
        t.repeat = input.repeat;
        t.repeat_anchor = if input.repeat.is_some() { due } else { None };
        t.subtasks = clean_subtasks(&input.subtasks);
        t.blocked_by = input.blocked_by;
        t.blocked_by.sort_unstable();
        t.blocked_by.dedup();
        self.settings.todos.push(t);
        self.tasks_dirty = true;
        self.view(id)
    }

    /// 输入框/看板快速添加：今天的普通任务。
    pub fn add_text(&mut self, text: &str) -> Result<TaskView> {
        self.add(NewTask {
            text: text.to_owned(),
            ..NewTask::default()
        })
    }

    /// 完整新建面板的提交：先校验，再整体写入；失败时不留下半个任务。
    pub fn create_task_draft(
        &mut self,
        draft: &TaskEditDraft,
        now: NaiveDateTime,
    ) -> std::result::Result<TaskView, String> {
        let text = draft.text.trim();
        if text.is_empty() {
            return Err("任务内容不能为空".to_owned());
        }
        let due = normalize_due(&draft.due)
            .map_err(|_| "截止时间格式不对，示例：2026-10-21 或 2026-10-21 18:00".to_owned())?;
        if draft.repeat.is_some() && due.is_none() {
            return Err("周期任务需要设置截止时间".to_owned());
        }
        for dep in &draft.blocked_by {
            validate_dep(&self.settings, 0, *dep)?;
        }
        let id = self.next_task_id();
        let mut next = self.settings.clone();
        let mut t = TodoItem::new(id, text.to_owned(), now.format("%Y-%m-%d").to_string());
        t.created_at = Some(format_due(now));
        t.updated_at = t.created_at.clone();
        t.due = due.clone();
        t.important = draft.important;
        t.repeat = draft.repeat;
        t.repeat_anchor = if draft.repeat.is_some() { due } else { None };
        t.subtasks = clean_subtasks(&draft.subtasks);
        t.blocked_by = draft.blocked_by.clone();
        t.blocked_by.sort_unstable();
        t.blocked_by.dedup();
        next.todos.push(t);
        if draft.status != TaskStatus::Todo {
            next.set_status(id, draft.status, now)?;
        }
        self.settings = next;
        self.tasks_dirty = true;
        self.view(id).map_err(|e| e.to_string())
    }

    /// 子项只是任务内的清单：勾选全部子项不会自动完成主任务。
    pub fn set_subtask_done(&mut self, id: u64, index: usize, done: bool) -> Result<TaskView> {
        let now = now();
        let t = self
            .settings
            .todos
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or_else(|| CoreError::new(format!("任务 #{id} 不存在")))?;
        let Some(subtask) = t.subtasks.get_mut(index) else {
            return Err(CoreError::new(format!(
                "任务 #{id} 没有第 {} 个子项",
                index + 1
            )));
        };
        if subtask.done != done {
            subtask.done = done;
            t.updated_at = Some(format_due(now));
            self.tasks_dirty = true;
        }
        self.view(id)
    }

    fn next_task_id(&mut self) -> u64 {
        let before = self.settings.next_id;
        let id = match self.repo.allocate_id() {
            Ok(id) => {
                if id >= self.settings.next_id {
                    self.settings.next_id = id + 1;
                }
                id
            }
            Err(_) => self.settings.take_id(),
        };
        if self.settings.next_id != before {
            self.settings_dirty = true;
        }
        id
    }

    pub fn update(&mut self, id: u64, patch: TaskPatch) -> Result<UpdateOutcome> {
        let current = self.task(id)?;
        let current_repeat = current.repeat;
        let current_due = current.due.clone();
        let current_status = current.status;

        let text = match patch.text {
            Some(text) => {
                let text = text.trim().to_owned();
                if text.is_empty() {
                    return Err(CoreError::new("任务内容不能为空"));
                }
                text
            }
            None => current.text.clone(),
        };
        let due_patch = patch.due.clone();
        let repeat_patch = patch.repeat;
        let new_due = match due_patch.clone() {
            Some(inner) => parse_due_input(inner.as_deref().unwrap_or(""))?,
            None => current_due.clone(),
        };
        let new_repeat = repeat_patch.unwrap_or(current_repeat);
        if new_repeat.is_some() && new_due.is_none() {
            return Err(CoreError::new(
                "周期任务必须有截止时间；如果要清空截止时间，请先取消重复",
            ));
        }
        if let Some(deps) = &patch.blocked_by {
            for dep in deps {
                validate_dep(&self.settings, id, *dep)?;
            }
        }

        let due_changed = due_patch.is_some() && new_due != current_due;
        let repeat_changed = repeat_patch.is_some() && new_repeat != current_repeat;
        let now = now();
        let mut next = self.settings.clone();
        let t = next
            .todos
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or_else(|| CoreError::new(format!("任务 #{id} 不存在")))?;
        t.text = text;
        if due_changed {
            t.due = new_due;
            t.reminded_due = None;
        }
        if repeat_changed {
            t.repeat = new_repeat;
            t.repeat_anchor = if new_repeat.is_some() {
                t.due.clone()
            } else {
                None
            };
        } else if due_changed && t.repeat.is_some() {
            // 手动改截止时间等于改周期计划，基准跟着走
            t.repeat_anchor = t.due.clone();
        }
        if let Some(important) = patch.important {
            t.important = important;
        }
        if let Some(subtasks) = patch.subtasks {
            t.subtasks = clean_subtasks(&subtasks);
        }
        if let Some(deps) = patch.blocked_by {
            t.blocked_by = deps;
            t.blocked_by.sort_unstable();
            t.blocked_by.dedup();
        }
        t.updated_at = Some(format_due(now));
        let next_due = if let Some(status) = patch.status {
            if status != current_status {
                next.set_status(id, status, now)?
            } else {
                None
            }
        } else {
            None
        };
        self.settings = next;
        self.tasks_dirty = true;
        Ok(UpdateOutcome {
            task: self.view(id)?,
            next_due: next_due.map(format_due),
        })
    }

    /// 返回下一轮截止时间：周期任务完成本轮时是 Some，普通任务是 None
    pub fn set_status(&mut self, id: u64, status: TaskStatus) -> Result<Option<NaiveDateTime>> {
        let next = self.settings.set_status(id, status, now())?;
        self.tasks_dirty = true;
        Ok(next)
    }

    pub fn postpone(&mut self, id: u64, by: Duration) -> Result<NaiveDateTime> {
        let next = self.settings.postpone(id, by, now())?;
        self.tasks_dirty = true;
        Ok(next)
    }

    pub fn postpone_by(&mut self, id: u64, by: Duration) -> Result<TaskView> {
        self.postpone(id, by)?;
        self.view(id)
    }

    pub fn postpone_to(&mut self, id: u64, due: NaiveDateTime) -> Result<TaskView> {
        self.settings.postpone_to(id, due, now())?;
        self.tasks_dirty = true;
        self.view(id)
    }

    pub fn remove(&mut self, id: u64) -> Result<TaskView> {
        let view = self.view(id)?;
        self.settings.remove_task(id);
        self.tasks_dirty = true;
        Ok(view)
    }

    /// 编辑弹窗保存；校验失败时返回给界面展示，状态保持原样。
    pub fn save_task_draft(
        &mut self,
        draft: &TaskEditDraft,
        now: NaiveDateTime,
    ) -> std::result::Result<Option<NaiveDateTime>, String> {
        let out = draft.save(&mut self.settings, now)?;
        self.tasks_dirty = true;
        Ok(out)
    }

    pub fn take_due_reminders(
        &mut self,
        last: NaiveDateTime,
        now: NaiveDateTime,
    ) -> Vec<(u64, String, NaiveDateTime)> {
        let hits = self.settings.take_due_reminders(last, now);
        if !hits.is_empty() {
            self.tasks_dirty = true;
        }
        hits
    }

    pub fn add_note(&mut self) -> u64 {
        let id = self.settings.add_note();
        self.settings_dirty = true;
        id
    }

    pub fn update_note(&mut self, id: u64, text: &str) {
        if let Some(note) = self.settings.notes.iter_mut().find(|n| n.id == id) {
            if note.text != text {
                note.text = text.to_owned();
                self.settings_dirty = true;
            }
        }
    }

    pub fn remove_note(&mut self, id: u64) {
        self.settings.remove_note(id);
        self.settings_dirty = true;
    }

    pub fn set_view(&mut self, view: View) {
        if self.settings.view != view {
            self.settings.view = view;
            self.settings_dirty = true;
        }
    }

    pub fn set_notes_open(&mut self, open: bool) {
        if self.settings.notes_open != open {
            self.settings.notes_open = open;
            self.settings_dirty = true;
        }
    }

    pub fn set_notes_width(&mut self, width: f32) {
        if (self.settings.notes_width - width).abs() > 0.5 {
            self.settings.notes_width = width;
            self.settings_dirty = true;
        }
    }

    pub fn set_window_pos(&mut self, pos: [f32; 2]) {
        if (self.settings.pos[0] - pos[0]).abs() > 0.5
            || (self.settings.pos[1] - pos[1]).abs() > 0.5
        {
            self.settings.pos = pos;
            self.settings_dirty = true;
        }
    }

    pub fn set_window_size(&mut self, size: [f32; 2]) {
        if (self.settings.size[0] - size[0]).abs() > 0.5
            || (self.settings.size[1] - size[1]).abs() > 0.5
        {
            self.settings.size = size;
            self.settings_dirty = true;
        }
    }

    /// 字号只属于 GUI 呈现，不进 SettingsView/Patch；这里只负责随设置持久化。
    pub fn set_font_size(&mut self, font_size: FontSize) {
        if self.settings.font_size != font_size {
            self.settings.font_size = font_size;
            self.settings_dirty = true;
        }
    }

    pub fn set_theme(&mut self, theme: ThemeKind) {
        if self.settings.theme != theme {
            self.settings.theme = theme;
            self.settings_dirty = true;
        }
    }

    /// 开机自启只属于 GUI 的 Windows 集成，不进 SettingsView/Patch；注册表写入由 GUI 负责。
    pub fn set_autostart(&mut self, autostart: bool) {
        if self.settings.autostart != autostart {
            self.settings.autostart = autostart;
            self.settings_dirty = true;
        }
    }

    /// 关闭窗口行为同样只作用于 GUI，不进 SettingsView/Patch。
    pub fn set_close_behavior(&mut self, behavior: CloseBehavior) {
        if self.settings.close_behavior != behavior {
            self.settings.close_behavior = behavior;
            self.settings_dirty = true;
        }
    }

    pub fn toggle_task_collapsed(&mut self, id: u64) {
        if let Some(pos) = self.settings.collapsed_tasks.iter().position(|x| *x == id) {
            self.settings.collapsed_tasks.remove(pos);
        } else {
            self.settings.collapsed_tasks.push(id);
        }
        self.settings_dirty = true;
    }

    pub fn set_kanban_collapsed(&mut self, index: usize, collapsed: bool) {
        if let Some(slot) = self.settings.kanban_collapsed.get_mut(index) {
            if *slot != collapsed {
                *slot = collapsed;
                self.settings_dirty = true;
            }
        }
    }

    pub fn settings_view(&self) -> SettingsView {
        SettingsView {
            show_seconds: self.settings.show_seconds,
            reminders_enabled: self.settings.reminders_enabled,
            reminder_seconds: self.settings.reminder_seconds,
            wip_limit: self.settings.wip_limit,
            urgent_within_hours: self.settings.urgent_within_hours,
            ai_summary_enabled: self.settings.ai_summary_enabled,
            ai_summary_daily_enabled: self.settings.ai_summary_daily_enabled,
            ai_summary_daily_times: self.settings.ai_summary_daily_times.clone(),
            ai_summary_interval_enabled: self.settings.ai_summary_interval_enabled,
            ai_summary_interval_hours: self.settings.ai_summary_interval_hours,
            ai_summary_available: self.settings.ai_summary.is_some(),
            data_dir: self.data_dir().display().to_string(),
        }
    }

    pub fn ai_summary_view(&self) -> AiSummaryView {
        ai_summary_view(&self.settings, now())
    }

    /// `generated_at` 省略时用当前时间；时间为空或格式不对直接报错，
    /// 免得存档里出现永远解析不了的总结。
    pub fn set_ai_summary(
        &mut self,
        text: &str,
        generated_at: Option<&str>,
    ) -> Result<AiSummaryView> {
        let text = text.trim();
        if text.is_empty() {
            return Err(CoreError::new("AI 总结内容不能为空"));
        }
        if text.chars().count() > 4000 {
            return Err(CoreError::new("AI 总结最多 4000 字"));
        }
        let generated_at = match generated_at.map(str::trim).filter(|s| !s.is_empty()) {
            Some(raw) => normalize_due(raw)
                .map_err(|_| {
                    CoreError::new(format!("无法识别总结时间「{raw}」，可用 2026-10-20 19:30"))
                })?
                .ok_or_else(|| {
                    CoreError::new(format!("无法识别总结时间「{raw}」，可用 2026-10-20 19:30"))
                })?,
            None => self.now_string(),
        };
        self.settings.ai_summary = Some(AiSummary {
            text: text.to_owned(),
            generated_at,
            source: SummarySource::Ai,
        });
        self.settings_dirty = true;
        Ok(self.ai_summary_view())
    }

    /// 用当前任务数据现场生成总结并覆盖已有正文；定时总结的开关和间隔不受影响。
    pub fn refresh_instant_summary(&mut self) -> AiSummaryView {
        self.refresh_instant_summary_at(now())
    }

    fn refresh_instant_summary_at(&mut self, now: NaiveDateTime) -> AiSummaryView {
        let infos = task_infos_at(&self.settings, now);
        self.settings.ai_summary = Some(AiSummary {
            text: instant_summary(&infos, now),
            generated_at: format_due(now),
            source: SummarySource::Instant,
        });
        self.settings_dirty = true;
        ai_summary_view(&self.settings, now)
    }

    pub fn clear_ai_summary(&mut self) -> Result<AiSummaryView> {
        self.settings.ai_summary = None;
        self.settings_dirty = true;
        Ok(self.ai_summary_view())
    }

    pub fn update_settings(&mut self, patch: SettingsPatch) -> Result<SettingsView> {
        let st = &mut self.settings;
        if let Some(v) = patch.show_seconds {
            st.show_seconds = v;
        }
        if let Some(v) = patch.reminders_enabled {
            st.reminders_enabled = v;
        }
        if let Some(v) = patch.reminder_seconds {
            st.reminder_seconds = v;
        }
        if let Some(v) = patch.wip_limit {
            st.wip_limit = v;
        }
        if let Some(v) = patch.urgent_within_hours {
            st.urgent_within_hours = v;
        }
        if let Some(v) = patch.ai_summary_enabled {
            st.ai_summary_enabled = v;
        }
        if let Some(v) = patch.ai_summary_daily_enabled {
            st.ai_summary_daily_enabled = v;
        }
        if let Some(v) = patch.ai_summary_daily_times {
            st.ai_summary_daily_times = v;
        }
        if let Some(v) = patch.ai_summary_interval_enabled {
            st.ai_summary_interval_enabled = v;
        }
        if let Some(v) = patch.ai_summary_interval_hours {
            st.ai_summary_interval_hours = v;
        }
        st.sanitize();
        self.settings_dirty = true;
        Ok(self.settings_view())
    }

    pub fn save(&mut self) -> Result<()> {
        if self.tasks_dirty {
            self.repo.sync(&self.settings.todos)?;
            self.tasks_dirty = false;
        }
        if self.settings_dirty {
            state::save(&self.settings)?;
            self.settings_dirty = false;
        }
        Ok(())
    }

    /// 外部（CLI/MCP）改过设置或任务库后整体重读。调用方负责避开本地未保存改动。
    pub fn reload_all(&mut self) -> Result<()> {
        let mut next = state::load();
        next.todos = self.repo.reload()?;
        let tasks_dirty = next.todos.iter().any(TodoItem::has_legacy_parent);
        next.sanitize();
        self.settings = next;
        self.tasks_dirty = tasks_dirty;
        self.settings_dirty = false;
        Ok(())
    }

    pub fn now_string(&self) -> String {
        format_due(now())
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct UpdateOutcome {
    pub task: TaskView,
    pub next_due: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct NewTask {
    pub text: String,
    pub due: Option<String>,
    pub important: bool,
    pub repeat: Option<Repeat>,
    pub subtasks: Vec<Subtask>,
    pub blocked_by: Vec<u64>,
}

/// `None` 表示该字段不变；`Some(None)` 表示清空
#[derive(Clone, Debug, Default)]
pub struct TaskPatch {
    pub text: Option<String>,
    pub due: Option<Option<String>>,
    pub status: Option<TaskStatus>,
    pub important: Option<bool>,
    pub repeat: Option<Option<Repeat>>,
    pub subtasks: Option<Vec<Subtask>>,
    pub blocked_by: Option<Vec<u64>>,
}

#[derive(Serialize, Clone, Debug)]
pub struct SettingsView {
    pub show_seconds: bool,
    pub reminders_enabled: bool,
    pub reminder_seconds: u32,
    pub wip_limit: usize,
    pub urgent_within_hours: u32,
    pub ai_summary_enabled: bool,
    pub ai_summary_daily_enabled: bool,
    pub ai_summary_daily_times: Vec<String>,
    pub ai_summary_interval_enabled: bool,
    pub ai_summary_interval_hours: u32,
    pub ai_summary_available: bool,
    pub data_dir: String,
}

#[derive(Clone, Debug, Default)]
pub struct SettingsPatch {
    pub show_seconds: Option<bool>,
    pub reminders_enabled: Option<bool>,
    pub reminder_seconds: Option<u32>,
    pub wip_limit: Option<usize>,
    pub urgent_within_hours: Option<u32>,
    pub ai_summary_enabled: Option<bool>,
    pub ai_summary_daily_enabled: Option<bool>,
    pub ai_summary_daily_times: Option<Vec<String>>,
    pub ai_summary_interval_enabled: Option<bool>,
    pub ai_summary_interval_hours: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryTaskStore;

    fn task(id: u64, text: &str) -> TodoItem {
        TodoItem::new(id, text.to_owned(), "2026-10-20".to_owned())
    }

    fn core_with(todos: Vec<TodoItem>) -> Core {
        let mut settings = Settings::default();
        settings.todos = todos;
        settings.sanitize();
        let mut repo = TaskRepository::new(Box::new(MemoryTaskStore::default()));
        repo.load(&[]).unwrap();
        Core {
            settings,
            repo,
            tasks_dirty: false,
            settings_dirty: false,
        }
    }

    #[test]
    fn shift_units_parse() {
        assert_eq!(parse_shift("1d").unwrap(), Duration::days(1));
        assert_eq!(parse_shift("2h").unwrap(), Duration::hours(2));
        assert_eq!(parse_shift("30m").unwrap(), Duration::minutes(30));
        assert_eq!(parse_shift("1w").unwrap(), Duration::weeks(1));
        assert_eq!(parse_shift("3").unwrap(), Duration::days(3));
        assert!(parse_shift("0d").is_err());
        assert!(parse_shift("1y").is_err());
    }

    #[test]
    fn repeat_input_accepts_none_and_rules() {
        assert_eq!(parse_repeat("none").unwrap(), None);
        assert_eq!(
            parse_repeat("2w").unwrap(),
            Some(Repeat::new(state::RepeatUnit::Week, 2))
        );
        assert_eq!(
            parse_repeat("weekly").unwrap(),
            Some(Repeat::new(state::RepeatUnit::Week, 1))
        );
        assert!(parse_repeat("every moon").is_err());
    }

    #[test]
    fn ai_summary_view_tracks_availability_and_staleness() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 10, 20)
            .unwrap()
            .and_hms_opt(10, 0, 0)
            .unwrap();
        let mut st = Settings::default();
        st.ai_summary_interval_hours = 6;

        let view = ai_summary_view(&st, now);
        assert!(view.enabled);
        assert!(!view.available);
        assert!(!view.stale);
        assert!(view.next_due_at.is_none());

        st.ai_summary = Some(AiSummary {
            text: "上午完成了周报".into(),
            generated_at: "2026-10-20 09:00".into(),
            source: SummarySource::Ai,
        });
        let view = ai_summary_view(&st, now);
        assert!(view.available);
        assert_eq!(view.source, Some(SummarySource::Ai));
        assert!(!view.stale);
        assert_eq!(view.age_minutes, Some(60));
        assert_eq!(view.next_due_at.as_deref(), Some("2026-10-20 15:00"));

        let later = now + Duration::hours(7);
        assert!(ai_summary_view(&st, later).stale);

        st.ai_summary_enabled = false;
        let view = ai_summary_view(&st, later);
        assert!(!view.enabled);
        assert!(!view.stale);
        assert!(view.next_due_at.is_none());
        assert!(view.available);
    }

    #[test]
    fn daily_schedule_uses_midnight_to_trigger_window() {
        let at = |h, m| {
            chrono::NaiveDate::from_ymd_opt(2026, 10, 20)
                .unwrap()
                .and_hms_opt(h, m, 0)
                .unwrap()
        };
        let mut st = Settings::default();
        st.ai_summary_daily_enabled = true;
        st.ai_summary_daily_times = vec!["18:00".into(), "09:00".into()];
        st.ai_summary_interval_enabled = false;
        st.sanitize();

        // 没有总结时，已过的 09:00 触发点算待更新
        let view = ai_summary_view(&st, at(10, 0));
        assert!(view.stale);
        assert_eq!(view.next_due_at.as_deref(), Some("2026-10-20 09:00"));
        assert_eq!(view.window_start.as_deref(), Some("2026-10-20 00:00"));
        assert_eq!(view.window_end.as_deref(), Some("2026-10-20 09:00"));
        assert_eq!(view.pending_windows.len(), 1);
        assert_eq!(view.pending_windows[0].mode, SummaryMode::Daily);

        // 09:30 生成后到下一触发点前都算新鲜，下一窗口覆盖到 18:00
        st.ai_summary = Some(AiSummary {
            text: "上午".into(),
            generated_at: "2026-10-20 09:30".into(),
            source: SummarySource::Ai,
        });
        let view = ai_summary_view(&st, at(10, 0));
        assert!(!view.stale);
        assert!(view.pending_windows.is_empty());
        assert_eq!(view.next_due_at.as_deref(), Some("2026-10-20 18:00"));
        assert_eq!(view.window_start.as_deref(), Some("2026-10-20 00:00"));
        assert_eq!(view.window_end.as_deref(), Some("2026-10-20 18:00"));

        // 过了 18:00 未刷新则待更新
        let view = ai_summary_view(&st, at(19, 0));
        assert!(view.stale);
        assert_eq!(view.next_due_at.as_deref(), Some("2026-10-20 18:00"));
        assert_eq!(view.window_end.as_deref(), Some("2026-10-20 18:00"));

        // 18:30 生成后，下一次是明天 09:00，窗口从明天 00:00 起
        st.ai_summary = Some(AiSummary {
            text: "晚间".into(),
            generated_at: "2026-10-20 18:30".into(),
            source: SummarySource::Ai,
        });
        let view = ai_summary_view(&st, at(19, 0));
        assert!(!view.stale);
        assert_eq!(view.next_due_at.as_deref(), Some("2026-10-21 09:00"));
        assert_eq!(view.window_start.as_deref(), Some("2026-10-21 00:00"));
        assert_eq!(view.window_end.as_deref(), Some("2026-10-21 09:00"));
    }

    #[test]
    fn both_modes_pick_the_earliest_next_trigger() {
        let at = |h, m| {
            chrono::NaiveDate::from_ymd_opt(2026, 10, 20)
                .unwrap()
                .and_hms_opt(h, m, 0)
                .unwrap()
        };
        let mut st = Settings::default();
        st.ai_summary_daily_enabled = true;
        st.ai_summary_daily_times = vec!["18:00".into()];
        st.ai_summary_interval_enabled = true;
        st.ai_summary_interval_hours = 6;
        st.ai_summary = Some(AiSummary {
            text: "早晨".into(),
            generated_at: "2026-10-20 09:00".into(),
            source: SummarySource::Ai,
        });

        // 09:00 + 6h 早于每日 18:00
        let view = ai_summary_view(&st, at(10, 0));
        assert!(!view.stale);
        assert_eq!(view.next_due_at.as_deref(), Some("2026-10-20 15:00"));
        assert_eq!(view.window_start.as_deref(), Some("2026-10-20 09:00"));
        assert_eq!(view.window_end.as_deref(), Some("2026-10-20 15:00"));
        assert_eq!(view.pending_windows.len(), 0);

        // 间隔触发点过了仍未刷新：按间隔窗口待更新
        let view = ai_summary_view(&st, at(15, 30));
        assert!(view.stale);
        assert_eq!(view.next_due_at.as_deref(), Some("2026-10-20 15:00"));
        assert_eq!(view.window_start.as_deref(), Some("2026-10-20 09:00"));
        assert_eq!(view.pending_windows[0].mode, SummaryMode::Interval);

        // 15:05 的总结让间隔计时重置到 21:05，但每日 18:00 更早到
        st.ai_summary = Some(AiSummary {
            text: "午后".into(),
            generated_at: "2026-10-20 15:05".into(),
            source: SummarySource::Ai,
        });
        let view = ai_summary_view(&st, at(16, 0));
        assert!(!view.stale);
        assert_eq!(view.next_due_at.as_deref(), Some("2026-10-20 18:00"));
        assert_eq!(view.window_start.as_deref(), Some("2026-10-20 00:00"));
        assert_eq!(view.window_end.as_deref(), Some("2026-10-20 18:00"));
    }

    #[test]
    fn instant_summary_resets_interval_anchor() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 10, 20)
            .unwrap()
            .and_hms_opt(16, 0, 0)
            .unwrap();
        let mut core = core_with(Vec::new());
        core.settings.ai_summary_daily_enabled = false;
        core.settings.ai_summary_interval_enabled = true;
        core.settings.ai_summary_interval_hours = 6;
        core.settings.ai_summary = Some(AiSummary {
            text: "早晨".into(),
            generated_at: "2026-10-20 09:00".into(),
            source: SummarySource::Ai,
        });

        let before = ai_summary_view(&core.settings, now);
        assert!(before.stale);
        assert_eq!(before.next_due_at.as_deref(), Some("2026-10-20 15:00"));

        let after = core.refresh_instant_summary_at(now);
        assert!(!after.stale);
        assert_eq!(after.next_due_at.as_deref(), Some("2026-10-20 22:00"));
        assert_eq!(after.window_start.as_deref(), Some("2026-10-20 16:00"));
        assert_eq!(after.window_end.as_deref(), Some("2026-10-20 22:00"));
    }

    #[test]
    fn instant_summary_overwrites_text_with_instant_source() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 10, 20)
            .unwrap()
            .and_hms_opt(10, 0, 0)
            .unwrap();
        let mut done = task(1, "已完成");
        done.status = TaskStatus::Done;
        let mut doing = task(2, "进行中");
        doing.status = TaskStatus::Doing;
        let mut core = core_with(vec![done, doing]);
        core.settings.ai_summary_enabled = false;

        let view = core.refresh_instant_summary_at(now);
        assert_eq!(view.source, Some(SummarySource::Instant));
        assert_eq!(view.generated_at.as_deref(), Some("2026-10-20 10:00"));
        assert!(
            view.text
                .as_deref()
                .unwrap()
                .contains("今天共 2 项任务，已完成 1 项（50%），还剩 1 项。")
        );
        assert!(!view.enabled);
        assert!(core.is_dirty());
    }

    #[test]
    fn status_input_accepts_codes_and_chinese() {
        assert_eq!(parse_status("done"), Some(TaskStatus::Done));
        assert_eq!(parse_status("进行中"), Some(TaskStatus::Doing));
        assert_eq!(parse_status("weird"), None);
    }

    #[test]
    fn update_error_keeps_old_task_and_dirty_state() {
        let a = task(1, "A");
        let mut b = task(2, "B");
        b.blocked_by = vec![1];
        let mut core = core_with(vec![a, b]);

        let err = core.update(
            1,
            TaskPatch {
                text: Some("A 改".into()),
                blocked_by: Some(vec![2]),
                ..TaskPatch::default()
            },
        );
        assert!(err.is_err());
        assert_eq!(core.tasks()[0].text, "A");
        assert!(!core.is_dirty());
    }

    #[test]
    fn create_task_draft_builds_checklist_and_rejects_bad_input() {
        let mut core = core_with(vec![]);
        let now = chrono::NaiveDate::from_ymd_opt(2026, 10, 20)
            .unwrap()
            .and_hms_opt(10, 0, 0)
            .unwrap();
        let mut draft = TaskEditDraft::new_task("带子项".into());
        draft.due = "2026-10-21 09:00".into();
        draft.repeat = Some(Repeat::new(state::RepeatUnit::Day, 1));
        draft.subtasks = vec![Subtask::new(" 一 ".into()), Subtask::new(" ".into())];
        let view = core.create_task_draft(&draft, now).unwrap();
        assert_eq!(view.subtask_total, 1);
        assert_eq!(view.subtask_done, 0);
        assert_eq!(view.repeat.as_deref(), Some("每天"));
        assert!(core.is_dirty());

        let before = core.tasks().len();
        let mut bad = TaskEditDraft::new_task("坏任务".into());
        bad.due = "明天".into();
        assert!(core.create_task_draft(&bad, now).is_err());
        assert_eq!(core.tasks().len(), before);

        let mut no_due = TaskEditDraft::new_task("没截止".into());
        no_due.repeat = Some(Repeat::new(state::RepeatUnit::Week, 1));
        assert!(core.create_task_draft(&no_due, now).is_err());
    }

    #[test]
    fn add_and_update_manage_embedded_subtasks() {
        let mut core = core_with(vec![]);
        let added = core
            .add(NewTask {
                text: "带清单".into(),
                subtasks: vec![
                    Subtask::new(" 一 ".into()),
                    Subtask {
                        text: "二".into(),
                        done: true,
                    },
                    Subtask::new(" ".into()),
                ],
                ..NewTask::default()
            })
            .unwrap();
        assert_eq!(added.subtask_total, 2);
        assert_eq!(added.subtask_done, 1);
        assert_eq!(added.subtasks[0].text, "一");

        let replaced = core
            .update(
                added.id,
                TaskPatch {
                    subtasks: Some(vec![Subtask::new("换".into())]),
                    ..TaskPatch::default()
                },
            )
            .unwrap();
        assert_eq!(replaced.task.subtask_total, 1);
        assert_eq!(replaced.task.subtasks[0].text, "换");

        let unchanged = core.update(added.id, TaskPatch::default()).unwrap();
        assert_eq!(unchanged.task.subtask_total, 1);

        let cleared = core
            .update(
                added.id,
                TaskPatch {
                    subtasks: Some(Vec::new()),
                    ..TaskPatch::default()
                },
            )
            .unwrap();
        assert_eq!(cleared.task.subtask_total, 0);
        assert!(core.is_dirty());
    }

    #[test]
    fn subtask_toggle_does_not_complete_the_parent() {
        let mut t = task(1, "有子项");
        t.subtasks = vec![Subtask::new("A".into()), Subtask::new("B".into())];
        let mut core = core_with(vec![t]);

        let view = core.set_subtask_done(1, 0, true).unwrap();
        assert_eq!(view.subtask_done, 1);
        assert_eq!(view.status, "todo");
        assert!(core.is_dirty());

        core.set_subtask_done(1, 1, true).unwrap();
        assert_eq!(core.view(1).unwrap().status, "todo");
        assert!(core.set_subtask_done(1, 9, true).is_err());
    }

    #[test]
    fn add_text_allocates_a_task_and_marks_dirty() {
        let mut core = core_with(vec![]);
        let view = core.add_text("新任务").unwrap();
        assert_eq!(view.text, "新任务");
        assert!(core.is_dirty());
        assert_eq!(core.tasks().len(), 1);
    }

    #[test]
    fn timestamps_are_created_and_exposed_through_the_view() {
        let mut core = core_with(vec![]);
        let created = core.add_text("有时间戳").unwrap();
        assert!(created.created_at.is_some());
        assert_eq!(created.created_at, created.updated_at);
        assert!(created.completed_at.is_none());

        core.set_status(created.id, TaskStatus::Done).unwrap();
        let done = core.view(created.id).unwrap();
        assert!(done.completed_at.is_some());

        core.set_status(created.id, TaskStatus::Todo).unwrap();
        assert!(core.view(created.id).unwrap().completed_at.is_none());
    }

    #[test]
    fn font_size_is_written_through_core_for_persistence() {
        let mut core = core_with(vec![]);
        assert_eq!(core.settings().font_size, FontSize::Small);
        core.set_font_size(FontSize::Large);
        assert_eq!(core.settings().font_size, FontSize::Large);
        assert!(core.is_dirty());
    }

    #[test]
    fn autostart_is_persisted_without_entering_settings_view_or_patch() {
        let mut core = core_with(vec![]);
        assert!(!core.settings().autostart);
        core.set_autostart(true);
        assert!(core.settings().autostart);
        assert!(core.is_dirty());

        core.update_settings(SettingsPatch {
            show_seconds: Some(true),
            ..Default::default()
        })
        .unwrap();
        assert!(core.settings().autostart);
    }

    #[test]
    fn status_and_notes_go_through_core_commands() {
        let mut core = core_with(vec![task(1, "A")]);
        core.set_status(1, TaskStatus::Doing).unwrap();
        assert_eq!(core.tasks()[0].status, TaskStatus::Doing);

        let note = core.add_note();
        core.update_note(note, "记一笔");
        assert_eq!(core.settings().notes.last().unwrap().text, "记一笔");
        core.remove_note(note);
        assert!(!core.settings().notes.iter().any(|n| n.id == note));
    }
}
