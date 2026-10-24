//! 低饱和暖色主题。明度层级参考 Solarized：大面积底色低对比，
//! 语义色只用于小面积标记；新增主题时在 `state::ThemeKind` 和这里各加一处。

use craboard::state::ThemeKind;
use eframe::egui::{self, Color32, Stroke};
use std::cell::Cell;

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub side: Color32,
    pub surface: Color32,
    pub column: Color32,
    pub field: Color32,
    pub hover: Color32,
    pub line: Color32,
    pub text: Color32,
    pub dim: Color32,
    pub accent: Color32,
    /// 强调色和状态色填充上的文字/图标
    pub on_color: Color32,
    pub done: Color32,
    pub doing: Color32,
    pub red: Color32,
    pub warn: Color32,
}

impl Palette {
    pub fn tint(self, color: Color32, alpha: f32) -> Color32 {
        tint(color, alpha)
    }

    pub fn accent_fill(self, alpha: f32) -> Color32 {
        tint(self.accent, alpha)
    }
}

fn tint(color: Color32, alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        color.r(),
        color.g(),
        color.b(),
        (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

/// 暖夜：偏黄的深灰底，避开纯黑与蓝调；正文不用纯白，减少暗环境眩光。
pub const WARM_NIGHT: Palette = Palette {
    bg: Color32::from_rgb(28, 27, 24),
    side: Color32::from_rgb(33, 31, 27),
    surface: Color32::from_rgb(40, 37, 31),
    column: Color32::from_rgb(35, 33, 28),
    field: Color32::from_rgb(51, 47, 39),
    hover: Color32::from_rgb(61, 56, 46),
    line: Color32::from_rgb(70, 64, 53),
    text: Color32::from_rgb(212, 204, 189),
    dim: Color32::from_rgb(163, 155, 139),
    accent: Color32::from_rgb(208, 138, 92),
    on_color: Color32::from_rgb(34, 26, 19),
    done: Color32::from_rgb(154, 176, 131),
    doing: Color32::from_rgb(143, 168, 190),
    red: Color32::from_rgb(201, 125, 112),
    warn: Color32::from_rgb(200, 166, 107),
};

/// 暖纸：低亮度米白，不用纯白；正文用暖炭色。
pub const WARM_PAPER: Palette = Palette {
    bg: Color32::from_rgb(242, 236, 221),
    side: Color32::from_rgb(234, 227, 210),
    surface: Color32::from_rgb(247, 242, 230),
    column: Color32::from_rgb(232, 225, 209),
    field: Color32::from_rgb(227, 219, 201),
    hover: Color32::from_rgb(217, 208, 188),
    line: Color32::from_rgb(207, 197, 177),
    text: Color32::from_rgb(74, 69, 59),
    dim: Color32::from_rgb(113, 105, 90),
    accent: Color32::from_rgb(158, 87, 41),
    on_color: Color32::from_rgb(255, 248, 236),
    done: Color32::from_rgb(82, 112, 71),
    doing: Color32::from_rgb(74, 109, 133),
    red: Color32::from_rgb(163, 79, 66),
    warn: Color32::from_rgb(134, 99, 42),
};

thread_local! {
    static ACTIVE_KIND: Cell<ThemeKind> = const { Cell::new(ThemeKind::WarmNight) };
    static ACTIVE: Cell<Palette> = const { Cell::new(WARM_NIGHT) };
}

pub fn palette_of(kind: ThemeKind) -> Palette {
    match kind {
        ThemeKind::WarmNight => WARM_NIGHT,
        ThemeKind::WarmPaper => WARM_PAPER,
    }
}

pub fn is_dark(kind: ThemeKind) -> bool {
    matches!(kind, ThemeKind::WarmNight)
}

pub fn active() -> Palette {
    ACTIVE.with(Cell::get)
}

pub fn active_kind() -> ThemeKind {
    ACTIVE_KIND.with(Cell::get)
}

pub fn apply(ctx: &egui::Context, kind: ThemeKind) {
    let p = palette_of(kind);
    ACTIVE_KIND.with(|c| c.set(kind));
    ACTIVE.with(|c| c.set(p));

    let dark = is_dark(kind);
    let egui_theme = if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    ctx.set_theme(egui_theme);

    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.override_text_color = Some(p.text);
    // 按钮、选择项等可点控件统一显示手型，和常见桌面应用保持一致
    visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
    visuals.panel_fill = p.bg;
    visuals.window_fill = p.surface;
    visuals.extreme_bg_color = p.bg;
    visuals.faint_bg_color = p.column;
    visuals.code_bg_color = p.field;
    visuals.hyperlink_color = p.accent;
    visuals.warn_fg_color = p.warn;
    visuals.error_fg_color = p.red;
    visuals.selection.bg_fill = tint(p.accent, 0.22);
    visuals.selection.stroke = Stroke::new(1.0, p.accent);

    visuals.widgets.noninteractive.bg_fill = Color32::TRANSPARENT;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.line);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    visuals.widgets.inactive.bg_fill = p.field;
    visuals.widgets.inactive.weak_bg_fill = p.field;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, p.line);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, p.text);
    visuals.widgets.hovered.bg_fill = p.hover;
    visuals.widgets.hovered.weak_bg_fill = p.hover;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, p.line);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, p.text);
    visuals.widgets.active.bg_fill = p.accent;
    visuals.widgets.active.weak_bg_fill = p.accent;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, p.accent);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, p.on_color);
    visuals.widgets.open.bg_fill = p.field;
    visuals.widgets.open.weak_bg_fill = p.field;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, p.line);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, p.text);

    let shadow = egui::epaint::Shadow {
        color: Color32::from_black_alpha(if dark { 90 } else { 28 }),
        offset: [0, 4],
        blur: if dark { 18 } else { 14 },
        spread: 0,
    };
    visuals.window_shadow = shadow;
    visuals.popup_shadow = shadow;

    let mut style = (*ctx.style_of(egui_theme)).clone();
    style.visuals = visuals.clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    // 固定滚动条默认按前景色画把手，改成与按钮同样的底色，深浅主题都能看清
    style.spacing.scroll.foreground_color = false;
    ctx.set_visuals(visuals);
    ctx.set_style_of(egui_theme, style);
}
