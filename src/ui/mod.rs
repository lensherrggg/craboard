pub mod board;
pub mod close;
pub mod kanban;
pub mod reminder;
pub mod settings;
pub mod theme;

use craboard::state::FontSize;
use eframe::egui;
use std::sync::atomic::{AtomicU32, Ordering};

static FONT_SCALE_1000: AtomicU32 = AtomicU32::new(1000);

/// 只缩放文字，不缩 UI：窗口尺寸、响应式断点和鼠标命中区域都保持不变。
pub fn font_scale() -> f32 {
    FONT_SCALE_1000.load(Ordering::Relaxed) as f32 / 1000.0
}

pub fn fs(base: f32) -> f32 {
    base * font_scale()
}

/// `YYYY-MM-DD HH:MM` 转成便于弹层阅读的今天/明天/日期标签
pub fn moment_label(raw: &str, now: chrono::NaiveDateTime) -> String {
    let Some(dt) = craboard::state::parse_due(raw) else {
        return raw.to_owned();
    };
    match (dt.date() - now.date()).num_days() {
        0 => format!("今天 {}", dt.format("%H:%M")),
        1 => format!("明天 {}", dt.format("%H:%M")),
        -1 => format!("昨天 {}", dt.format("%H:%M")),
        _ => dt.format("%m-%d %H:%M").to_string(),
    }
}

pub fn apply_font_size(ctx: &egui::Context, size: FontSize) {
    let scale = size.scale();
    FONT_SCALE_1000.store((scale * 1000.0).round() as u32, Ordering::Relaxed);
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (egui::TextStyle::Small, egui::FontId::proportional(fs(9.0))),
            (egui::TextStyle::Body, egui::FontId::proportional(fs(13.0))),
            (
                egui::TextStyle::Button,
                egui::FontId::proportional(fs(13.0)),
            ),
            (
                egui::TextStyle::Heading,
                egui::FontId::proportional(fs(18.0)),
            ),
            (
                egui::TextStyle::Monospace,
                egui::FontId::monospace(fs(13.0)),
            ),
        ]
        .into();
    });
}
