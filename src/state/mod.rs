use chrono::{Datelike, NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};

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

mod settings;
mod time;

pub(crate) use settings::default_ai_interval_hours;
pub use settings::{
    AiSummary, CloseBehavior, FontSize, NoteCard, Settings, SummarySource, ThemeKind, View,
};
pub use time::{
    data_dir, format_due, load, normalize_clock, normalize_due, parse_clock, parse_due, save,
    state_path,
};

#[cfg(test)]
mod tests;
