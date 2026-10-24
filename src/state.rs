use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TaskStatus {
    #[default]
    Todo,
    Doing,
    Done,
}

impl TaskStatus {
    pub fn label(self) -> &'static str {
        match self {
            TaskStatus::Todo => "待办",
            TaskStatus::Doing => "进行中",
            TaskStatus::Done => "已完成",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum RepeatUnit {
    Day,
    Week,
    Month,
    Year,
}

impl RepeatUnit {
    pub fn label(self) -> &'static str {
        match self {
            RepeatUnit::Day => "天",
            RepeatUnit::Week => "周",
            RepeatUnit::Month => "月",
            RepeatUnit::Year => "年",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().to_ascii_lowercase();
        let unit = match s.as_str() {
            "d" | "day" | "days" | "daily" | "天" | "每天" | "每日" => Self::Day,
            "w" | "week" | "weeks" | "weekly" | "周" | "每周" | "每星期" => Self::Week,
            "m" | "month" | "months" | "monthly" | "月" | "每月" => Self::Month,
            "y" | "year" | "years" | "yearly" | "年" | "每年" => Self::Year,
            _ => return None,
        };
        Some(unit)
    }
}

/// 周期规则。间隔为 1 时就是每天/每周/每月/每年。
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Repeat {
    pub unit: RepeatUnit,
    #[serde(default = "one")]
    pub interval: u32,
}

fn one() -> u32 {
    1
}

impl Repeat {
    pub fn new(unit: RepeatUnit, interval: u32) -> Self {
        Self {
            unit,
            interval: interval.clamp(1, 999),
        }
    }

    pub fn label(self) -> String {
        if self.interval <= 1 {
            format!("每{}", self.unit.label())
        } else {
            format!("每 {} {}", self.interval, self.unit.label())
        }
    }

    /// 接受 daily、2w、每 2 周、monthly 这类写法
    pub fn parse(input: &str) -> Option<Self> {
        let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();
        let s = cleaned.strip_prefix('每').unwrap_or(&cleaned);
        if s.is_empty() {
            return None;
        }
        let mut interval = 0u32;
        let mut unit_start = s.len();
        for (i, c) in s.char_indices() {
            match c.to_digit(10) {
                Some(d) => interval = interval.saturating_mul(10).saturating_add(d),
                None => {
                    unit_start = i;
                    break;
                }
            }
        }
        let unit = RepeatUnit::parse(&s[unit_start..])?;
        Some(Self::new(unit, interval.max(1)))
    }

    pub fn advance(self, from: NaiveDateTime) -> NaiveDateTime {
        let n = self.interval.max(1);
        match self.unit {
            RepeatUnit::Day => from + chrono::Duration::days(n as i64),
            RepeatUnit::Week => from + chrono::Duration::weeks(n as i64),
            RepeatUnit::Month => add_months(from, n),
            RepeatUnit::Year => add_months(from, 12 * n),
        }
    }

    /// 从当前这一轮往后推进，跳过已经错过的轮次
    pub fn next_after(self, anchor: NaiveDateTime, now: NaiveDateTime) -> NaiveDateTime {
        let mut next = self.advance(anchor);
        for _ in 0..10_000 {
            if next > now {
                break;
            }
            next = self.advance(next);
        }
        next
    }
}

/// 任务内的可勾选子项。只是附属清单，不是独立任务：
/// 不设截止/状态/前置，也不参与今日进度和 WIP 计数。
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Subtask {
    pub text: String,
    #[serde(default)]
    pub done: bool,
}

impl Subtask {
    pub fn new(text: String) -> Self {
        Self { text, done: false }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct TodoItem {
    pub id: u64,
    pub text: String,
    #[serde(default)]
    pub status: TaskStatus,
    /// 旧版本只有 done 布尔。新存档不再写这个字段，读到旧值时在 sanitize 里迁移。
    #[serde(rename = "done", default, skip_serializing)]
    legacy_done: bool,
    /// 截止时间，本地时间，格式 YYYY-MM-DD HH:MM
    #[serde(default)]
    pub due: Option<String>,
    /// 旧版本把子任务存成独立任务行；读档后并入根任务的 subtasks，不再写回
    #[serde(rename = "parent", default, skip_serializing)]
    legacy_parent: Option<u64>,
    #[serde(default)]
    pub subtasks: Vec<Subtask>,
    #[serde(default)]
    pub blocked_by: Vec<u64>,
    /// 创建日（本地日期，格式 YYYY-MM-DD），用于「今日」筛选
    pub day: String,
    /// 创建时间（本地时间，格式 YYYY-MM-DD HH:MM）；旧数据可能缺失
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// 最近一次用户可见修改的时间
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    /// 完成时间；取消完成后清空。周期任务完成一轮不算完成，保持为空
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    /// 已经弹过提醒的截止时间（原样存 due 字符串）；改期后清空，重新提醒
    #[serde(default)]
    pub reminded_due: Option<String>,
    /// 二元的重要标记；以后要做优先级时在这里扩展
    #[serde(default)]
    pub important: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat: Option<Repeat>,
    /// 本轮在周期序列里的基准时间。延期只改 due，不动它，所以下一轮回到原计划
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat_anchor: Option<String>,
}

/// 后端还原一条任务时用的扁平记录，避免长参数列表
#[derive(Clone, Debug, Default)]
pub struct TaskRecord {
    pub id: u64,
    pub text: String,
    pub status: TaskStatus,
    pub important: bool,
    pub due: Option<String>,
    /// 旧库的 parent 列；只用于迁移，不再作为任务关系
    pub parent: Option<u64>,
    pub subtasks: Vec<Subtask>,
    pub blocked_by: Vec<u64>,
    pub day: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub completed_at: Option<String>,
    pub reminded_due: Option<String>,
    pub repeat: Option<Repeat>,
    pub repeat_anchor: Option<String>,
}

impl TodoItem {
    pub fn new(id: u64, text: String, day: String) -> Self {
        Self {
            id,
            text,
            status: TaskStatus::Todo,
            legacy_done: false,
            due: None,
            legacy_parent: None,
            subtasks: Vec::new(),
            blocked_by: Vec::new(),
            day,
            created_at: None,
            updated_at: None,
            completed_at: None,
            reminded_due: None,
            important: false,
            repeat: None,
            repeat_anchor: None,
        }
    }

    pub fn from_record(r: TaskRecord) -> Self {
        Self {
            id: r.id,
            text: r.text,
            status: r.status,
            legacy_done: false,
            due: r.due,
            legacy_parent: r.parent,
            subtasks: r.subtasks,
            blocked_by: r.blocked_by,
            day: r.day,
            created_at: r.created_at,
            updated_at: r.updated_at,
            completed_at: r.completed_at,
            reminded_due: r.reminded_due,
            important: r.important,
            repeat: r.repeat,
            repeat_anchor: r.repeat_anchor,
        }
    }

    pub fn is_done(&self) -> bool {
        self.status == TaskStatus::Done
    }

    /// 旧存档/旧库里还有独立子任务行，需要并进本任务的子项清单
    pub fn has_legacy_parent(&self) -> bool {
        self.legacy_parent.is_some()
    }

    pub fn subtask_done(&self) -> usize {
        self.subtasks.iter().filter(|s| s.done).count()
    }

    pub fn due_at(&self) -> Option<NaiveDateTime> {
        self.due.as_deref().and_then(parse_due)
    }

    /// 已完成的任务不再算逾期
    pub fn past_due(&self, now: NaiveDateTime) -> bool {
        !self.is_done() && self.due_at().is_some_and(|d| d < now)
    }

    /// 没截止时间的任务不算紧急；已逾期的始终算紧急
    pub fn is_urgent(&self, now: NaiveDateTime, within_hours: u32) -> bool {
        if self.is_done() {
            return false;
        }
        self.due_at()
            .is_some_and(|d| d <= now + chrono::Duration::hours(within_hours as i64))
    }

    pub fn quadrant(&self, now: NaiveDateTime, within_hours: u32) -> Quadrant {
        Quadrant::of(self.important, self.is_urgent(now, within_hours))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Quadrant {
    ImportantUrgent,
    Important,
    Urgent,
    Normal,
}

impl Quadrant {
    pub fn of(important: bool, urgent: bool) -> Self {
        match (important, urgent) {
            (true, true) => Self::ImportantUrgent,
            (true, false) => Self::Important,
            (false, true) => Self::Urgent,
            (false, false) => Self::Normal,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::ImportantUrgent => "重要且紧急",
            Self::Important => "重要不紧急",
            Self::Urgent => "紧急不重要",
            Self::Normal => "不紧急不重要",
        }
    }

    pub const ALL: [Quadrant; 4] = [
        Quadrant::ImportantUrgent,
        Quadrant::Important,
        Quadrant::Urgent,
        Quadrant::Normal,
    ];
}

pub fn format_due(dt: NaiveDateTime) -> String {
    dt.format("%Y-%m-%d %H:%M").to_string()
}

/// 按自然月推进，目标月没有这一天时落到月末
fn add_months(from: NaiveDateTime, months: u32) -> NaiveDateTime {
    let date = from.date();
    let index = date.year() as i64 * 12 + date.month0() as i64 + months as i64;
    let year = index.div_euclid(12) as i32;
    let month = index.rem_euclid(12) as u32 + 1;
    let Some(first) = NaiveDate::from_ymd_opt(year, month, 1) else {
        return from;
    };
    let next_first = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    };
    let last = next_first
        .map(|n| n.signed_duration_since(first).num_days() as u32)
        .unwrap_or(28);
    NaiveDate::from_ymd_opt(year, month, date.day().min(last))
        .unwrap_or(date)
        .and_time(from.time())
}

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

pub fn parse_due(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M")
        .ok()
        .or_else(|| {
            NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .ok()
                .and_then(|d| d.and_hms_opt(23, 59, 0))
        })
}

/// `HH:MM` 归一化成两位小时/分钟；接受 `9:05` 这类写法
pub fn normalize_clock(input: &str) -> Option<String> {
    let time = NaiveTime::parse_from_str(input.trim(), "%H:%M").ok()?;
    Some(time.format("%H:%M").to_string())
}

pub fn parse_clock(input: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(input.trim(), "%H:%M").ok()
}

/// 只给日期时按当天 23:59 存
pub fn normalize_due(input: &str) -> Result<Option<String>, ()> {
    let s = input.trim();
    if s.is_empty() {
        return Ok(None);
    }
    let dt = parse_due(s).ok_or(())?;
    Ok(Some(dt.format("%Y-%m-%d %H:%M").to_string()))
}

pub fn data_dir() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("craboard")
}

fn file() -> PathBuf {
    data_dir().join("state.json")
}

/// 界面监听外部（命令行 / MCP）改设置时用
pub fn state_path() -> PathBuf {
    file()
}

pub fn load() -> Settings {
    let path = file();
    let raw = std::fs::read_to_string(&path).ok();
    // 任务迁到独立存储前，留一份仍带 todos 的旧存档
    let has_legacy_tasks = raw
        .as_deref()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .is_some_and(|v| v.get("todos").is_some());
    if has_legacy_tasks {
        let backup = path.with_extension("json.before-sqlite");
        if !backup.exists() {
            let _ = std::fs::copy(&path, backup);
        }
    }
    let mut st: Settings = raw
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    st.sanitize();
    st
}

/// 先写临时文件再替换：避免中途崩溃把原文件写成半截 JSON
pub fn save(s: &Settings) -> std::io::Result<()> {
    let path = file();
    std::fs::create_dir_all(data_dir())?;
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_string_pretty(s).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, body)?;
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    std::fs::rename(&tmp, &path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: u64, text: &str) -> TodoItem {
        TodoItem::new(id, text.to_owned(), "2026-10-20".to_owned())
    }

    fn state(todos: Vec<TodoItem>) -> Settings {
        let mut st = Settings::default();
        st.next_id = 100;
        st.todos = todos;
        st
    }

    fn dt(h: u32, m: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, 20)
            .unwrap()
            .and_hms_opt(h, m, 0)
            .unwrap()
    }

    #[test]
    fn legacy_done_migrates() {
        let mut st = state(vec![TodoItem {
            id: 7,
            text: "旧任务".into(),
            status: TaskStatus::Todo,
            legacy_done: true,
            due: None,
            legacy_parent: None,
            subtasks: vec![],
            blocked_by: vec![],
            day: "2026-10-20".into(),
            created_at: None,
            updated_at: None,
            completed_at: None,
            reminded_due: None,
            important: false,
            repeat: None,
            repeat_anchor: None,
        }]);
        st.sanitize();
        assert_eq!(st.todos[0].status, TaskStatus::Done);
    }

    #[test]
    fn legacy_kanban_migrates_to_view() {
        let mut st: Settings = serde_json::from_str(r#"{"kanban":true}"#).unwrap();
        st.sanitize();
        assert_eq!(st.view, View::Kanban);

        let mut st: Settings = serde_json::from_str(r#"{"kanban":false}"#).unwrap();
        st.sanitize();
        assert_eq!(st.view, View::All);

        let mut st: Settings = serde_json::from_str(r#"{"view":"Today"}"#).unwrap();
        st.sanitize();
        assert_eq!(st.view, View::Today);
    }

    #[test]
    fn due_accepts_date_and_datetime() {
        assert_eq!(normalize_due(""), Ok(None));
        assert_eq!(
            normalize_due("2026-10-21"),
            Ok(Some("2026-10-21 23:59".to_owned()))
        );
        assert_eq!(
            normalize_due("2026-10-21 18:00"),
            Ok(Some("2026-10-21 18:00".to_owned()))
        );
        assert!(normalize_due("明天").is_err());
    }

    #[test]
    fn status_checks_prerequisites_and_completes_subtasks() {
        let prereq = task(1, "前置");
        let mut t = task(2, "任务");
        t.blocked_by = vec![1];
        t.subtasks = vec![Subtask::new("子项".into())];
        let mut st = state(vec![prereq, t]);
        st.wip_limit = 3;
        let now = dt(10, 0);
        assert!(st.set_status(2, TaskStatus::Doing, now).is_err());
        st.set_status(1, TaskStatus::Done, now).unwrap();
        st.set_status(2, TaskStatus::Doing, now).unwrap();
        // 子项没勾不影响主任务完成，完成时自动勾上
        st.set_status(2, TaskStatus::Done, now).unwrap();
        assert!(st.todos[1].subtasks[0].done);
    }

    #[test]
    fn legacy_children_flatten_into_root_subtasks() {
        let mut st: Settings = serde_json::from_str(
            r#"{"todos":[
                {"id":1,"text":"根","day":"2026-10-20"},
                {"id":2,"text":"子 B","done":true,"day":"2026-10-20","parent":1},
                {"id":3,"text":"孙","day":"2026-10-20","parent":2},
                {"id":4,"text":"孤儿","day":"2026-10-20","parent":99}
            ]}"#,
        )
        .unwrap();
        st.sanitize();
        assert_eq!(st.todos.len(), 2);
        let root = st.task(1).unwrap();
        assert_eq!(
            root.subtasks,
            vec![
                Subtask {
                    text: "子 B".into(),
                    done: true
                },
                Subtask {
                    text: "孙".into(),
                    done: false
                },
            ]
        );
        assert!(st.task(3).is_none());
        assert!(st.task(4).is_some());
        assert!(!serde_json::to_string(&st).unwrap().contains("parent"));
    }

    #[test]
    fn recurring_completion_resets_subtasks() {
        let mut t = recurring(1, "2026-10-20 09:00", RepeatUnit::Day, 1);
        t.subtasks = vec![
            Subtask {
                text: "A".into(),
                done: true,
            },
            Subtask {
                text: "B".into(),
                done: true,
            },
        ];
        let mut st = state(vec![t]);
        st.set_status(1, TaskStatus::Done, dt(8, 0)).unwrap();
        assert!(st.todos[0].subtasks.iter().all(|s| !s.done));
    }

    #[test]
    fn subtasks_are_trimmed_and_empty_ones_dropped() {
        let mut t = task(1, "A");
        t.subtasks = vec![
            Subtask {
                text: "  写提纲 ".into(),
                done: true,
            },
            Subtask::new("   ".into()),
        ];
        let mut st = state(vec![t]);
        st.sanitize();
        assert_eq!(
            st.todos[0].subtasks,
            vec![Subtask {
                text: "写提纲".into(),
                done: true
            }]
        );
    }

    #[test]
    fn due_reminders_fire_once_and_again_after_reschedule() {
        let mut t = task(1, "到期任务");
        t.due = Some("2026-10-20 10:00".into());
        let mut st = state(vec![t]);

        assert!(st.take_due_reminders(dt(9, 0), dt(9, 59)).is_empty());
        let got = st.take_due_reminders(dt(9, 59), dt(10, 0));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].1, "到期任务");
        assert!(st.take_due_reminders(dt(10, 0), dt(10, 30)).is_empty());

        st.todos[0].due = Some("2026-10-20 11:00".into());
        assert_eq!(st.take_due_reminders(dt(10, 30), dt(11, 1)).len(), 1);

        st.todos[0].status = TaskStatus::Done;
        st.todos[0].due = Some("2026-10-20 12:00".into());
        assert!(st.take_due_reminders(dt(11, 1), dt(12, 1)).is_empty());
    }

    #[test]
    fn wip_limit_blocks_new_doing() {
        let mut a = task(1, "A");
        a.status = TaskStatus::Doing;
        let b = task(2, "B");
        let mut st = state(vec![a, b]);
        st.wip_limit = 1;
        let now = dt(10, 0);
        assert!(st.set_status(2, TaskStatus::Doing, now).is_err());
        st.wip_limit = 2;
        assert!(st.set_status(2, TaskStatus::Doing, now).is_ok());
    }

    #[test]
    fn dependency_cycle_check_ignores_the_task_itself() {
        let a = task(1, "A");
        let mut b = task(2, "B");
        b.blocked_by = vec![1];
        let mut st = state(vec![a, b]);
        assert!(!st.dep_has_cycle(1));
        assert!(!st.dep_has_cycle(2));

        st.todos[0].blocked_by = vec![2];
        assert!(st.dep_has_cycle(1));
        assert!(st.dep_has_cycle(2));
    }

    #[test]
    fn dependency_cycles_are_detected_and_broken() {
        let mut a = task(1, "A");
        let mut b = task(2, "B");
        a.blocked_by = vec![2];
        b.blocked_by = vec![1];
        let mut st = state(vec![a, b]);
        st.sanitize();
        assert!(st.todos.iter().all(|t| t.blocked_by.is_empty()));
    }

    #[test]
    fn legacy_parent_cycles_stay_top_level() {
        let mut st: Settings = serde_json::from_str(
            r#"{"todos":[
                {"id":1,"text":"A","day":"2026-10-20","parent":2},
                {"id":2,"text":"B","day":"2026-10-20","parent":1}
            ]}"#,
        )
        .unwrap();
        st.sanitize();
        assert_eq!(st.todos.len(), 2);
        assert!(st.todos.iter().all(|t| t.subtasks.is_empty()));
    }

    #[test]
    fn settings_save_omits_tasks_but_still_reads_legacy() {
        let st = state(vec![task(1, "A")]);
        let json = serde_json::to_string(&st).unwrap();
        assert!(!json.contains("todos"));
        assert!(
            serde_json::from_str::<Settings>(&json)
                .unwrap()
                .todos
                .is_empty()
        );

        let mut legacy: Settings = serde_json::from_str(
            r#"{"todos":[{"id":3,"text":"旧任务","done":true,"day":"2026-10-20"}]}"#,
        )
        .unwrap();
        assert!(legacy.todos[0].legacy_done);
        legacy.sanitize();
        assert!(legacy.todos[0].is_done());
    }

    #[test]
    fn duplicate_task_ids_are_dropped() {
        let mut st = state(vec![task(1, "先来的"), task(1, "重复的")]);
        st.sanitize();
        assert_eq!(st.todos.len(), 1);
        assert_eq!(st.todos[0].text, "先来的");
    }

    #[test]
    fn deleting_task_clears_references() {
        let a = task(1, "A");
        let mut b = task(2, "B");
        b.blocked_by = vec![1];
        let mut st = state(vec![a, b]);
        st.remove_task(1);
        assert_eq!(st.todos.len(), 1);
        assert!(st.todos[0].blocked_by.is_empty());
    }

    fn recurring(id: u64, due: &str, unit: RepeatUnit, interval: u32) -> TodoItem {
        let mut t = task(id, "周期任务");
        t.due = Some(due.to_owned());
        t.repeat = Some(Repeat::new(unit, interval));
        t.repeat_anchor = Some(due.to_owned());
        t
    }

    #[test]
    fn recurring_completion_advances_to_next_round() {
        let mut st = state(vec![recurring(1, "2026-10-20 09:00", RepeatUnit::Day, 1)]);

        let next = st.set_status(1, TaskStatus::Done, dt(8, 0)).unwrap();
        assert_eq!(next, parse_due("2026-10-21 09:00"));
        let t = &st.todos[0];
        assert_eq!(t.status, TaskStatus::Todo);
        assert_eq!(t.due.as_deref(), Some("2026-10-21 09:00"));
        assert_eq!(t.repeat_anchor.as_deref(), Some("2026-10-21 09:00"));
        assert_eq!(t.completed_at, None);
        assert_eq!(t.updated_at.as_deref(), Some("2026-10-20 08:00"));

        let next = st.set_status(1, TaskStatus::Done, dt(8, 0)).unwrap();
        assert_eq!(next, parse_due("2026-10-22 09:00"));
    }

    #[test]
    fn timestamps_follow_status_and_postpone() {
        let mut st = state(vec![task(1, "A")]);
        st.set_status(1, TaskStatus::Doing, dt(8, 0)).unwrap();
        assert_eq!(st.todos[0].updated_at.as_deref(), Some("2026-10-20 08:00"));
        assert_eq!(st.todos[0].completed_at, None);

        st.set_status(1, TaskStatus::Done, dt(9, 30)).unwrap();
        assert_eq!(
            st.todos[0].completed_at.as_deref(),
            Some("2026-10-20 09:30")
        );

        st.set_status(1, TaskStatus::Todo, dt(10, 0)).unwrap();
        assert_eq!(st.todos[0].completed_at, None);
        assert_eq!(st.todos[0].updated_at.as_deref(), Some("2026-10-20 10:00"));

        st.postpone(1, chrono::Duration::days(1), dt(11, 0))
            .unwrap();
        assert_eq!(st.todos[0].updated_at.as_deref(), Some("2026-10-20 11:00"));
    }

    #[test]
    fn sanitize_backfills_missing_timestamps_from_creation_day() {
        let mut done = task(1, "旧的已完成");
        done.status = TaskStatus::Done;
        let mut st = state(vec![done]);
        st.sanitize();
        assert_eq!(st.todos[0].created_at.as_deref(), Some("2026-10-20 00:00"));
        assert_eq!(st.todos[0].updated_at.as_deref(), Some("2026-10-20 00:00"));
        // 旧数据没有完成时间，不能拿创建日冒充
        assert_eq!(st.todos[0].completed_at, None);
    }

    #[test]
    fn completing_postponed_round_does_not_repeat_the_same_due() {
        let mut st = state(vec![recurring(1, "2026-10-20 09:00", RepeatUnit::Day, 1)]);
        st.postpone(1, chrono::Duration::days(1), dt(8, 0)).unwrap();
        assert_eq!(st.todos[0].due.as_deref(), Some("2026-10-21 09:00"));
        assert_eq!(
            st.todos[0].repeat_anchor.as_deref(),
            Some("2026-10-20 09:00")
        );

        let next = st.set_status(1, TaskStatus::Done, dt(8, 30)).unwrap();
        assert_eq!(next, parse_due("2026-10-22 09:00"));
        assert_eq!(st.todos[0].due.as_deref(), Some("2026-10-22 09:00"));
    }

    #[test]
    fn late_completion_skips_missed_rounds() {
        let mut st = state(vec![recurring(1, "2026-10-18 09:00", RepeatUnit::Day, 1)]);
        let next = st.set_status(1, TaskStatus::Done, dt(10, 0)).unwrap();
        assert_eq!(next, parse_due("2026-10-21 09:00"));
    }

    #[test]
    fn recurring_interval_and_units() {
        let mut st = state(vec![recurring(1, "2026-10-20 09:00", RepeatUnit::Week, 2)]);
        let next = st.set_status(1, TaskStatus::Done, dt(8, 0)).unwrap();
        assert_eq!(next, parse_due("2026-11-03 09:00"));

        let mut st = state(vec![recurring(1, "2026-10-31 09:00", RepeatUnit::Month, 1)]);
        let now = NaiveDate::from_ymd_opt(2026, 10, 31)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap();
        let next = st.set_status(1, TaskStatus::Done, now).unwrap();
        assert_eq!(
            next,
            Some(
                NaiveDate::from_ymd_opt(2026, 11, 30)
                    .unwrap()
                    .and_hms_opt(9, 0, 0)
                    .unwrap()
            )
        );
    }

    #[test]
    fn postpone_keeps_the_recurring_anchor() {
        let mut st = state(vec![recurring(1, "2026-10-20 09:00", RepeatUnit::Day, 1)]);
        let now = dt(8, 0);
        let next = st.postpone(1, chrono::Duration::days(1), now).unwrap();
        assert_eq!(next, parse_due("2026-10-21 09:00").unwrap());
        assert_eq!(
            st.todos[0].repeat_anchor.as_deref(),
            Some("2026-10-20 09:00")
        );

        let done_at = NaiveDate::from_ymd_opt(2026, 10, 21)
            .unwrap()
            .and_hms_opt(10, 0, 0)
            .unwrap();
        let next = st.set_status(1, TaskStatus::Done, done_at).unwrap();
        assert_eq!(next, parse_due("2026-10-22 09:00"));
    }

    #[test]
    fn postpone_moves_overdue_from_now() {
        let mut t = task(1, "逾期任务");
        t.due = Some("2026-10-19 09:00".into());
        let mut st = state(vec![t]);
        let next = st
            .postpone(1, chrono::Duration::days(1), dt(10, 0))
            .unwrap();
        assert_eq!(next, parse_due("2026-10-21 10:00").unwrap());
        assert_eq!(st.todos[0].reminded_due, None);
    }

    #[test]
    fn urgency_uses_threshold_and_ignores_done() {
        let mut soon = task(1, "五小时后");
        soon.due = Some("2026-10-20 15:00".into());
        let mut later = task(2, "两天后");
        later.due = Some("2026-10-22 10:00".into());
        let mut overdue = task(3, "逾期");
        overdue.due = Some("2026-10-19 10:00".into());
        let mut done = task(4, "已完成");
        done.due = Some("2026-10-20 11:00".into());
        done.status = TaskStatus::Done;
        let none = task(5, "无截止");
        let now = dt(10, 0);

        assert!(soon.is_urgent(now, 24));
        assert!(!later.is_urgent(now, 24));
        assert!(overdue.is_urgent(now, 1));
        assert!(!done.is_urgent(now, 24));
        assert!(!none.is_urgent(now, 720));
        assert_eq!(soon.quadrant(now, 24), Quadrant::Urgent);
        assert_eq!(overdue.quadrant(now, 24), Quadrant::Urgent);
    }

    #[test]
    fn repeat_parsing() {
        assert_eq!(Repeat::parse("2w"), Some(Repeat::new(RepeatUnit::Week, 2)));
        assert_eq!(
            Repeat::parse("daily"),
            Some(Repeat::new(RepeatUnit::Day, 1))
        );
        assert_eq!(
            Repeat::parse("每 3 天"),
            Some(Repeat::new(RepeatUnit::Day, 3))
        );
        assert!(Repeat::parse("someday").is_none());
    }

    #[test]
    fn font_size_defaults_small_and_round_trips() {
        let mut st: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(st.font_size, FontSize::Small);
        st.font_size = FontSize::Large;
        let json = serde_json::to_string(&st).unwrap();
        assert!(json.contains("\"font_size\":\"large\""));
        assert_eq!(FontSize::parse("MEDIUM"), Some(FontSize::Medium));
        assert_eq!(FontSize::parse("大"), None);
        assert_eq!(FontSize::ALL.len(), 3);
    }

    #[test]
    fn theme_defaults_warm_night_and_round_trips() {
        let mut st: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(st.theme, ThemeKind::WarmNight);
        st.theme = ThemeKind::WarmPaper;
        let json = serde_json::to_string(&st).unwrap();
        assert!(json.contains("\"theme\":\"warm_paper\""));
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.theme, ThemeKind::WarmPaper);
        assert_eq!(ThemeKind::ALL.len(), 2);
    }

    #[test]
    fn autostart_defaults_false_and_round_trips() {
        let mut st: Settings = serde_json::from_str("{}").unwrap();
        assert!(!st.autostart);
        st.autostart = true;
        let json = serde_json::to_string(&st).unwrap();
        assert!(json.contains("\"autostart\":true"));
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert!(back.autostart);
    }

    #[test]
    fn close_behavior_defaults_to_ask_and_round_trips() {
        let mut st: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(st.close_behavior, CloseBehavior::Ask);
        st.close_behavior = CloseBehavior::Tray;
        let json = serde_json::to_string(&st).unwrap();
        assert!(json.contains("\"close_behavior\":\"tray\""));
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.close_behavior, CloseBehavior::Tray);
        assert_eq!(CloseBehavior::ALL.len(), 3);
    }

    #[test]
    fn settings_clamp_new_options() {
        let mut st: Settings =
            serde_json::from_str(r#"{"urgent_within_hours":0,"reminder_seconds":999}"#).unwrap();
        st.sanitize();
        assert_eq!(st.urgent_within_hours, 1);
        assert_eq!(st.reminder_seconds, 60);
    }

    #[test]
    fn ai_summary_defaults_and_sanitize() {
        let mut st: Settings = serde_json::from_str("{}").unwrap();
        st.sanitize();
        assert!(st.ai_summary_enabled);
        assert!(st.ai_summary_interval_enabled);
        assert_eq!(st.ai_summary_interval_hours, 6);
        assert!(!st.ai_summary_daily_enabled);
        assert!(st.ai_summary_daily_times.is_empty());
        assert!(st.ai_summary.is_none());

        st.ai_summary_daily_times = vec![
            "18:00".into(),
            "9:05".into(),
            "18:00".into(),
            "25:00".into(),
            "早上".into(),
        ];
        st.sanitize();
        assert_eq!(st.ai_summary_daily_times, ["09:05", "18:00"]);

        st.ai_summary_interval_hours = 0;
        st.ai_summary = Some(AiSummary {
            text: "  今日完成 2 项  ".into(),
            generated_at: "2026-10-20 19:30".into(),
            source: SummarySource::Instant,
        });
        st.sanitize();
        assert_eq!(st.ai_summary_interval_hours, 1);
        assert_eq!(st.ai_summary.as_ref().unwrap().text, "今日完成 2 项");
        assert_eq!(
            st.ai_summary.as_ref().unwrap().source,
            SummarySource::Instant
        );

        st.ai_summary_interval_hours = 999;
        st.ai_summary = Some(AiSummary {
            text: "总结".into(),
            generated_at: "不是时间".into(),
            source: SummarySource::Ai,
        });
        st.sanitize();
        assert_eq!(st.ai_summary_interval_hours, 168);
        assert!(st.ai_summary.is_none());
    }

    #[test]
    fn legacy_ai_summary_reads_as_ai_source() {
        let summary: AiSummary =
            serde_json::from_str(r#"{"text":"旧总结","generated_at":"2026-10-20 09:00"}"#).unwrap();
        assert_eq!(summary.source, SummarySource::Ai);
    }
}
