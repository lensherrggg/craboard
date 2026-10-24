//! 低饱和主题。明度层级参考 Solarized：大面积底色低对比，
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

/// 青灰：冷调深灰，降低蓝光感但不发蓝。
pub const SLATE_NIGHT: Palette = Palette {
    bg: Color32::from_rgb(24, 27, 30),
    side: Color32::from_rgb(29, 32, 36),
    surface: Color32::from_rgb(36, 40, 45),
    column: Color32::from_rgb(32, 36, 40),
    field: Color32::from_rgb(46, 51, 57),
    hover: Color32::from_rgb(54, 60, 67),
    line: Color32::from_rgb(64, 71, 79),
    text: Color32::from_rgb(198, 207, 214),
    dim: Color32::from_rgb(148, 159, 169),
    accent: Color32::from_rgb(139, 164, 187),
    on_color: Color32::from_rgb(22, 28, 34),
    done: Color32::from_rgb(139, 172, 148),
    doing: Color32::from_rgb(137, 166, 190),
    red: Color32::from_rgb(200, 128, 124),
    warn: Color32::from_rgb(202, 170, 116),
};

/// 雾青：冷调浅灰，接近阴天窗边的纸面。
pub const SLATE_PAPER: Palette = Palette {
    bg: Color32::from_rgb(235, 239, 242),
    side: Color32::from_rgb(227, 232, 236),
    surface: Color32::from_rgb(242, 246, 248),
    column: Color32::from_rgb(224, 230, 234),
    field: Color32::from_rgb(218, 225, 230),
    hover: Color32::from_rgb(208, 217, 223),
    line: Color32::from_rgb(196, 206, 213),
    text: Color32::from_rgb(58, 68, 76),
    dim: Color32::from_rgb(88, 101, 111),
    accent: Color32::from_rgb(72, 107, 130),
    on_color: Color32::from_rgb(248, 251, 252),
    done: Color32::from_rgb(68, 108, 82),
    doing: Color32::from_rgb(66, 102, 132),
    red: Color32::from_rgb(158, 76, 68),
    warn: Color32::from_rgb(128, 98, 44),
};

/// 苔夜：绿灰深色，像背阴处的苔藓。
pub const MOSS_NIGHT: Palette = Palette {
    bg: Color32::from_rgb(24, 28, 25),
    side: Color32::from_rgb(29, 34, 30),
    surface: Color32::from_rgb(36, 41, 36),
    column: Color32::from_rgb(32, 37, 33),
    field: Color32::from_rgb(46, 52, 46),
    hover: Color32::from_rgb(55, 62, 54),
    line: Color32::from_rgb(65, 73, 64),
    text: Color32::from_rgb(199, 207, 196),
    dim: Color32::from_rgb(150, 160, 148),
    accent: Color32::from_rgb(157, 179, 130),
    on_color: Color32::from_rgb(24, 30, 21),
    done: Color32::from_rgb(156, 184, 136),
    doing: Color32::from_rgb(138, 166, 179),
    red: Color32::from_rgb(200, 128, 116),
    warn: Color32::from_rgb(199, 170, 108),
};

/// 苔纸：绿灰浅色，接近旧书纸。
pub const MOSS_PAPER: Palette = Palette {
    bg: Color32::from_rgb(237, 240, 232),
    side: Color32::from_rgb(229, 233, 224),
    surface: Color32::from_rgb(244, 247, 239),
    column: Color32::from_rgb(226, 231, 219),
    field: Color32::from_rgb(219, 225, 211),
    hover: Color32::from_rgb(209, 216, 200),
    line: Color32::from_rgb(197, 205, 187),
    text: Color32::from_rgb(64, 72, 58),
    dim: Color32::from_rgb(96, 105, 88),
    accent: Color32::from_rgb(94, 117, 64),
    on_color: Color32::from_rgb(250, 252, 246),
    done: Color32::from_rgb(68, 106, 64),
    doing: Color32::from_rgb(68, 102, 128),
    red: Color32::from_rgb(158, 76, 62),
    warn: Color32::from_rgb(130, 98, 42),
};

/// 暮紫：低饱和紫灰深色，像日落后的一小段天空。
pub const MAUVE_NIGHT: Palette = Palette {
    bg: Color32::from_rgb(28, 25, 30),
    side: Color32::from_rgb(33, 30, 36),
    surface: Color32::from_rgb(40, 37, 44),
    column: Color32::from_rgb(36, 33, 40),
    field: Color32::from_rgb(51, 47, 56),
    hover: Color32::from_rgb(61, 56, 67),
    line: Color32::from_rgb(72, 66, 79),
    text: Color32::from_rgb(211, 202, 215),
    dim: Color32::from_rgb(161, 152, 167),
    accent: Color32::from_rgb(186, 150, 171),
    on_color: Color32::from_rgb(32, 24, 30),
    done: Color32::from_rgb(146, 178, 150),
    doing: Color32::from_rgb(142, 160, 192),
    red: Color32::from_rgb(204, 126, 132),
    warn: Color32::from_rgb(198, 166, 111),
};

/// 藕荷：低饱和紫灰浅色，带一点粉调但不过暖。
pub const MAUVE_PAPER: Palette = Palette {
    bg: Color32::from_rgb(241, 236, 241),
    side: Color32::from_rgb(233, 228, 234),
    surface: Color32::from_rgb(247, 242, 248),
    column: Color32::from_rgb(231, 225, 232),
    field: Color32::from_rgb(226, 219, 227),
    hover: Color32::from_rgb(216, 208, 218),
    line: Color32::from_rgb(205, 196, 207),
    text: Color32::from_rgb(72, 64, 74),
    dim: Color32::from_rgb(104, 94, 106),
    accent: Color32::from_rgb(145, 96, 126),
    on_color: Color32::from_rgb(253, 247, 252),
    done: Color32::from_rgb(74, 110, 80),
    doing: Color32::from_rgb(72, 102, 134),
    red: Color32::from_rgb(160, 76, 86),
    warn: Color32::from_rgb(132, 98, 44),
};

thread_local! {
    static ACTIVE_KIND: Cell<ThemeKind> = const { Cell::new(ThemeKind::WarmNight) };
    static ACTIVE: Cell<Palette> = const { Cell::new(WARM_NIGHT) };
}

pub fn palette_of(kind: ThemeKind) -> Palette {
    match kind {
        ThemeKind::WarmNight => WARM_NIGHT,
        ThemeKind::WarmPaper => WARM_PAPER,
        ThemeKind::SlateNight => SLATE_NIGHT,
        ThemeKind::SlatePaper => SLATE_PAPER,
        ThemeKind::MossNight => MOSS_NIGHT,
        ThemeKind::MossPaper => MOSS_PAPER,
        ThemeKind::MauveNight => MAUVE_NIGHT,
        ThemeKind::MauvePaper => MAUVE_PAPER,
    }
}

pub fn is_dark(kind: ThemeKind) -> bool {
    matches!(
        kind,
        ThemeKind::WarmNight | ThemeKind::SlateNight | ThemeKind::MossNight | ThemeKind::MauveNight
    )
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
