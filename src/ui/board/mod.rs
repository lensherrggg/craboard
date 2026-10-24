use super::fs;
use super::kanban;
use super::moment_label;
use super::theme;
use crate::{App, win32};
use chrono::{Datelike, NaiveDateTime};
use craboard::core::{
    AiSummaryView, ListFilter as TodoFilter, ListScope, TaskInfo, TodayProgress, ai_summary_view,
    due_label, list_rows, nav_stats, quadrant_tasks, task_infos, today_progress as today_counts,
};
use craboard::experimental::Experimental;
use craboard::state::{Quadrant, Settings, SummarySource, TaskStatus, View};
use eframe::egui;
use egui::{
    Align, Align2, Color32, CursorIcon, FontId, Frame, Label, Layout, Pos2, Rect, RichText,
    ScrollArea, Sense, Shape, Stroke, TextEdit, Vec2,
};
use std::collections::HashSet;
use std::time::Duration;

const MIN_W: f32 = 260.0;
const MIN_H: f32 = 360.0;
const EDGE: f32 = 6.0;
const CORNER: f32 = 14.0;
const NAV_W_TEXT: f32 = 92.0;
const NAV_W_ICON: f32 = 50.0;
const NAV_ICON_BREAK: f32 = 560.0;
const NOTES_MIN: f32 = 200.0;
const NOTES_MAX: f32 = 480.0;

fn nav_width(win_w: f32) -> f32 {
    if win_w < fs(NAV_ICON_BREAK) {
        NAV_W_ICON
    } else {
        fs(NAV_W_TEXT)
    }
}

/// 侧栏宽度。窗口太窄时最多让出 MIN_W 给主区，正常情况下就是用户设定的宽度。
fn notes_panel_width(win_w: f32, notes_width: f32) -> f32 {
    let max_w = (win_w - MIN_W).clamp(NOTES_MIN, NOTES_MAX);
    notes_width.clamp(NOTES_MIN, max_w)
}

/// 主区（导航 + 内容）占的宽度，不含右侧边栏。响应式断点只看它。
fn board_width(win_w: f32, notes_open: bool, notes_width: f32) -> f32 {
    if notes_open {
        win_w - notes_panel_width(win_w, notes_width)
    } else {
        win_w
    }
}

/// 展开/收起右侧边栏时改窗口宽度，保持左上角与主区宽度不变。
pub(crate) fn set_notes_open(app: &mut App, ctx: &egui::Context, open: bool) {
    if app.st().notes_open == open {
        return;
    }
    let size = ctx.input(|i| i.viewport().inner_rect.map(|r| r.size()));
    app.core.set_notes_open(open);
    if let Some(size) = size {
        let notes_width = app.st().notes_width;
        let new_w = if open {
            size.x + notes_width.clamp(NOTES_MIN, NOTES_MAX)
        } else {
            (size.x - notes_panel_width(size.x, notes_width)).max(MIN_W)
        };
        app.core.set_window_size([new_w, size.y]);
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(new_w, size.y)));
    }
}

/// 一次缩放拖拽的起点。窗口位置/尺寸都锚在按下那一刻，避免逐帧累加误差。
#[derive(Clone, Copy)]
pub struct ResizeDrag {
    dir: ResizeDir,
    outer: Pos2,
    size: Vec2,
    /// 物理屏幕光标。用窗口内坐标会随窗口移动/缩放而抵消
    cursor: (f32, f32),
    ppp: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct ResizeDir {
    left: bool,
    right: bool,
    top: bool,
    bottom: bool,
}

impl ResizeDir {
    const fn new(left: bool, right: bool, top: bool, bottom: bool) -> Self {
        Self {
            left,
            right,
            top,
            bottom,
        }
    }

    fn cursor(self) -> CursorIcon {
        match (self.left, self.right, self.top, self.bottom) {
            (true, _, true, _) | (_, true, _, true) => CursorIcon::ResizeNwSe,
            (_, true, true, _) | (true, _, _, true) => CursorIcon::ResizeNeSw,
            (true, _, _, _) | (_, true, _, _) => CursorIcon::ResizeHorizontal,
            _ => CursorIcon::ResizeVertical,
        }
    }
}

pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    title_bar(app, ui);
    left_bar(app, ui);

    if app.st().notes_open {
        side_panel(app, ui);
    }

    match app.st().view {
        View::Kanban => kanban::draw(app, ui),
        View::Quadrant => quadrant_view(app, ui),
        _ => list_view(app, ui),
    }

    toast(app, ui);
    kanban::task_editor(app, ui.ctx());
    super::settings::settings_modal(app, ui.ctx());
    window_edges(app, ui);
    grip_hint(ui);
}

fn left_bar(app: &mut App, ui: &mut egui::Ui) {
    let win_w = ui.max_rect().width();
    let own_w = board_width(win_w, app.st().notes_open, app.st().notes_width);
    let icon_mode = own_w < fs(NAV_ICON_BREAK);
    let width = nav_width(own_w);
    let infos = task_infos(app.st());
    let stats = nav_stats(&infos);
    let open = stats.open;
    let overdue = stats.overdue;

    let resp = egui::Panel::left("view-nav")
        .resizable(false)
        .show_separator_line(false)
        .exact_size(width)
        .frame(
            Frame::new()
                .fill(theme::active().side)
                .inner_margin(egui::Margin {
                    left: 8,
                    right: 8,
                    top: 10,
                    bottom: 14,
                }),
        )
        .show(ui, |ui| {
            if icon_mode {
                ui.add_space(2.0);
            } else {
                ui.label(
                    RichText::new("任务")
                        .size(fs(13.0))
                        .color(theme::active().text)
                        .strong(),
                );
                ui.add_space(2.0);
                pending_count(ui, open, overdue);
                ui.add_space(12.0);
            }

            if icon_mode {
                if nav_icon(ui, NavIcon::Today, app.st().view == View::Today, "今天").clicked() {
                    switch_view(app, View::Today);
                }
                ui.add_space(4.0);
                if nav_icon(ui, NavIcon::All, app.st().view == View::All, "全部任务").clicked()
                {
                    switch_view(app, View::All);
                }
                ui.add_space(4.0);
                if nav_icon(
                    ui,
                    NavIcon::Kanban,
                    app.st().view == View::Kanban,
                    "待办看板",
                )
                .clicked()
                {
                    switch_view(app, View::Kanban);
                }
                ui.add_space(4.0);
                if nav_icon(
                    ui,
                    NavIcon::Quadrant,
                    app.st().view == View::Quadrant,
                    "四象限",
                )
                .clicked()
                {
                    switch_view(app, View::Quadrant);
                }
            } else {
                if nav_button(ui, "今天", app.st().view == View::Today).clicked() {
                    switch_view(app, View::Today);
                }
                ui.add_space(4.0);
                if nav_button(ui, "全部任务", app.st().view == View::All).clicked() {
                    switch_view(app, View::All);
                }
                ui.add_space(4.0);
                if nav_button(ui, "看板", app.st().view == View::Kanban).clicked() {
                    switch_view(app, View::Kanban);
                }
                ui.add_space(4.0);
                if nav_button(ui, "四象限", app.st().view == View::Quadrant).clicked() {
                    switch_view(app, View::Quadrant);
                }
            }

            // 今日进度贴住左下角
            let refresh = today_progress(
                ui,
                today_counts(&infos),
                icon_mode,
                app.st(),
                app.experimental,
            );
            if refresh {
                app.core.refresh_instant_summary();
            }
        });
    let panel = resp.response.rect;
    ui.painter().vline(
        panel.right(),
        panel.y_range(),
        Stroke::new(1.0, theme::active().line),
    );
}

fn switch_view(app: &mut App, view: View) {
    app.core.set_view(view);
}

/// 数字随状态变色：有逾期转红，全部处理完转绿，其余用主色
fn pending_count(ui: &mut egui::Ui, open: usize, overdue: usize) {
    let color = if overdue > 0 {
        theme::active().red
    } else if open > 0 {
        theme::active().accent
    } else {
        theme::active().done
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(3.0, 0.0);
        ui.label(
            RichText::new(open.to_string())
                .size(fs(10.0))
                .color(color)
                .strong(),
        );
        ui.label(
            RichText::new("项待处理")
                .size(fs(10.0))
                .color(theme::active().dim),
        );
    });
}

fn nav_button(ui: &mut egui::Ui, text: &str, active: bool) -> egui::Response {
    let btn = egui::Button::new(RichText::new(text).size(fs(12.5)).color(if active {
        theme::active().accent
    } else {
        theme::active().dim
    }))
    .fill(if active {
        theme::active().accent_fill(0.12)
    } else {
        Color32::TRANSPARENT
    })
    .stroke(Stroke::NONE)
    .corner_radius(7.0)
    .min_size(Vec2::new(ui.available_width(), 30.0));
    ui.add(btn)
}

fn painted_sun(painter: &egui::Painter, c: Pos2, color: Color32) {
    painter.circle_stroke(c, 5.5, Stroke::new(1.4, color));
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::TAU / 8.0;
        let dir = Vec2::new(a.cos(), a.sin());
        painter.line_segment([c + dir * 8.5, c + dir * 10.5], Stroke::new(1.3, color));
    }
}

#[derive(Clone, Copy)]
enum NavIcon {
    Today,
    All,
    Kanban,
    Quadrant,
}

fn nav_icon(ui: &mut egui::Ui, icon: NavIcon, active: bool, tip: &str) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), Sense::click());
    if active {
        ui.painter()
            .rect_filled(rect, 7.0, theme::active().accent_fill(0.12));
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 7.0, theme::active().hover);
    }
    let color = if active {
        theme::active().accent
    } else {
        theme::active().dim
    };
    let c = rect.center();
    let painter = ui.painter();
    match icon {
        NavIcon::Today => {
            painted_sun(painter, c, color);
        }
        NavIcon::All => {
            for i in 0..3 {
                let y = c.y - 5.0 + i as f32 * 5.0;
                painter.circle_filled(Pos2::new(c.x - 7.0, y), 1.2, color);
                painter.line_segment(
                    [Pos2::new(c.x - 3.5, y), Pos2::new(c.x + 8.0, y)],
                    Stroke::new(1.3, color),
                );
            }
        }
        NavIcon::Kanban => {
            let bars: [(f32, f32); 3] = [(-8.0, 12.0), (-2.0, 8.0), (4.0, 12.0)];
            for (x, h) in bars {
                painter.rect_filled(
                    Rect::from_min_size(Pos2::new(c.x + x, c.y - h / 2.0), Vec2::new(4.0, h)),
                    1.5,
                    color,
                );
            }
        }
        NavIcon::Quadrant => {
            for (x, y) in [(-6.0, -6.0), (2.0, -6.0), (-6.0, 2.0), (2.0, 2.0)] {
                painter.rect_filled(
                    Rect::from_min_size(Pos2::new(c.x + x, c.y + y), Vec2::splat(4.0)),
                    1.2,
                    color,
                );
            }
        }
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.on_hover_text(tip)
}

fn title_bar(app: &mut App, ui: &mut egui::Ui) {
    let mut started: Option<(f32, f32)> = None;
    let mut hide = false;
    let mut close = false;
    let mut toggle_notes = false;
    let mut open_settings = false;
    // 拖动热区只能盖住标题条本身。Frame 内的 ui.max_rect 仍是整窗剩余区域，
    // 直接拿它当拖动区会导致在卡片空白处拖拽也整窗跑
    let strip = Rect::from_min_size(ui.cursor().min, Vec2::new(ui.available_width(), 38.0));

    Frame::new()
        .fill(Color32::TRANSPARENT)
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            // 整条标题栏都是拖动区；按钮在它之后分配，会盖在拖动区上面，
            // 所以点按钮不会误拖窗口
            let bg = ui.interact(strip, stable_id("title-drag"), egui::Sense::drag());
            if bg.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::Grab);
            }
            if bg.dragged() {
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
            }
            if bg.drag_started() {
                started = win32::cursor_pos();
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 0.0);
                ui.label(
                    RichText::new("Craboard")
                        .size(fs(13.0))
                        .color(theme::active().dim)
                        .strong(),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if caption_button(ui, CaptionIcon::Close, "关闭").clicked() {
                        close = true;
                    }
                    if caption_button(ui, CaptionIcon::Minimize, "隐藏到托盘").clicked() {
                        hide = true;
                    }
                    if sidebar_toggle(ui, app.st().notes_open).clicked() {
                        toggle_notes = true;
                    }
                    if settings_button(ui).clicked() {
                        open_settings = true;
                    }
                });
            });
        });

    if hide {
        app.set_visible(ui.ctx(), false);
    }
    if close {
        app.request_close(ui.ctx());
    }
    if toggle_notes {
        set_notes_open(app, ui.ctx(), !app.st().notes_open);
    }
    if open_settings {
        app.settings_open = true;
    }

    // winit 的 StartDrag 对垫底的无边框窗口不可靠，直接按指针位移改 OuterPosition
    if let Some(cursor) = started {
        if let Some(outer) = ui.ctx().input(|i| i.viewport().outer_rect) {
            app.move_drag = Some((outer.min, cursor));
        }
    }
    if app.move_drag.is_some() && !ui.input(|i| i.pointer.any_down()) {
        app.move_drag = None;
    }
    if let Some((win, cursor0)) = app.move_drag {
        if let Some(cursor) = win32::cursor_pos() {
            let ppp = ui.ctx().pixels_per_point();
            let delta = egui::vec2((cursor.0 - cursor0.0) / ppp, (cursor.1 - cursor0.1) / ppp);
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::OuterPosition(win + delta));
        }
    }
}

/// egui 0.37 弃用了 Id::new，但拖拽源和缩放手柄的 id 必须跨帧稳定，Id::unique 每帧都不同。
#[allow(deprecated)]
pub(crate) fn stable_id(source: impl std::hash::Hash + std::fmt::Debug) -> egui::Id {
    egui::Id::new(source)
}

/// 纯 Label 默认只有 hover 感知，想让它可点必须显式给 Sense::click()
fn link_label(ui: &mut egui::Ui, text: RichText, tip: &str) -> egui::Response {
    ui.add(egui::Label::new(text).sense(egui::Sense::click()))
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(tip)
}

enum CaptionIcon {
    Minimize,
    Close,
}

/// 标题栏按钮统一 30×24，图标手绘：文本字形在不同字体下宽高差很多，× 和 — 会一边大一边小
fn caption_button(ui: &mut egui::Ui, icon: CaptionIcon, tip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(30.0, 24.0), Sense::click());
    let painter = ui.painter();
    if resp.hovered() {
        painter.rect_filled(rect, 6.0, theme::active().hover);
    }
    let color = if resp.hovered() {
        theme::active().text
    } else {
        theme::active().dim
    };
    let c = rect.center();
    let stroke = Stroke::new(1.2, color);
    match icon {
        CaptionIcon::Minimize => {
            painter.line_segment(
                [Pos2::new(c.x - 5.0, c.y), Pos2::new(c.x + 5.0, c.y)],
                stroke,
            );
        }
        CaptionIcon::Close => {
            let d = 4.5;
            painter.line_segment(
                [Pos2::new(c.x - d, c.y - d), Pos2::new(c.x + d, c.y + d)],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(c.x - d, c.y + d), Pos2::new(c.x + d, c.y - d)],
                stroke,
            );
        }
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.on_hover_text(tip)
}

pub(crate) fn ghost_button(ui: &mut egui::Ui, text: &str, tip: &str) -> egui::Response {
    let btn = egui::Button::new(
        RichText::new(text)
            .size(fs(13.0))
            .color(theme::active().dim),
    )
    .fill(Color32::TRANSPARENT)
    .stroke(Stroke::NONE)
    .corner_radius(6.0);
    ui.add(btn).on_hover_text(tip)
}

fn settings_button(ui: &mut egui::Ui) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(30.0, 24.0), Sense::click());
    let painter = ui.painter();
    if resp.hovered() {
        painter.rect_filled(rect, 6.0, theme::active().hover);
    }
    let color = if resp.hovered() {
        theme::active().text
    } else {
        theme::active().dim
    };
    let c = rect.center();
    for (i, y) in [-5.0, 0.0, 5.0].into_iter().enumerate() {
        painter.line_segment(
            [Pos2::new(c.x - 7.0, c.y + y), Pos2::new(c.x + 7.0, c.y + y)],
            Stroke::new(1.2, color),
        );
        let knob = match i {
            0 => c.x + 2.5,
            1 => c.x - 3.0,
            _ => c.x + 4.0,
        };
        painter.circle_filled(Pos2::new(knob, c.y + y), 1.9, color);
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.on_hover_text("设置")
}

fn sidebar_toggle(ui: &mut egui::Ui, active: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(30.0, 24.0), Sense::click());
    let painter = ui.painter();
    if resp.hovered() {
        painter.rect_filled(rect, 6.0, theme::active().hover);
    }
    let color = if active {
        theme::active().accent
    } else {
        theme::active().dim
    };
    let body = Rect::from_center_size(rect.center(), Vec2::new(17.0, 14.0));
    painter.rect_stroke(body, 2.5, Stroke::new(1.2, color), egui::StrokeKind::Inside);
    let split = body.right() - 5.0;
    painter.line_segment(
        [
            Pos2::new(split, body.top() + 1.8),
            Pos2::new(split, body.bottom() - 1.8),
        ],
        Stroke::new(1.2, color),
    );
    if active {
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(split + 1.5, body.top() + 1.5),
                Pos2::new(body.right() - 1.5, body.bottom() - 1.5),
            ),
            1.5,
            theme::active().accent_fill(0.65),
        );
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.on_hover_text("显示/隐藏时钟与便签边栏")
}

fn toast(app: &mut App, ui: &mut egui::Ui) {
    let Some((msg, at)) = app.toast.as_ref() else {
        return;
    };
    let life = Duration::from_secs(4);
    let elapsed = at.elapsed();
    if elapsed >= life {
        app.toast = None;
        return;
    }
    ui.ctx().request_repaint_after(life - elapsed);
    egui::Area::new(stable_id("toast"))
        .anchor(Align2::CENTER_BOTTOM, Vec2::new(0.0, -48.0))
        .order(egui::Order::Foreground)
        .interactable(false)
        .show(ui.ctx(), |ui| {
            Frame::new()
                .fill(
                    theme::active()
                        .field
                        .lerp_to_gamma(theme::active().red, 0.16),
                )
                .stroke(Stroke::new(
                    1.0,
                    theme::active().tint(theme::active().red, 0.6),
                ))
                .corner_radius(8.0)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_max_width(420.0);
                    ui.label(
                        RichText::new(msg)
                            .size(fs(12.0))
                            .color(theme::active().text),
                    );
                });
        });
}

/// 四条边 + 四个角都能拖。直接按指针位置判断，不注册可被内容抢走的拖动控件，
/// 这样边框 6px 的拖动区不会被标题栏/滚动区/四象限网格盖住。
mod list;
mod quadrant;
mod side;
mod today;
mod window;

use list::list_view;
use quadrant::quadrant_view;
use side::side_panel;
use today::today_progress;
use window::{grip_hint, window_edges};

pub(crate) use list::open_editor;
