//! 试验功能开关：读取 exe 同目录的 `experimental.json`。
//! 文件缺失或解析失败时全部关闭，不影响正式功能。

use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "experimental.json";

const SUMMARY_KEYS: [&str; 6] = [
    "ai_summary_enabled",
    "ai_summary_available",
    "ai_summary_daily_enabled",
    "ai_summary_daily_times",
    "ai_summary_interval_enabled",
    "ai_summary_interval_hours",
];

const ACTIVE_KEYS: [&str; 5] = [
    "ai_summary_enabled",
    "ai_summary_daily_enabled",
    "ai_summary_daily_times",
    "ai_summary_interval_enabled",
    "ai_summary_interval_hours",
];

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Experimental {
    /// 展示总结面板，并开放 MCP 的总结读写工具
    pub ai_summary: bool,
    /// 开放即时总结和每日定点/固定间隔；开启时自动视为展示总结
    pub ai_summary_active: bool,
}

impl Experimental {
    pub fn summary(&self) -> bool {
        self.ai_summary || self.ai_summary_active
    }

    pub fn active(&self) -> bool {
        self.ai_summary_active
    }

    pub fn path() -> Option<PathBuf> {
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join(FILE_NAME)))
    }

    pub fn path_label() -> String {
        Self::path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| FILE_NAME.to_owned())
    }

    pub fn load() -> (Self, Option<String>) {
        let Some(path) = Self::path() else {
            return (
                Self::default(),
                Some("无法定位程序目录，试验功能已关闭".to_owned()),
            );
        };
        Self::load_from(&path)
    }

    pub fn load_from(path: &Path) -> (Self, Option<String>) {
        if !path.exists() {
            return (Self::default(), None);
        }
        match std::fs::read_to_string(path) {
            Ok(raw) => {
                // 记事本另存为 UTF-8 时可能带 BOM
                let raw = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
                match serde_json::from_str::<Self>(raw) {
                    Ok(cfg) => (cfg, None),
                    Err(e) => (
                        Self::default(),
                        Some(format!("{FILE_NAME} 解析失败，试验功能已关闭：{e}")),
                    ),
                }
            }
            Err(e) => (
                Self::default(),
                Some(format!("{FILE_NAME} 读取失败，试验功能已关闭：{e}")),
            ),
        }
    }

    /// 从对外设置里移除未开启的试验字段，避免 MCP/CLI 暴露隐藏选项。
    pub fn filter_settings_json(&self, settings: &mut serde_json::Value) {
        let Some(map) = settings.as_object_mut() else {
            return;
        };
        if !self.summary() {
            for key in SUMMARY_KEYS {
                map.remove(key);
            }
        } else if !self.active() {
            for key in ACTIVE_KEYS {
                map.remove(key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_implies_summary() {
        let cfg = Experimental {
            ai_summary: false,
            ai_summary_active: true,
        };
        assert!(cfg.summary());
        assert!(cfg.active());

        let push_only = Experimental {
            ai_summary: true,
            ai_summary_active: false,
        };
        assert!(push_only.summary());
        assert!(!push_only.active());
        assert!(!Experimental::default().summary());
    }

    #[test]
    fn parses_partial_config_and_rejects_bad_types() {
        let cfg: Experimental = serde_json::from_str(r#"{"ai_summary": true}"#).unwrap();
        assert!(cfg.ai_summary);
        assert!(!cfg.ai_summary_active);

        let cfg: Experimental = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg, Experimental::default());

        assert!(serde_json::from_str::<Experimental>(r#"{"ai_summary": "yes"}"#).is_err());
    }

    #[test]
    fn missing_file_keeps_everything_off() {
        let path = std::env::temp_dir().join(format!(
            "craboard-experimental-missing-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let (cfg, warning) = Experimental::load_from(&path);
        assert_eq!(cfg, Experimental::default());
        assert!(warning.is_none());
    }

    #[test]
    fn filters_settings_by_gate() {
        let full = serde_json::json!({
            "show_seconds": true,
            "ai_summary_enabled": true,
            "ai_summary_available": true,
            "ai_summary_daily_enabled": true,
            "ai_summary_daily_times": ["09:00"],
            "ai_summary_interval_enabled": true,
            "ai_summary_interval_hours": 6
        });

        let mut off = full.clone();
        Experimental::default().filter_settings_json(&mut off);
        assert!(off.get("ai_summary_enabled").is_none());
        assert!(off.get("ai_summary_available").is_none());
        assert!(off.get("show_seconds").is_some());

        let mut push = full.clone();
        Experimental {
            ai_summary: true,
            ai_summary_active: false,
        }
        .filter_settings_json(&mut push);
        assert!(push.get("ai_summary_available").is_some());
        assert!(push.get("ai_summary_enabled").is_none());
        assert!(push.get("ai_summary_interval_hours").is_none());

        let mut active = full;
        Experimental {
            ai_summary: true,
            ai_summary_active: true,
        }
        .filter_settings_json(&mut active);
        assert!(active.get("ai_summary_enabled").is_some());
        assert!(active.get("ai_summary_interval_hours").is_some());
    }
}
