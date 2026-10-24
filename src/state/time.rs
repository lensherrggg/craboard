use super::settings::Settings;
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use std::path::PathBuf;

pub fn format_due(dt: NaiveDateTime) -> String {
    dt.format("%Y-%m-%d %H:%M").to_string()
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
