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
                    bottom: 8,
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

fn painted_sun(painter: &egui::Painter, c: Pos2, color: Color32) {
    painter.circle_stroke(c, 5.5, Stroke::new(1.4, color));
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::TAU / 8.0;
        let dir = Vec2::new(a.cos(), a.sin());
        painter.line_segment([c + dir * 8.5, c + dir * 10.5], Stroke::new(1.3, color));
    }
}

/// 左下角今日进度。文字/图标两种形态，点击展开今日总结/完成情况。
/// 返回 true 表示弹层里点了「即时总结」，由调用方写回总结正文。
fn today_progress(
    ui: &mut egui::Ui,
    progress: TodayProgress,
    icon_mode: bool,
    st: &Settings,
    exp: Experimental,
) -> bool {
    let done = progress.done;
    let frac = progress.fraction();
    let done_text = format!("已完成 {done} 项任务");

    // 父布局是 top_down，用 bottom_up 先把整块分配到左下角，
    // 块内再显式 top_down，避免卡片内容跟着倒序
    ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
        // 高度必须把 Frame 外边距、行高和 item_spacing 都算进去；
        // 以前固定 50 会短于内容，最后一行会被窗口下沿裁掉
        let h = if icon_mode {
            34.0
        } else {
            let gap = ui.spacing().item_spacing.y;
            let title_h = ui
                .ctx()
                .fonts_mut(|f| f.row_height(&FontId::proportional(fs(11.0))));
            // 大字号下这句会折行，按实际排版高度分配，避免最后一行被卡片裁掉
            let wrap_w = (ui.available_width() - 10.0).max(16.0);
            let small_h = ui.ctx().fonts_mut(|f| {
                f.layout(
                    done_text.clone(),
                    FontId::proportional(fs(8.5)),
                    theme::active().dim,
                    wrap_w,
                )
                .rect
                .height()
            });
            6.0 + title_h + gap + 4.0 + 3.0 + gap + 4.0 + small_h + 6.0
        };
        ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width(), h),
            Layout::top_down(Align::Min),
            |ui| -> bool {
                if icon_mode {
                    let (rect, resp) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::click());
                    progress_ring(ui.painter(), rect.center(), 10.0, frac);
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    }
                    let refresh = today_popup(ui, &resp, progress, st, exp);
                    resp.on_hover_text(if exp.summary() {
                        "今日总结"
                    } else {
                        "今日完成情况"
                    });
                    return refresh;
                }

                let card = Frame::new()
                    .fill(theme::active().surface)
                    .corner_radius(8.0)
                    .inner_margin(egui::Margin::symmetric(5, 6))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("今日进度")
                                    .size(fs(11.0))
                                    .color(theme::active().text)
                                    .strong(),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let (r, _) =
                                    ui.allocate_exact_size(Vec2::new(7.0, 8.0), Sense::hover());
                                let c = r.center();
                                ui.painter().add(Shape::convex_polygon(
                                    vec![
                                        Pos2::new(c.x - 4.0, c.y + 3.0),
                                        Pos2::new(c.x + 4.0, c.y + 3.0),
                                        Pos2::new(c.x, c.y - 3.0),
                                    ],
                                    theme::active().dim,
                                    Stroke::NONE,
                                ));
                            });
                        });
                        ui.add_space(4.0);
                        let (bar, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 3.0),
                            Sense::hover(),
                        );
                        ui.painter().rect_filled(bar, 1.5, theme::active().line);
                        let w = bar.width() * frac;
                        if w > 0.0 {
                            ui.painter().rect_filled(
                                Rect::from_min_size(bar.min, Vec2::new(w, bar.height())),
                                1.5,
                                theme::active().done,
                            );
                        }
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(&done_text)
                                .size(fs(8.5))
                                .color(theme::active().dim),
                        );
                    });
                let resp = ui.interact(
                    card.response.rect,
                    stable_id("today-progress-card"),
                    Sense::click(),
                );
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                today_popup(ui, &resp, progress, st, exp)
            },
        )
        .inner
    })
    .inner
}

/// 今日完成情况；试验功能开启后追加 AI 总结正文与推送状态。
fn today_popup(
    ui: &mut egui::Ui,
    anchor: &egui::Response,
    progress: TodayProgress,
    st: &Settings,
    exp: Experimental,
) -> bool {
    let local = chrono::Local::now();
    let now = local.naive_local();
    let date = format!(
        "{} {}",
        local.format("%Y年%-m月%-d日"),
        weekday_cn(local.weekday())
    );
    let (done, total) = (progress.done, progress.total);
    let pct = progress.percent();
    let remaining = total.saturating_sub(done);
    let summary = exp.summary().then(|| ai_summary_view(st, now));
    let width = if summary.is_some() {
        (ui.ctx().content_rect().width() - 28.0).clamp(220.0, 340.0)
    } else {
        (ui.ctx().content_rect().width() - 28.0).clamp(200.0, 280.0)
    };
    let copy_text = summary
        .as_ref()
        .map(|view| summary_copy_text(view, &date, done, total, pct));
    let mut refresh = false;

    egui::Popup::from_toggle_button_response(anchor)
        .id(stable_id("today-progress-popup"))
        .align(egui::RectAlign::TOP_START)
        .gap(6.0)
        .width(width)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .frame(
            Frame::new()
                .fill(theme::active().surface)
                .stroke(Stroke::new(1.0, theme::active().line))
                .corner_radius(12.0)
                .inner_margin(egui::Margin::same(14)),
        )
        .show(|ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(if summary.is_some() {
                        "今日总结"
                    } else {
                        "今日完成情况"
                    })
                    .size(fs(13.0))
                    .color(theme::active().text)
                    .strong(),
                );
                if summary.is_some() {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        if exp.active() {
                            let btn = ui.add(
                                egui::Button::new(
                                    RichText::new("即时总结")
                                        .size(fs(10.0))
                                        .color(theme::active().accent)
                                        .strong(),
                                )
                                .fill(theme::active().accent_fill(0.12))
                                .stroke(Stroke::new(1.0, theme::active().accent_fill(0.45)))
                                .corner_radius(5.0)
                                .min_size(Vec2::new(0.0, 20.0)),
                            );
                            if btn.hovered() {
                                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                            }
                            if btn.clicked() {
                                refresh = true;
                            }
                        }
                        if let Some(text) = &copy_text {
                            let resp = ui.add(
                                egui::Label::new(
                                    RichText::new("复制")
                                        .size(fs(10.0))
                                        .color(theme::active().dim),
                                )
                                .sense(Sense::click()),
                            );
                            if resp.hovered() {
                                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                            }
                            if resp.clicked() {
                                ui.ctx().copy_text(text.clone());
                            }
                        }
                    });
                }
            });
            ui.add(
                Label::new(
                    RichText::new(date)
                        .size(fs(9.5))
                        .color(theme::active().dim),
                )
                .selectable(true),
            );
            ui.add_space(8.0);
            ui.add(
                Label::new(
                    RichText::new(format!("今日进度 {done}/{total}（{pct}%）"))
                        .size(fs(11.5))
                        .color(theme::active().text),
                )
                .selectable(true),
            );
            ui.add_space(4.0);
            let (bar, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 3.0), Sense::hover());
            ui.painter().rect_filled(bar, 1.5, theme::active().line);
            let w = bar.width() * progress.fraction();
            if w > 0.0 {
                ui.painter().rect_filled(
                    Rect::from_min_size(bar.min, Vec2::new(w, bar.height())),
                    1.5,
                    theme::active().done,
                );
            }
            ui.add_space(8.0);
            let (detail, color) = if total == 0 {
                ("今天还没有任务".to_owned(), theme::active().dim)
            } else if remaining == 0 {
                ("今天的任务已全部完成".to_owned(), theme::active().done)
            } else {
                (
                    format!("已完成 {done} 项 · 未完成 {remaining} 项"),
                    theme::active().dim,
                )
            };
            ui.add(
                Label::new(RichText::new(detail).size(fs(10.5)).color(color))
                    .wrap()
                    .selectable(true),
            );

            let Some(view) = &summary else {
                return;
            };
            ui.add_space(8.0);
            let y = ui.cursor().top() + 2.0;
            ui.painter().hline(
                ui.max_rect().x_range(),
                y,
                Stroke::new(1.0, theme::active().line),
            );
            ui.add_space(8.0);

            if let Some(body) = view.text.as_deref() {
                let title = match view.source {
                    Some(SummarySource::Instant) => "即时总结",
                    _ => "AI 总结",
                };
                ui.add(
                    Label::new(
                        RichText::new(title)
                            .size(fs(10.5))
                            .color(theme::active().accent)
                            .strong(),
                    )
                    .selectable(true),
                );
                let window = match (view.window_start.as_deref(), view.window_end.as_deref()) {
                    (Some(start), Some(end)) => {
                        format!("{} → {}", moment_label(start, now), moment_label(end, now))
                    }
                    _ => "—".to_owned(),
                };
                let meta = if !exp.active() {
                    view.generated_at
                        .as_ref()
                        .map(|g| format!("生成于 {g}"))
                        .unwrap_or_else(|| "等待 AI 助手推送".to_owned())
                } else if !view.enabled {
                    view.generated_at
                        .as_ref()
                        .map(|g| format!("生成于 {g} · 定时总结已关闭"))
                        .unwrap_or_else(|| "定时总结已关闭 · 即时总结仍可用".to_owned())
                } else if view.stale {
                    format!(
                        "{} · 等待 AI 推送",
                        view.next_due_at
                            .as_deref()
                            .map(|due| moment_label(due, now))
                            .unwrap_or_else(|| "已到更新窗口".to_owned())
                    )
                } else if let Some(due) = view.next_due_at.as_deref() {
                    format!("下次更新 {} · 窗口 {window}", moment_label(due, now))
                } else if let Some(g) = view.generated_at.as_deref() {
                    format!("生成于 {g}")
                } else {
                    "等待第一个触发时间点".to_owned()
                };
                let meta_color = if exp.active() && view.stale {
                    theme::active().warn
                } else {
                    theme::active().dim
                };
                ui.add(
                    Label::new(RichText::new(meta).size(fs(9.5)).color(meta_color))
                        .wrap()
                        .selectable(true),
                );
                ui.add_space(3.0);
                // 只展示约 8 行，更长的内容在弹层内滚动，避免撑出屏幕
                ui.spacing_mut().scroll.floating = false;
                let line_h = ui
                    .ctx()
                    .fonts_mut(|f| f.row_height(&FontId::proportional(fs(10.5))));
                ScrollArea::vertical()
                    .id_salt("today-summary-ai-scroll")
                    .max_height(line_h * 8.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.add(
                            Label::new(
                                RichText::new(body)
                                    .size(fs(10.5))
                                    .color(theme::active().dim),
                            )
                            .wrap()
                            .selectable(true),
                        );
                    });
            } else if exp.active() {
                ui.add(
                    Label::new(
                        RichText::new("尚未收到 AI 总结")
                            .size(fs(10.5))
                            .color(theme::active().warn)
                            .strong(),
                    )
                    .selectable(true),
                );
                ui.add_space(3.0);
                ui.add(
                    Label::new(
                        RichText::new(
                            "点「即时总结」现场生成，或等待定时窗口由 AI 助手通过 MCP 推送。",
                        )
                        .size(fs(10.5))
                        .color(theme::active().dim),
                    )
                    .wrap()
                    .selectable(true),
                );
            } else {
                ui.add(
                    Label::new(
                        RichText::new("等待 AI 助手推送")
                            .size(fs(10.5))
                            .color(theme::active().warn)
                            .strong(),
                    )
                    .selectable(true),
                );
                ui.add_space(3.0);
                ui.add(
                    Label::new(
                        RichText::new(
                            "让 agent 调用 MCP 的 set_ai_summary 写入正文；程序不内置模型，也不读取 API Key。",
                        )
                        .size(fs(10.5))
                        .color(theme::active().dim),
                    )
                    .wrap()
                    .selectable(true),
                );
            }
        });
    refresh
}

fn summary_copy_text(
    view: &AiSummaryView,
    date: &str,
    done: usize,
    total: usize,
    pct: usize,
) -> String {
    let mut text = format!("今日总结\n{date}\n今日进度 {done}/{total}（{pct}%）\n\n");
    if let Some(body) = view.text.as_deref() {
        let label = match view.source {
            Some(SummarySource::Instant) => "即时总结",
            _ => "AI 总结",
        };
        text.push_str(&format!(
            "{label} · {}\n{body}\n",
            view.generated_at.as_deref().unwrap_or("—")
        ));
    } else {
        text.push_str("尚未收到 AI 总结。\n");
    }
    text
}

fn progress_ring(painter: &egui::Painter, center: Pos2, radius: f32, frac: f32) {
    painter.circle_stroke(center, radius, Stroke::new(3.0, theme::active().line));
    if frac <= 0.0 {
        return;
    }
    let steps = (frac * 48.0).ceil().max(2.0) as usize;
    let start = -std::f32::consts::FRAC_PI_2;
    let sweep = std::f32::consts::TAU * frac;
    let points: Vec<Pos2> = (0..=steps)
        .map(|i| {
            let a = start + sweep * (i as f32 / steps as f32);
            center + Vec2::new(a.cos(), a.sin()) * radius
        })
        .collect();
    painter.add(Shape::line(points, Stroke::new(3.0, theme::active().done)));
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

fn clock_body(app: &mut App, ui: &mut egui::Ui) {
    let now = chrono::Local::now();
    let fmt = if app.st().show_seconds {
        "%H:%M:%S"
    } else {
        "%H:%M"
    };
    let time = now.format(fmt).to_string();
    let date = now.format("%Y年%-m月%-d日").to_string();
    let week = weekday_cn(now.weekday());
    // 字号跟着卡片实际宽度走，宽窗口里时钟不会缩成一小块
    let size = (ui.available_width() * 0.13).clamp(26.0, 44.0);

    ui.vertical_centered(|ui| {
        ui.label(
            RichText::new(time)
                .size(fs(size))
                .color(theme::active().text)
                .strong(),
        );
        ui.label(
            RichText::new(format!("{date}  {week}"))
                .size(fs(13.0))
                .color(theme::active().dim),
        );
    });
}

fn weekday_cn(d: chrono::Weekday) -> &'static str {
    match d {
        chrono::Weekday::Mon => "星期一",
        chrono::Weekday::Tue => "星期二",
        chrono::Weekday::Wed => "星期三",
        chrono::Weekday::Thu => "星期四",
        chrono::Weekday::Fri => "星期五",
        chrono::Weekday::Sat => "星期六",
        chrono::Weekday::Sun => "星期日",
    }
}

fn list_view(app: &mut App, ui: &mut egui::Ui) {
    let now = chrono::Local::now().naive_local();
    let infos = task_infos(app.st());
    let today_view = app.st().view == View::Today;
    let collapsed: HashSet<u64> = app.st().collapsed_tasks.iter().copied().collect();
    let listing = list_rows(&infos, ListScope::of(app.st().view), app.filter);
    let rows = listing.rows;
    let stats = listing.stats;
    let wip = app.st().wip_limit;
    let mut actions = Vec::new();

    Frame::new()
        .fill(Color32::TRANSPARENT)
        .inner_margin(egui::Margin::symmetric(14, 6))
        .show(ui, |ui| {
            // 主区被两侧栏挤窄时，筛选链接折到第二行，统计行也允许换行
            let filters_inline = ui.available_width() >= 300.0;
            // 极窄主区只留标题和未完成数，进行中/逾期折进悬浮说明，避免整行折行
            let compact_stats = ui.available_width() < 190.0;
            let stats_row = |ui: &mut egui::Ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 0.0);
                let title = if today_view { "今天" } else { "任务" };
                ui.label(
                    RichText::new(title)
                        .size(fs(15.0))
                        .color(theme::active().text)
                        .strong(),
                );
                ui.add_space(2.0);
                let open_resp = ui.label(
                    RichText::new(format!("{} 未完成", stats.open))
                        .size(fs(11.0))
                        .color(theme::active().dim),
                );
                if compact_stats {
                    let mut tip = Vec::new();
                    if stats.doing > 0 {
                        tip.push(format!("进行中 {}/{}", stats.doing, wip));
                    }
                    if stats.overdue > 0 {
                        tip.push(format!("逾期 {}", stats.overdue));
                    }
                    if !tip.is_empty() {
                        open_resp.on_hover_text(tip.join(" · "));
                    }
                    return;
                }
                if stats.doing > 0 {
                    dot(ui);
                    let over = stats.doing > wip;
                    ui.label(
                        RichText::new(format!("进行中 {}/{}", stats.doing, wip))
                            .size(fs(11.0))
                            .color(if over {
                                theme::active().red
                            } else {
                                theme::active().doing
                            }),
                    )
                    .on_hover_text("theme::active().doing 上限在设置里调整");
                }
                if stats.overdue > 0 {
                    dot(ui);
                    ui.label(
                        RichText::new(format!("逾期 {}", stats.overdue))
                            .size(fs(11.0))
                            .color(theme::active().red),
                    );
                }
            };
            if filters_inline {
                ui.horizontal(|ui| {
                    stats_row(ui);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        filter_controls(app, ui);
                    });
                });
            } else {
                ui.horizontal_wrapped(|ui| stats_row(ui));
            }
            if !filters_inline {
                ui.add_space(2.0);
                ui.allocate_ui_with_layout(
                    Vec2::new(ui.available_width(), 18.0),
                    Layout::right_to_left(Align::Center),
                    |ui| filter_controls(app, ui),
                );
            }
            ui.add_space(7.0);
            let hint = if today_view {
                "添加今天的任务，回车确认"
            } else {
                "添加任务，回车确认"
            };
            add_task_input(app, ui, hint);
            ui.add_space(6.0);
            let sep = ui.cursor().top();
            ui.painter().hline(
                ui.max_rect().x_range(),
                sep,
                Stroke::new(1.0, theme::active().line),
            );
            ui.add_space(6.0);

            let wide = ui.available_width() >= fs(330.0);

            ScrollArea::vertical()
                .id_salt("task-list")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if rows.is_empty() {
                        ui.add_space(30.0);
                        ui.vertical_centered(|ui| {
                            if stats.scope_total == 0 && today_view {
                                ui.label(
                                    RichText::new("今天没有任务")
                                        .size(fs(12.5))
                                        .color(theme::active().dim),
                                );
                                ui.add_space(3.0);
                                ui.label(
                                    RichText::new("在上面的输入框添加今天的任务")
                                        .size(fs(11.0))
                                        .color(theme::active().dim),
                                );
                            } else if stats.scope_total == 0 {
                                ui.label(
                                    RichText::new("还没有任务")
                                        .size(fs(12.5))
                                        .color(theme::active().dim),
                                );
                                ui.add_space(3.0);
                                ui.label(
                                    RichText::new("在上面的输入框添加第一条")
                                        .size(fs(11.0))
                                        .color(theme::active().dim),
                                );
                            } else {
                                ui.label(
                                    RichText::new("没有符合筛选的任务")
                                        .size(fs(12.0))
                                        .color(theme::active().dim),
                                );
                            }
                        });
                        return;
                    }
                    for &idx in &rows {
                        let info = &infos[idx];
                        if let Some(action) =
                            task_row(ui, info, wide, now, true, collapsed.contains(&info.id))
                        {
                            actions.push(action);
                        }
                        let y = ui.cursor().top();
                        ui.painter().hline(
                            ui.max_rect().x_range(),
                            y,
                            Stroke::new(
                                1.0,
                                theme::active().bg.lerp_to_gamma(theme::active().line, 0.55),
                            ),
                        );
                        ui.add_space(2.0);
                    }
                });
        });

    apply_actions(app, actions, now);
}

#[derive(Clone, Copy)]
enum RowAction {
    Toggle(u64, TaskStatus),
    ToggleSubtask(u64, usize, bool),
    Edit(u64),
    Postpone(u64, i64),
    ToggleCollapse(u64),
}

fn task_title(app: &App, id: u64) -> String {
    app.core
        .task(id)
        .map(|t| t.text.clone())
        .unwrap_or_else(|_| "任务".to_owned())
}

fn apply_actions(app: &mut App, actions: Vec<RowAction>, now: NaiveDateTime) {
    for action in actions {
        match action {
            RowAction::Toggle(id, target) => match app.core.set_status(id, target) {
                Ok(Some(next)) => {
                    let title = task_title(app, id);
                    app.notify(format!(
                        "完成「{title}」本轮，下次 {}",
                        due_label(next, now)
                    ));
                }
                Ok(None) => {}
                Err(e) => app.notify(e.to_string()),
            },
            RowAction::ToggleSubtask(id, index, done) => {
                if let Err(e) = app.core.set_subtask_done(id, index, done) {
                    app.notify(e.to_string());
                }
            }
            RowAction::Edit(id) => open_editor(app, id),
            RowAction::ToggleCollapse(id) => app.core.toggle_task_collapsed(id),
            RowAction::Postpone(id, days) => {
                let recurring = app.core.task(id).is_ok_and(|t| t.repeat.is_some());
                match app.core.postpone(id, chrono::Duration::days(days)) {
                    Ok(next) => {
                        let prefix = if recurring {
                            "本轮已延至"
                        } else {
                            "已延至"
                        };
                        app.notify(format!("{prefix} {}", due_label(next, now)));
                    }
                    Err(e) => app.notify(e.to_string()),
                }
            }
        }
    }
}

fn quad_color(q: Quadrant) -> Color32 {
    match q {
        Quadrant::ImportantUrgent => theme::active().red,
        Quadrant::Important => theme::active().accent,
        Quadrant::Urgent => theme::active().warn,
        Quadrant::Normal => theme::active().dim,
    }
}

fn quadrant_view(app: &mut App, ui: &mut egui::Ui) {
    let now = chrono::Local::now().naive_local();
    let infos = task_infos(app.st());
    let hours = app.st().urgent_within_hours;
    let open = infos.iter().filter(|i| !i.is_done()).count();
    let mut actions = Vec::new();

    Frame::new()
        .fill(Color32::TRANSPARENT)
        .inner_margin(egui::Margin::symmetric(14, 6))
        .show(ui, |ui| {
            let compact = ui.available_width() < fs(190.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 0.0);
                ui.label(
                    RichText::new("四象限")
                        .size(fs(15.0))
                        .color(theme::active().text)
                        .strong(),
                );
                ui.add_space(2.0);
                ui.label(
                    RichText::new(format!("{open} 未完成"))
                        .size(fs(11.0))
                        .color(theme::active().dim),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let text = if compact {
                        "设置".to_owned()
                    } else {
                        format!("紧急窗口 {hours} 小时")
                    };
                    if link_label(
                        ui,
                        RichText::new(text)
                            .size(fs(10.5))
                            .color(theme::active().dim),
                        "在设置里调整紧急判定窗口",
                    )
                    .clicked()
                    {
                        app.settings_open = true;
                    }
                });
            });
            ui.add_space(7.0);
            add_task_input(app, ui, "添加任务，回车确认");
            ui.add_space(6.0);
            let sep = ui.cursor().top();
            ui.painter().hline(
                ui.max_rect().x_range(),
                sep,
                Stroke::new(1.0, theme::active().line),
            );
            ui.add_space(6.0);

            let avail = ui.available_size();
            let stacked = avail.x < fs(470.0) || avail.y < fs(300.0);
            if stacked {
                ScrollArea::vertical()
                    .id_salt("quadrant-stack")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for q in Quadrant::ALL {
                            quadrant_cell(ui, q, &infos, now, None, &mut actions);
                            ui.add_space(8.0);
                        }
                    });
            } else {
                let gap = 10.0;
                let cell = Vec2::new((avail.x - gap) / 2.0, (avail.y - gap) / 2.0);
                let rows = [
                    [Quadrant::ImportantUrgent, Quadrant::Important],
                    [Quadrant::Urgent, Quadrant::Normal],
                ];
                for (i, row) in rows.into_iter().enumerate() {
                    // 只在两行之间留缝；尾部再留会把第二行推进底边缩放区
                    if i > 0 {
                        ui.add_space(gap);
                    }
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(gap, 0.0);
                        for q in row {
                            quadrant_cell(ui, q, &infos, now, Some(cell), &mut actions);
                        }
                    });
                }
            }
        });

    apply_actions(app, actions, now);
}

fn quadrant_cell(
    ui: &mut egui::Ui,
    q: Quadrant,
    infos: &[TaskInfo],
    now: NaiveDateTime,
    size: Option<Vec2>,
    actions: &mut Vec<RowAction>,
) {
    let tasks = quadrant_tasks(infos, q);
    let count = tasks.len();
    let color = quad_color(q);

    let Some(size) = size else {
        Frame::new()
            .fill(theme::active().column)
            .stroke(Stroke::new(1.0, theme::active().line))
            .corner_radius(12.0)
            .inner_margin(egui::Margin::symmetric(10, 9))
            .outer_margin(egui::Margin::symmetric(7, 0))
            .show(ui, |ui| {
                ui.set_width((ui.available_width() - 14.0).max(80.0));
                quadrant_head(ui, q, count, color);
                flat_task_list(ui, &tasks, now, actions);
            });
        return;
    };

    let (outer, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter()
        .rect_filled(outer, 12.0, theme::active().column);
    ui.painter().rect_stroke(
        outer,
        12.0,
        Stroke::new(1.0, theme::active().line),
        egui::StrokeKind::Inside,
    );
    let inner = outer.shrink2(Vec2::new(10.0, 9.0));
    let mut content = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::top_down(Align::Min)),
    );
    content.set_clip_rect(ui.clip_rect().intersect(outer));
    content.spacing_mut().item_spacing = Vec2::new(5.0, 3.0);
    quadrant_head(&mut content, q, count, color);
    let body_h = content.available_height().max(40.0);
    ScrollArea::vertical()
        .id_salt(("quadrant-body", q.label()))
        .auto_shrink([false, false])
        .max_height(body_h)
        .show(&mut content, |ui| {
            flat_task_list(ui, &tasks, now, actions);
        });
}

fn quadrant_head(ui: &mut egui::Ui, q: Quadrant, count: usize, color: Color32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(5.0, 0.0);
        ui.label(
            RichText::new(q.label())
                .size(fs(11.5))
                .color(color)
                .strong(),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(count.to_string())
                    .size(fs(11.0))
                    .color(theme::active().dim),
            );
        });
    });
    ui.add_space(2.0);
}

fn flat_task_list(
    ui: &mut egui::Ui,
    tasks: &[&TaskInfo],
    now: NaiveDateTime,
    actions: &mut Vec<RowAction>,
) {
    if tasks.is_empty() {
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("没有任务")
                    .size(fs(10.5))
                    .color(theme::active().dim),
            );
        });
        return;
    }
    let wide = ui.available_width() >= fs(300.0);
    for info in tasks {
        if let Some(action) = task_row(ui, info, wide, now, false, true) {
            actions.push(action);
        }
    }
}

fn add_task_input(app: &mut App, ui: &mut egui::Ui, hint: &str) {
    let mut new_task = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let more_w = fs(70.0);
        let edit_w = (ui.available_width() - more_w - 6.0).max(60.0);
        let resp = ui.add_sized(
            [edit_w, 24.0],
            TextEdit::singleline(&mut app.todo_draft)
                .hint_text(hint)
                .text_color(theme::active().text),
        );
        if app.focus_todo {
            resp.request_focus();
            app.focus_todo = false;
        }
        let more = ghost_button(ui, "＋ 新建", "完整新建：截止、子项、前置、周期");
        if more.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        new_task = more.clicked();
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
        if !new_task
            && (resp.lost_focus() || (resp.has_focus() && enter))
            && !app.todo_draft.trim().is_empty()
        {
            let text = std::mem::take(&mut app.todo_draft);
            if let Err(e) = app.core.add_text(text.trim()) {
                app.notify(e.to_string());
            }
            // 连续录入：回车后输入框保持焦点
            resp.request_focus();
        }
    });
    if new_task {
        let text = std::mem::take(&mut app.todo_draft);
        app.editor = Some(kanban::TaskEditor::new_task(text));
    }
}

fn filter_controls(app: &mut App, ui: &mut egui::Ui) {
    if filter_link(
        ui,
        "全部",
        app.filter == TodoFilter::All,
        "显示已完成的任务",
    )
    .clicked()
    {
        app.filter = TodoFilter::All;
    }
    ui.label(
        RichText::new("/")
            .size(fs(11.0))
            .color(theme::active().line),
    );
    if filter_link(
        ui,
        "未完成",
        app.filter == TodoFilter::Active,
        "只看还没做完的任务",
    )
    .clicked()
    {
        app.filter = TodoFilter::Active;
    }
}

fn filter_link(ui: &mut egui::Ui, text: &str, active: bool, tip: &str) -> egui::Response {
    link_label(
        ui,
        RichText::new(text)
            .size(fs(11.5))
            .color(if active {
                theme::active().accent
            } else {
                theme::active().dim
            })
            .strong(),
        tip,
    )
}

fn dot(ui: &mut egui::Ui) {
    ui.label(
        RichText::new("·")
            .size(fs(11.0))
            .color(theme::active().line),
    );
}

/// 列表行：勾选、状态、标题、截止；子项直接挂在任务下面，可逐条勾选。
/// 窗口窄到放不下时状态和截止也折到第二行，避免主行被挤爆
fn task_row(
    ui: &mut egui::Ui,
    info: &TaskInfo,
    wide: bool,
    now: NaiveDateTime,
    checklist: bool,
    collapsed: bool,
) -> Option<RowAction> {
    let id = info.id;
    let done = info.is_done();
    let overdue = info.overdue;
    let recurring = info.repeat.is_some();
    let subtask_total = info.subtask_total();
    let subtask_done = info.subtask_done();
    let mut checked = done;
    let mut action = None;
    let wide_due = info.due.map(|due| {
        let label = due_label(due, now);
        if overdue {
            (format!("逾期 {label}"), theme::active().red)
        } else if due.date() == now.date() {
            (label, theme::active().accent)
        } else {
            (label, theme::active().dim)
        }
    });

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 0.0);
        if checklist {
            if subtask_total > 0 {
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(12.0, 20.0), Sense::click());
                let color = if resp.hovered() {
                    theme::active().text
                } else {
                    theme::active().dim
                };
                let c = rect.center();
                let pts = if collapsed {
                    vec![
                        Pos2::new(c.x - 2.5, c.y - 4.5),
                        Pos2::new(c.x + 3.5, c.y),
                        Pos2::new(c.x - 2.5, c.y + 4.5),
                    ]
                } else {
                    vec![
                        Pos2::new(c.x - 4.5, c.y - 2.0),
                        Pos2::new(c.x + 4.5, c.y - 2.0),
                        Pos2::new(c.x, c.y + 3.5),
                    ]
                };
                ui.painter()
                    .add(Shape::convex_polygon(pts, color, Stroke::NONE));
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                if resp.clicked() {
                    action = Some(RowAction::ToggleCollapse(id));
                }
                resp.on_hover_text(if collapsed {
                    "展开子项"
                } else {
                    "收起子项"
                });
            } else {
                ui.add_space(12.0);
            }
        }
        let check = ui.add(egui::Checkbox::without_text(&mut checked));
        if check.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if check.changed() {
            action = Some(RowAction::Toggle(
                id,
                if checked {
                    TaskStatus::Done
                } else {
                    TaskStatus::Todo
                },
            ));
        }
        if wide {
            status_pill(ui, info);
        }
        if info.important {
            ui.label(
                RichText::new("重要")
                    .size(fs(9.5))
                    .color(theme::active().accent)
                    .strong(),
            );
        }
        // 按实际字宽留位，不能写死 96：中/大字号或长日期会把时间挤成第二行
        let due_w = if wide {
            wide_due.as_ref().map_or(0.0, |(text, _)| {
                ui.painter()
                    .layout_no_wrap(
                        text.clone(),
                        FontId::proportional(fs(10.5)),
                        theme::active().dim,
                    )
                    .rect
                    .width()
                    + 2.0
            })
        } else {
            0.0
        };
        let title_w = (ui.available_width() - due_w - 28.0).max(40.0);
        let mut rt = RichText::new(&info.text).size(fs(13.0));
        rt = if done {
            rt.color(theme::active().dim).strikethrough()
        } else {
            rt.color(theme::active().text)
        };
        // 让最上层的 Label 自己感知点击，并用显式左对齐的子布局包住；
        // 先分配可点矩形再往上盖 Label，点击会被盖住
        let title = ui
            .allocate_ui_with_layout(
                Vec2::new(title_w, 22.0),
                Layout::left_to_right(Align::Center),
                |ui| ui.add(egui::Label::new(rt).truncate().sense(Sense::click())),
            )
            .inner;
        if title.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if title.clicked() {
            action = Some(RowAction::Edit(id));
        }
        title.on_hover_text("点击编辑：时间、状态、子项、前置");
        if wide {
            if let Some((text, color)) = wide_due {
                ui.allocate_ui_with_layout(
                    Vec2::new(due_w, 22.0),
                    Layout::right_to_left(Align::Center),
                    |ui| {
                        let resp = ui.add(
                            egui::Label::new(RichText::new(text).size(fs(10.5)).color(color))
                                .truncate()
                                .sense(Sense::click()),
                        );
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                        }
                        if let Some(a) = due_menu(&resp, id, recurring) {
                            action = Some(a);
                        }
                    },
                );
            }
        }
    });

    let has_meta = subtask_total > 0
        || info.blocked_left > 0
        || info.repeat.is_some()
        || (!wide && (info.status != TaskStatus::Todo || info.due.is_some()));
    if !has_meta {
        ui.add_space(3.0);
        return action;
    }

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(5.0, 2.0);
        let indent = (if checklist { 18.0 } else { 0.0 }) + 24.0 + if wide { 52.0 } else { 0.0 };
        ui.add_space(indent);
        let mut first = true;
        if !wide {
            let (text, color) = match info.status {
                TaskStatus::Done => ("已完成", theme::active().done),
                TaskStatus::Doing => ("进行中", theme::active().doing),
                TaskStatus::Todo if overdue => ("逾期", theme::active().red),
                TaskStatus::Todo => ("待办", theme::active().dim),
            };
            meta_bit(ui, &mut first, text, color);
            if let Some(due) = info.due {
                let text = format!("截止 {}", due_label(due, now));
                let color = if overdue {
                    theme::active().red
                } else {
                    theme::active().dim
                };
                if let Some(a) = meta_due_action(ui, &mut first, text, color, id, recurring) {
                    action = Some(a);
                }
            }
        }
        if let Some(repeat) = info.repeat {
            meta_bit(ui, &mut first, repeat.label(), theme::active().doing);
        }
        if subtask_total > 0 {
            meta_bit(
                ui,
                &mut first,
                format!("子项 {subtask_done}/{subtask_total}"),
                if subtask_done == subtask_total {
                    theme::active().done
                } else {
                    theme::active().dim
                },
            );
            progress_bar(ui, subtask_done, subtask_total);
        }
        if info.blocked_left > 0 {
            meta_bit(
                ui,
                &mut first,
                format!("前置未完成 {}", info.blocked_left),
                theme::active().warn,
            );
        }
    });

    if checklist && !collapsed && subtask_total > 0 {
        let indent = (if wide { 52.0 } else { 0.0 }) + 42.0;
        for (i, sub) in info.subtasks.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 0.0);
                ui.add_space(indent);
                let mut on = sub.done;
                let cb = ui.add_enabled(!done, egui::Checkbox::without_text(&mut on));
                if cb.hovered() && !done {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                if cb.changed() {
                    action = Some(RowAction::ToggleSubtask(id, i, on));
                }
                let mut rt = RichText::new(&sub.text).size(fs(12.0));
                rt = if sub.done {
                    rt.color(theme::active().dim).strikethrough()
                } else {
                    rt.color(theme::active().text)
                };
                let label = ui.add(egui::Label::new(rt).truncate().sense(Sense::click()));
                if !done && label.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                if !done && label.clicked() {
                    action = Some(RowAction::ToggleSubtask(id, i, !sub.done));
                }
            });
            ui.add_space(1.0);
        }
    }
    ui.add_space(3.0);
    action
}

fn meta_due_action(
    ui: &mut egui::Ui,
    first: &mut bool,
    text: String,
    color: Color32,
    id: u64,
    recurring: bool,
) -> Option<RowAction> {
    let resp = meta_label(ui, first, &text, color, Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    due_menu(&resp, id, recurring)
}

/// 点截止时间弹出延期菜单，不额外占一行
fn due_menu(resp: &egui::Response, id: u64, recurring: bool) -> Option<RowAction> {
    let mut action = None;
    egui::Popup::menu(resp).show(|ui| {
        ui.set_min_width(132.0);
        if recurring {
            ui.label(
                RichText::new("周期任务只改本轮")
                    .size(fs(10.0))
                    .color(theme::active().dim),
            );
        }
        if ui.button("顺延 1 天").clicked() {
            action = Some(RowAction::Postpone(id, 1));
            ui.close();
        }
        if ui.button("顺延 3 天").clicked() {
            action = Some(RowAction::Postpone(id, 3));
            ui.close();
        }
        if ui.button("顺延 1 周").clicked() {
            action = Some(RowAction::Postpone(id, 7));
            ui.close();
        }
        ui.separator();
        if ui.button("编辑任务…").clicked() {
            action = Some(RowAction::Edit(id));
            ui.close();
        }
    });
    action
}

fn meta_bit(ui: &mut egui::Ui, first: &mut bool, text: impl Into<String>, color: Color32) {
    meta_label(ui, first, &text.into(), color, Sense::hover());
}

/// 分隔点和值放进同一个 Label，换行时整块移动，不会出现「今天」和「18:00」分家。
fn meta_label(
    ui: &mut egui::Ui,
    first: &mut bool,
    text: &str,
    color: Color32,
    sense: Sense,
) -> egui::Response {
    let font = FontId::proportional(fs(10.0));
    let mut job = egui::text::LayoutJob::default();
    if !*first {
        job.append(
            "· ",
            0.0,
            egui::TextFormat {
                font_id: font.clone(),
                color: theme::active().line,
                ..Default::default()
            },
        );
    }
    *first = false;
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: font,
            color,
            ..Default::default()
        },
    );
    ui.add(Label::new(job).extend().sense(sense))
}

fn progress_bar(ui: &mut egui::Ui, done: usize, total: usize) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(32.0, 3.0), Sense::hover());
    ui.painter().rect_filled(rect, 1.5, theme::active().line);
    let w = rect.width() * (done as f32 / total.max(1) as f32);
    if w > 0.0 {
        ui.painter().rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(w, rect.height())),
            1.5,
            if done == total {
                theme::active().done
            } else {
                theme::active().doing
            },
        );
    }
}

/// 状态用低透明度底色的小药丸表示，比纯文字更容易扫读；
/// 逾期的待办直接显示成红色「逾期」，和看板 OVERDUE 列保持一致
fn status_pill(ui: &mut egui::Ui, info: &TaskInfo) {
    let (text, color) = match info.status {
        TaskStatus::Done => ("已完成", theme::active().done),
        TaskStatus::Doing => ("进行中", theme::active().doing),
        TaskStatus::Todo if info.overdue => ("逾期", theme::active().red),
        TaskStatus::Todo => ("待办", theme::active().dim),
    };
    let (rect, _) = ui.allocate_exact_size(Vec2::new(46.0, 17.0), Sense::hover());
    ui.painter().rect_filled(
        rect,
        8.5,
        Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 26),
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(fs(10.0)),
        color,
    );
}

pub(crate) fn open_editor(app: &mut App, id: u64) {
    app.editor = app.core.task(id).ok().map(kanban::TaskEditor::new);
}

fn side_panel(app: &mut App, ui: &mut egui::Ui) {
    let win_w = ui.max_rect().width();
    let width = notes_panel_width(win_w, app.st().notes_width);

    let resp = egui::Panel::right("notes")
        .resizable(false)
        .show_separator_line(false)
        .exact_size(width)
        .frame(
            Frame::new()
                .fill(theme::active().side)
                .inner_margin(egui::Margin::symmetric(12, 8)),
        )
        .show(ui, |ui| {
            side_body(app, ui);
        });

    // 自绘分隔条：Panel 自带缩放柄还没开始拖动就会改宽度，没法按帧回写持久化
    let panel = resp.response.rect;
    let x = panel.left();
    let strip = Rect::from_min_max(
        Pos2::new(x - 5.0, panel.top()),
        Pos2::new(x + 5.0, panel.bottom()),
    );
    let handle = ui.interact(strip, stable_id("notes-splitter"), Sense::drag());
    if handle.hovered() || handle.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
    if handle.drag_started() {
        let win = ui
            .ctx()
            .input(|i| i.viewport().inner_rect.map(|r| r.size()))
            .unwrap_or(egui::vec2(win_w, 0.0));
        app.notes_drag_start = Some((app.st().notes_width, win));
    }
    if handle.dragged() {
        if let Some((start_w, start_win)) = app.notes_drag_start {
            let w = (start_w - handle.drag_delta().x).clamp(NOTES_MIN, NOTES_MAX);
            if (w - app.st().notes_width).abs() > 0.5 {
                app.core.set_notes_width(w);
                let new_w = (start_win.x + (w - start_w)).max(MIN_W);
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                        new_w,
                        start_win.y,
                    )));
            }
        }
    }
    if !ui.input(|i| i.pointer.any_down()) {
        app.notes_drag_start = None;
    }
    let line = if handle.hovered() || handle.dragged() {
        theme::active().accent
    } else {
        theme::active().line
    };
    ui.painter()
        .vline(x, panel.y_range(), Stroke::new(1.0, line));
}

fn side_body(app: &mut App, ui: &mut egui::Ui) {
    let mut close = false;
    let mut add = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 0.0);
        ui.label(
            RichText::new("时钟")
                .size(fs(13.0))
                .color(theme::active().text)
                .strong(),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ghost_button(ui, "×", "收起边栏").clicked() {
                close = true;
            }
        });
    });
    if close {
        set_notes_open(app, ui.ctx(), false);
        return;
    }
    ui.add_space(5.0);

    Frame::new()
        .fill(theme::active().surface)
        .corner_radius(12.0)
        .stroke(Stroke::new(1.0, theme::active().line))
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            clock_body(app, ui);
        });
    ui.add_space(10.0);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 0.0);
        ui.label(
            RichText::new("便签")
                .size(fs(13.0))
                .color(theme::active().text)
                .strong(),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if link_label(
                ui,
                RichText::new("+ 新建")
                    .size(fs(11.0))
                    .color(theme::active().dim),
                "新建便签",
            )
            .clicked()
            {
                add = true;
            }
        });
    });
    if add {
        app.focus_note = Some(app.core.add_note());
    }
    ui.add_space(5.0);

    if app.st().notes.is_empty() {
        ui.add_space(12.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("没有便签")
                    .size(fs(12.0))
                    .color(theme::active().dim),
            );
            if link_label(
                ui,
                RichText::new("新建一张")
                    .size(fs(12.0))
                    .color(theme::active().accent),
                "新建便签",
            )
            .clicked()
            {
                app.focus_note = Some(app.core.add_note());
            }
        });
        return;
    }

    // 便签正文是唯一需要可写缓冲的地方：本帧克隆出来编辑，改完立即经 Core 提交
    let mut notes: Vec<(u64, String)> = app
        .st()
        .notes
        .iter()
        .map(|n| (n.id, n.text.clone()))
        .collect();
    let total = notes.len();
    let rows = if total == 1 {
        ((ui.available_height() - 52.0) / 19.0).clamp(5.0, 40.0) as usize
    } else {
        7
    };
    let mut focus = app.focus_note.take();
    let mut changed: Vec<u64> = Vec::new();
    let mut remove = None;

    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (i, (id, text)) in notes.iter_mut().enumerate() {
                ui.push_id(*id, |ui| {
                    ui.horizontal(|ui| {
                        let tag = if total > 1 {
                            format!("便签 {}", i + 1)
                        } else {
                            "自动保存".to_owned()
                        };
                        ui.label(RichText::new(tag).size(fs(10.5)).color(theme::active().dim));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ghost_button(ui, "×", "删除这张便签").clicked() {
                                remove = Some(*id);
                            }
                        });
                    });
                    let framed = Frame::new()
                        .fill(theme::active().surface)
                        .corner_radius(10.0)
                        .stroke(Stroke::new(1.0, theme::active().line))
                        .inner_margin(egui::Margin::symmetric(10, 8))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.add(
                                TextEdit::multiline(text)
                                    .desired_rows(rows)
                                    .desired_width(f32::INFINITY)
                                    .frame(Frame::NONE)
                                    .hint_text("随手记点什么")
                                    .text_color(theme::active().text),
                            )
                        });
                    let resp = framed.inner;
                    if resp.changed() {
                        changed.push(*id);
                    }
                    if focus == Some(*id) {
                        resp.request_focus();
                        ui.scroll_to_rect(framed.response.rect, Some(Align::BOTTOM));
                        focus = None;
                    }
                    ui.add_space(10.0);
                });
            }
        });

    app.focus_note = focus;
    for id in changed {
        if let Some((_, text)) = notes.iter().find(|(nid, _)| *nid == id) {
            app.core.update_note(id, text);
        }
    }
    if let Some(id) = remove {
        app.core.remove_note(id);
    }
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
fn window_edges(app: &mut App, ui: &mut egui::Ui) {
    // 不能用 ui.max_rect()：视图内容（如四象限的底部内边距）会把根 Ui 撑大，
    // 热区跟着跑到窗口外；也不能用 viewport().inner_rect（屏幕坐标）。
    let full = ui.ctx().viewport_rect();
    let (l, t, r, b) = (full.left(), full.top(), full.right(), full.bottom());
    let handles = [
        (
            ResizeDir::new(true, false, true, false),
            Rect::from_min_max(Pos2::new(l, t), Pos2::new(l + CORNER, t + CORNER)),
        ),
        (
            ResizeDir::new(false, true, true, false),
            Rect::from_min_max(Pos2::new(r - CORNER, t), Pos2::new(r, t + CORNER)),
        ),
        (
            ResizeDir::new(true, false, false, true),
            Rect::from_min_max(Pos2::new(l, b - CORNER), Pos2::new(l + CORNER, b)),
        ),
        (
            ResizeDir::new(false, true, false, true),
            Rect::from_min_max(Pos2::new(r - CORNER, b - CORNER), Pos2::new(r, b)),
        ),
        (
            ResizeDir::new(true, false, false, false),
            Rect::from_min_max(Pos2::new(l, t + CORNER), Pos2::new(l + EDGE, b - CORNER)),
        ),
        (
            ResizeDir::new(false, true, false, false),
            Rect::from_min_max(Pos2::new(r - EDGE, t + CORNER), Pos2::new(r, b - CORNER)),
        ),
        (
            ResizeDir::new(false, false, true, false),
            Rect::from_min_max(Pos2::new(l + CORNER, t), Pos2::new(r - CORNER, t + EDGE)),
        ),
        (
            ResizeDir::new(false, false, false, true),
            Rect::from_min_max(Pos2::new(l + CORNER, b - EDGE), Pos2::new(r - CORNER, b)),
        ),
    ];

    let hovered = ui
        .ctx()
        .input(|i| i.pointer.hover_pos())
        .and_then(|p| handles.iter().find(|(_, rect)| rect.contains(p)))
        .map(|(dir, _)| *dir);
    // 四象限的网格会盖住底边，不能依赖控件的 drag_started；直接按指针位置启动缩放。
    let pressed = ui
        .ctx()
        .input(|i| i.pointer.button_pressed(egui::PointerButton::Primary));
    if app.resize_drag.is_none() && pressed {
        if let (Some(dir), Some(cursor)) = (hovered, win32::cursor_pos()) {
            if let Some(o) = ui.ctx().input(|i| i.viewport().outer_rect) {
                app.resize_drag = Some(ResizeDrag {
                    dir,
                    outer: o.min,
                    size: full.size(),
                    cursor,
                    ppp: ui.ctx().pixels_per_point(),
                });
                app.move_drag = None;
            }
        }
    }
    if app.resize_drag.is_some() && !ui.input(|i| i.pointer.any_down()) {
        app.resize_drag = None;
    }

    if let Some(rs) = app.resize_drag {
        ui.ctx().set_cursor_icon(rs.dir.cursor());
        app.move_drag = None;
        let Some(cursor) = win32::cursor_pos() else {
            return;
        };
        let d = egui::vec2(
            (cursor.0 - rs.cursor.0) / rs.ppp,
            (cursor.1 - rs.cursor.1) / rs.ppp,
        );
        let mut pos = rs.outer;
        let mut size = rs.size;
        if rs.dir.left {
            let w = (rs.size.x - d.x).max(MIN_W);
            pos.x = rs.outer.x + rs.size.x - w;
            size.x = w;
        }
        if rs.dir.right {
            size.x = (rs.size.x + d.x).max(MIN_W);
        }
        if rs.dir.top {
            let h = (rs.size.y - d.y).max(MIN_H);
            pos.y = rs.outer.y + rs.size.y - h;
            size.y = h;
        }
        if rs.dir.bottom {
            size.y = (rs.size.y + d.y).max(MIN_H);
        }
        let ctx = ui.ctx();
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        if rs.dir.left || rs.dir.top {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
        }
    } else if let Some(dir) = hovered {
        ui.ctx().set_cursor_icon(dir.cursor());
    }
}

fn grip_hint(ui: &mut egui::Ui) {
    let corner = ui.ctx().viewport_rect().max;
    ui.painter().add(Shape::convex_polygon(
        vec![
            corner - Vec2::new(4.0, 13.0),
            corner - Vec2::new(13.0, 4.0),
            corner - Vec2::new(4.0, 4.0),
        ],
        theme::active().line,
        Stroke::NONE,
    ));
}
