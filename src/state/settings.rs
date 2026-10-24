use super::time::{format_due, normalize_clock, parse_due};
use super::{Subtask, TaskStatus, TodoItem};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum View {
    #[default]
    Today,
    All,
    Kanban,
    Quadrant,
}

/// 界面字号档位。小档是原有外观，缩放只作用于文字，不改窗口和布局尺寸。
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum FontSize {
    #[default]
    Small,
    Medium,
    Large,
}

impl FontSize {
    pub const ALL: [Self; 3] = [Self::Small, Self::Medium, Self::Large];

    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "小",
            Self::Medium => "中",
            Self::Large => "大",
        }
    }

    pub fn scale(self) -> f32 {
        match self {
            Self::Small => 1.0,
            Self::Medium => 1.12,
            Self::Large => 1.25,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "small" => Some(Self::Small),
            "medium" => Some(Self::Medium),
            "large" => Some(Self::Large),
            _ => None,
        }
    }
}

/// 界面主题。新增主题时在这里加一项，并在 `ui::theme` 补对应调色板。
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemeKind {
    #[default]
    WarmNight,
    WarmPaper,
}

/// 关闭主窗口（标题栏 ×、Alt+F4、任务栏关闭）时的动作。托盘菜单的「退出」不受影响。
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum CloseBehavior {
    /// 弹确认框，由用户当场选择
    #[default]
    Ask,
    Tray,
    Quit,
}

impl CloseBehavior {
    pub const ALL: [Self; 3] = [Self::Ask, Self::Tray, Self::Quit];

    pub fn label(self) -> &'static str {
        match self {
            Self::Ask => "每次询问",
            Self::Tray => "最小化到托盘",
            Self::Quit => "退出程序",
        }
    }
}

impl ThemeKind {
    pub const ALL: [Self; 2] = [Self::WarmNight, Self::WarmPaper];

    pub fn label(self) -> &'static str {
        match self {
            Self::WarmNight => "暖夜",
            Self::WarmPaper => "暖纸",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct NoteCard {
    #[serde(alias = "card_id")]
    pub id: u64,
    pub text: String,
}

fn default_true() -> bool {
    true
}

fn default_notes_width() -> f32 {
    280.0
}

fn default_wip() -> usize {
    3
}

fn default_urgent_hours() -> u32 {
    24
}

fn default_reminder_seconds() -> u32 {
    5
}

pub(crate) fn default_ai_interval_hours() -> u32 {
    6
}

/// 总结来源：定时 AI 写入，或用户点击「即时总结」现场生成。
#[derive(Serialize, Deserialize, Clone, Copy, Default, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SummarySource {
    #[default]
    Ai,
    Instant,
}

/// 最近一次今日总结。没有这个字段就表示还没有任何总结。
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct AiSummary {
    pub text: String,
    /// `YYYY-MM-DD HH:MM`，即写入总结的时间
    pub generated_at: String,
    #[serde(default)]
    pub source: SummarySource,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Settings {
    pub next_id: u64,
    pub pos: [f32; 2],
    pub size: [f32; 2],
    /// 任务已由独立存储接管；这里只用于读取旧 JSON 存档，保存时不再回写
    #[serde(default, skip_serializing)]
    pub todos: Vec<TodoItem>,
    pub notes: Vec<NoteCard>,
    pub show_seconds: bool,
    /// 登录 Windows 后自动启动；只由 GUI 修改，注册表由 GUI 负责写入
    #[serde(default)]
    pub autostart: bool,
    /// 关闭主窗口的默认动作；同样只属于 GUI
    #[serde(default)]
    pub close_behavior: CloseBehavior,
    #[serde(default)]
    pub font_size: FontSize,
    #[serde(default)]
    pub theme: ThemeKind,
    #[serde(default = "default_true")]
    pub notes_open: bool,
    #[serde(default = "default_notes_width")]
    pub notes_width: f32,
    pub view: View,
    /// 旧版本用 kanban 布尔记视图，读到时在 sanitize 里迁移一次
    #[serde(rename = "kanban", default, skip_serializing)]
    legacy_kanban: Option<bool>,
    #[serde(default = "default_wip")]
    pub wip_limit: usize,
    /// 纵向堆叠时各列是否收起，顺序 TODO/DOING/DONE/OVERDUE
    pub kanban_collapsed: [bool; 4],
    /// 四象限里「紧急」的时间窗，含已逾期
    #[serde(default = "default_urgent_hours")]
    pub urgent_within_hours: u32,
    #[serde(default = "default_true")]
    pub reminders_enabled: bool,
    #[serde(default = "default_reminder_seconds")]
    pub reminder_seconds: u32,
    /// 列表里收起了子项的任务；只影响子项清单的展开状态
    #[serde(default)]
    pub collapsed_tasks: Vec<u64>,
    /// 定时总结总开关；关闭后保留最近内容，但不再提示待更新
    #[serde(default = "default_true")]
    pub ai_summary_enabled: bool,
    /// 固定间隔模式：距上次任意来源的总结满 N 小时后触发
    #[serde(default = "default_true")]
    pub ai_summary_interval_enabled: bool,
    /// 定时总结的刷新间隔；固定间隔模式使用
    #[serde(default = "default_ai_interval_hours")]
    pub ai_summary_interval_hours: u32,
    /// 每日定点模式：每个时间点触发一次，窗口从当天 00:00 到该时刻
    #[serde(default)]
    pub ai_summary_daily_enabled: bool,
    /// `HH:MM`，升序去重
    #[serde(default)]
    pub ai_summary_daily_times: Vec<String>,
    #[serde(default)]
    pub ai_summary: Option<AiSummary>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            next_id: 2,
            pos: [80.0, 80.0],
            size: [320.0, 560.0],
            todos: Vec::new(),
            notes: vec![NoteCard {
                id: 1,
                text: String::new(),
            }],
            show_seconds: false,
            autostart: false,
            close_behavior: CloseBehavior::Ask,
            font_size: FontSize::Small,
            theme: ThemeKind::WarmNight,
            notes_open: true,
            notes_width: 280.0,
            view: View::default(),
            legacy_kanban: None,
            wip_limit: 3,
            kanban_collapsed: [false; 4],
            urgent_within_hours: 24,
            reminders_enabled: true,
            reminder_seconds: 5,
            collapsed_tasks: Vec::new(),
            ai_summary_enabled: true,
            ai_summary_interval_enabled: true,
            ai_summary_interval_hours: 6,
            ai_summary_daily_enabled: false,
            ai_summary_daily_times: Vec::new(),
            ai_summary: None,
        }
    }
}

impl Settings {
    pub fn take_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn task(&self, id: u64) -> Option<&TodoItem> {
        self.todos.iter().find(|t| t.id == id)
    }

    pub fn remove_task(&mut self, id: u64) {
        self.todos.retain(|t| t.id != id);
        for t in &mut self.todos {
            t.blocked_by.retain(|b| *b != id);
        }
    }

    /// 取出在 (last, now] 内到期的未完成任务，并记下已提醒的截止时间。
    /// 只处理本次运行期间跨过的时点：启动时已经逾期的旧任务不会一次性弹窗。
    pub fn take_due_reminders(
        &mut self,
        last: NaiveDateTime,
        now: NaiveDateTime,
    ) -> Vec<(u64, String, NaiveDateTime)> {
        let mut out = Vec::new();
        for t in &mut self.todos {
            if t.status == TaskStatus::Done {
                continue;
            }
            let Some(due) = t.due_at() else {
                continue;
            };
            if due <= last || due > now {
                continue;
            }
            let Some(due_str) = t.due.as_deref() else {
                continue;
            };
            if t.reminded_due.as_deref() == Some(due_str) {
                continue;
            }
            t.reminded_due = Some(due_str.to_owned());
            out.push((t.id, t.text.clone(), due));
        }
        out.sort_by(|a, b| a.2.cmp(&b.2).then(a.0.cmp(&b.0)));
        out
    }

    pub fn add_note(&mut self) -> u64 {
        let id = self.take_id();
        self.notes.push(NoteCard {
            id,
            text: String::new(),
        });
        self.notes_open = true;
        id
    }

    pub fn remove_note(&mut self, id: u64) {
        self.notes.retain(|n| n.id != id);
    }

    /// next_id 可能落后于已有条目（旧存档里 id 各自独立分配），会发出重复 id
    pub fn sanitize_ids(&mut self) {
        let mut max = self
            .todos
            .iter()
            .map(|t| t.id)
            .chain(self.notes.iter().map(|n| n.id))
            .max()
            .unwrap_or(0);
        max = max.max(self.next_id.saturating_sub(1));
        self.next_id = max + 1;
    }

    /// 旧版把子任务存成独立任务行；读档后按 id 顺序并入根任务的子项清单，子行删除。
    /// 父任务不存在、自成环或父链成环的行保留为顶层任务。
    fn flatten_legacy_children(&mut self) {
        if !self.todos.iter().any(TodoItem::has_legacy_parent) {
            return;
        }
        let ids: HashSet<u64> = self.todos.iter().map(|t| t.id).collect();
        let parent_of: HashMap<u64, Option<u64>> =
            self.todos.iter().map(|t| (t.id, t.legacy_parent)).collect();
        let mut drop = HashSet::new();
        let mut additions: HashMap<u64, Vec<(u64, Subtask)>> = HashMap::new();
        for t in &self.todos {
            let Some(parent) = t.legacy_parent else {
                continue;
            };
            if parent == t.id || !ids.contains(&parent) {
                continue;
            }
            let mut root = parent;
            let mut cur = parent;
            let mut seen = HashSet::new();
            seen.insert(t.id);
            let mut broken = false;
            loop {
                if !seen.insert(cur) {
                    broken = true;
                    break;
                }
                match parent_of.get(&cur).copied().flatten() {
                    Some(next) if ids.contains(&next) => {
                        root = next;
                        cur = next;
                    }
                    _ => break,
                }
            }
            if broken {
                continue;
            }
            drop.insert(t.id);
            additions.entry(root).or_default().push((
                t.id,
                Subtask {
                    text: t.text.clone(),
                    done: t.is_done(),
                },
            ));
        }
        for t in &mut self.todos {
            if let Some(mut items) = additions.remove(&t.id) {
                items.sort_by_key(|(id, _)| *id);
                t.subtasks.extend(items.into_iter().map(|(_, s)| s));
            }
            t.legacy_parent = None;
        }
        self.todos.retain(|t| !drop.contains(&t.id));
    }

    pub fn sanitize(&mut self) {
        if let Some(kanban) = self.legacy_kanban.take() {
            self.view = if kanban { View::Kanban } else { View::All };
        }
        // 旧版的 cards 字段已废弃；serde 会忽略存档里的多余字段，
        // 便签正文本来就存在 notes 里，不需要再从卡片迁移
        for t in &mut self.todos {
            if t.legacy_done && t.status == TaskStatus::Todo {
                t.status = TaskStatus::Done;
            }
            t.legacy_done = false;
            if t.due.as_deref().is_some_and(|d| parse_due(d).is_none()) {
                t.due = None;
                t.reminded_due = None;
            }
            match t.repeat {
                Some(mut r) => {
                    r.interval = r.interval.clamp(1, 999);
                    t.repeat = Some(r);
                    // 周期任务必须有截止时间；旧数据缺基准时用当前截止时间补上
                    if t.due_at().is_none() {
                        t.repeat = None;
                        t.repeat_anchor = None;
                    } else if t.repeat_anchor.as_deref().and_then(parse_due).is_none() {
                        t.repeat_anchor = t.due.clone();
                    }
                }
                None => t.repeat_anchor = None,
            }
            // 旧数据没有时间戳：创建时间用创建日补 00:00；完成时间无法还原，保持未知
            let created = t.created_at.as_deref().and_then(parse_due);
            t.created_at = created.map(format_due).or_else(|| {
                let fallback = format!("{} 00:00", t.day);
                parse_due(&fallback).map(|_| fallback)
            });
            let updated = t.updated_at.as_deref().and_then(parse_due);
            t.updated_at = updated.map(format_due).or_else(|| t.created_at.clone());
            let completed = t.completed_at.as_deref().and_then(parse_due);
            t.completed_at = completed.map(format_due);
            if t.status != TaskStatus::Done {
                t.completed_at = None;
            }
        }
        self.notes.sort_by_key(|n| n.id);
        let mut seen_notes = HashSet::new();
        self.notes.retain(|n| seen_notes.insert(n.id));

        // 任务 id 是存储主键，旧存档里可能出现重复
        let mut seen_todos = HashSet::new();
        self.todos.retain(|t| seen_todos.insert(t.id));

        self.flatten_legacy_children();
        for t in &mut self.todos {
            t.subtasks = t
                .subtasks
                .drain(..)
                .filter_map(|s| {
                    let text = s.text.trim().to_owned();
                    (!text.is_empty()).then_some(Subtask { text, done: s.done })
                })
                .collect();
        }

        let ids: HashSet<u64> = self.todos.iter().map(|t| t.id).collect();
        self.collapsed_tasks.retain(|id| ids.contains(id));
        self.collapsed_tasks.sort_unstable();
        self.collapsed_tasks.dedup();
        self.ai_summary_interval_hours = self.ai_summary_interval_hours.clamp(1, 168);
        let mut daily_times: Vec<String> = self
            .ai_summary_daily_times
            .iter()
            .filter_map(|raw| normalize_clock(raw))
            .collect();
        daily_times.sort();
        daily_times.dedup();
        daily_times.truncate(24);
        self.ai_summary_daily_times = daily_times;
        if let Some(mut summary) = self.ai_summary.take() {
            summary.text = summary.text.trim().to_owned();
            if !summary.text.is_empty() && parse_due(&summary.generated_at).is_some() {
                self.ai_summary = Some(summary);
            }
        }
        for t in &mut self.todos {
            t.blocked_by.retain(|b| *b != t.id && ids.contains(b));
            t.blocked_by.sort_unstable();
            t.blocked_by.dedup();
        }

        // 前置依赖同样要无环
        let dep_map: HashMap<u64, Vec<u64>> = self
            .todos
            .iter()
            .map(|t| (t.id, t.blocked_by.clone()))
            .collect();
        let mut broken = HashSet::new();
        for t in &self.todos {
            for dep in &t.blocked_by {
                if dep_reaches(&dep_map, *dep, t.id) {
                    broken.insert((t.id, *dep));
                }
            }
        }
        for t in &mut self.todos {
            t.blocked_by.retain(|b| !broken.contains(&(t.id, *b)));
        }

        self.notes_width = self.notes_width.clamp(200.0, 480.0);
        self.wip_limit = self.wip_limit.clamp(1, 99);
        self.urgent_within_hours = self.urgent_within_hours.clamp(1, 720);
        self.reminder_seconds = self.reminder_seconds.clamp(3, 60);
        self.sanitize_ids();
    }

    fn blocker_names(&self, t: &TodoItem) -> Vec<&str> {
        t.blocked_by
            .iter()
            .filter_map(|id| self.task(*id))
            .filter(|b| b.status != TaskStatus::Done)
            .map(|b| b.text.as_str())
            .take(3)
            .collect()
    }

    /// 状态迁移检查。进入 DOING/DONE 要求前置完成；进入 DOING 还受 WIP 上限约束。
    /// 返回 Err 时调用方必须保持原状态。
    pub fn check_status(&self, id: u64, target: TaskStatus) -> Result<(), String> {
        let Some(t) = self.task(id) else {
            return Err("任务不存在".to_owned());
        };
        let mut problems = Vec::new();
        if target != TaskStatus::Todo {
            let names = self.blocker_names(t);
            if !names.is_empty() {
                problems.push(format!("前置任务未完成：{}", names.join("、")));
            }
        }
        if target == TaskStatus::Doing && t.status != TaskStatus::Doing {
            let doing = self
                .todos
                .iter()
                .filter(|x| x.status == TaskStatus::Doing && x.id != id)
                .count();
            let limit = self.wip_limit.max(1);
            if doing >= limit {
                problems.push(format!("进行中已达上限 {limit} 个"));
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(format!("「{}」{}", t.text, problems.join("；")))
        }
    }

    /// 状态迁移。周期任务标记完成时直接推进到下一轮并回到待办，
    /// 返回下一轮的截止时间，调用方可以据此提示用户。
    pub fn set_status(
        &mut self,
        id: u64,
        target: TaskStatus,
        now: NaiveDateTime,
    ) -> Result<Option<NaiveDateTime>, String> {
        self.check_status(id, target)?;
        let Some(t) = self.todos.iter_mut().find(|t| t.id == id) else {
            return Err("任务不存在".to_owned());
        };
        let previous = t.status;
        let stamp = format_due(now);
        if target == TaskStatus::Done {
            if let Some(repeat) = t.repeat {
                let anchor = t
                    .repeat_anchor
                    .as_deref()
                    .and_then(parse_due)
                    .or_else(|| t.due_at())
                    .unwrap_or(now);
                // 本轮被延期过时，下一轮必须晚于延期后的截止，不能和本轮重合
                let from = t.due_at().filter(|d| *d > now).unwrap_or(now);
                let next = repeat.next_after(anchor, from);
                t.status = TaskStatus::Todo;
                t.due = Some(format_due(next));
                t.repeat_anchor = Some(format_due(next));
                t.reminded_due = None;
                t.updated_at = Some(stamp);
                // 周期任务只是推进一轮，任务本身没有完成；子项留给下一轮重新勾
                for s in &mut t.subtasks {
                    s.done = false;
                }
                t.completed_at = None;
                return Ok(Some(next));
            }
            // 普通任务完成时把子项一并勾上；取消完成不回退子项
            for s in &mut t.subtasks {
                s.done = true;
            }
        }
        if target != previous {
            t.updated_at = Some(stamp.clone());
            t.completed_at = (target == TaskStatus::Done).then_some(stamp);
        }
        t.status = target;
        Ok(None)
    }

    /// 延期只改这一轮的截止时间；有周期基准时保持不动，下一轮仍按原计划。
    pub fn postpone(
        &mut self,
        id: u64,
        by: chrono::Duration,
        now: NaiveDateTime,
    ) -> Result<NaiveDateTime, String> {
        let Some(t) = self.todos.iter_mut().find(|t| t.id == id) else {
            return Err("任务不存在".to_owned());
        };
        let base = t.due_at().filter(|d| *d > now).unwrap_or(now);
        if t.repeat.is_some() && t.repeat_anchor.is_none() {
            t.repeat_anchor = t.due.clone();
        }
        let next = base + by;
        t.due = Some(format_due(next));
        t.reminded_due = None;
        t.updated_at = Some(format_due(now));
        Ok(next)
    }

    pub fn postpone_to(
        &mut self,
        id: u64,
        due: NaiveDateTime,
        now: NaiveDateTime,
    ) -> Result<(), String> {
        let Some(t) = self.todos.iter_mut().find(|t| t.id == id) else {
            return Err("任务不存在".to_owned());
        };
        if t.repeat.is_some() && t.repeat_anchor.is_none() {
            t.repeat_anchor = t.due.clone();
        }
        t.due = Some(format_due(due));
        t.reminded_due = None;
        t.updated_at = Some(format_due(now));
        Ok(())
    }

    pub fn dep_has_cycle(&self, id: u64) -> bool {
        let map: HashMap<u64, Vec<u64>> = self
            .todos
            .iter()
            .map(|t| (t.id, t.blocked_by.clone()))
            .collect();
        map.get(&id)
            .is_some_and(|deps| deps.iter().any(|dep| dep_reaches(&map, *dep, id)))
    }

    /// 把 dep 设为 id 的前置是否会成环（当前图 + 这条新边）
    pub fn dep_would_cycle(&self, id: u64, dep: u64) -> bool {
        if dep == id {
            return true;
        }
        let map: HashMap<u64, Vec<u64>> = self
            .todos
            .iter()
            .map(|t| (t.id, t.blocked_by.clone()))
            .collect();
        dep_reaches(&map, dep, id)
    }
}

fn dep_reaches(map: &HashMap<u64, Vec<u64>>, from: u64, target: u64) -> bool {
    let mut stack = vec![from];
    let mut seen = HashSet::new();
    while let Some(x) = stack.pop() {
        if x == target {
            return true;
        }
        if seen.insert(x) {
            if let Some(deps) = map.get(&x) {
                stack.extend(deps.iter().copied());
            }
        }
    }
    false
}
