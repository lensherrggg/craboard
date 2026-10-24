//! 独立的到期提醒小窗：无边框、置顶，5 秒自动关，失去焦点也关。

use super::fs;
use super::theme;
use crate::win32;
use craboard::core::due_label;
use eframe::egui;
use egui::{
    Align, CursorIcon, Frame, Layout, Margin, RichText, Sense, Stroke, Vec2, ViewportBuilder,
    ViewportId,
};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const TITLE: &str = "Craboard 提醒";
pub fn viewport_id() -> ViewportId {
    ViewportId::from_hash_of("craboard-reminder")
}
const WIDTH: f32 = 340.0;
const HEIGHT: f32 = 188.0;
const SCREEN_MARGIN: f32 = 20.0;

#[derive(Clone)]
pub struct Item {
    pub id: u64,
    pub text: String,
    pub due: chrono::NaiveDateTime,
}

struct State {
    items: Vec<Item>,
    deadline: Instant,
    visible: bool,
    /// 主窗需要在下一帧跑一次完整 UI，才能创建或回收 deferred viewport
    root_paint: bool,
    ever_focused: bool,
    styled: bool,
    /// 鼠标悬停过就取消一切自动关闭，只剩手动关闭
    held: bool,
}

pub struct Reminder {
    state: Arc<Mutex<State>>,
}

impl Reminder {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                items: Vec::new(),
                deadline: Instant::now(),
                visible: false,
                root_paint: false,
                ever_focused: false,
                styled: false,
                held: false,
            })),
        }
    }

    pub fn is_visible(&self) -> bool {
        self.state.lock().map(|s| s.visible).unwrap_or(false)
    }

    pub fn needs_root_paint(&self) -> bool {
        self.state.lock().map(|s| s.root_paint).unwrap_or(false)
    }

    pub fn clear_root_paint(&self) {
        if let Ok(mut s) = self.state.lock() {
            s.root_paint = false;
        }
    }

    pub fn notify(&mut self, items: Vec<Item>, seconds: u64) {
        let Ok(mut st) = self.state.lock() else {
            return;
        };
        if st.visible {
            for item in items {
                if !st.items.iter().any(|old| old.id == item.id) {
                    st.items.push(item);
                }
            }
        } else {
            st.items = items;
            st.visible = true;
            st.ever_focused = false;
            st.styled = false;
            st.held = false;
        }
        st.items.sort_by_key(|i| i.due);
        st.deadline = Instant::now() + Duration::from_secs(seconds.clamp(3, 60));
        st.root_paint = true;
    }

    /// 显示器工作区右下角，物理像素换算回 egui 点
    pub fn origin(&self, hwnd: Option<win32::HWND>, ppp: f32) -> egui::Pos2 {
        let ppp = ppp.max(1.0);
        let (w, h) = (fs(WIDTH), fs(HEIGHT));
        match hwnd.and_then(win32::work_area) {
            Some((_, _, right, bottom)) => egui::pos2(
                right as f32 / ppp - w - SCREEN_MARGIN,
                bottom as f32 / ppp - h - SCREEN_MARGIN,
            ),
            None => egui::pos2(80.0, 80.0),
        }
    }

    pub fn show(&self, ctx: &egui::Context, pos: egui::Pos2) {
        if !self.is_visible() {
            return;
        }
        let state = self.state.clone();
        let size = [fs(WIDTH), fs(HEIGHT)];
        let builder = ViewportBuilder::default()
            .with_title(TITLE)
            .with_app_id("Craboard.Reminder")
            .with_decorations(false)
            .with_taskbar(false)
            .with_has_shadow(true)
            .with_resizable(false)
            .with_always_on_top()
            .with_active(true)
            .with_inner_size(size)
            .with_min_inner_size(size)
            .with_position(pos);
        ctx.show_viewport_deferred(viewport_id(), builder, move |ui, _class| {
            draw(ui, &state);
        });
    }
}

fn draw(ui: &mut egui::Ui, state: &Arc<Mutex<State>>) {
    let now = Instant::now();
    let mut close = false;
    // 先在 input 闭包外取 rect：input 持有 ctx 锁，里面再取会自锁
    let content = ui.ctx().content_rect();
    let hovered = ui
        .ctx()
        .input(|i| i.pointer.hover_pos().is_some_and(|p| content.contains(p)));
    let (items, total, deadline, held) = {
        let Ok(mut st) = state.lock() else {
            return;
        };
        if !st.styled {
            if let Some(h) = win32::find_self(TITLE) {
                win32::style_window(h);
                st.styled = true;
            }
        }
        if ui.ctx().input(|i| i.viewport().close_requested()) {
            close = true;
        }
        let focused = ui.ctx().input(|i| i.viewport().focused);
        if focused == Some(true) {
            st.ever_focused = true;
        }
        // 鼠标一旦停在提醒上就取消自动关闭（含失焦），只留手动关闭
        if hovered {
            st.held = true;
        }
        if !st.held {
            if st.ever_focused && focused == Some(false) {
                close = true;
            }
            if now >= st.deadline {
                close = true;
            }
        }
        if close {
            st.visible = false;
            st.root_paint = true;
        } else if !st.held {
            // 倒计时每秒跳一格：睡到下一个整秒边界，别为了刷新数字空转
            let rem = st.deadline.saturating_duration_since(now);
            let frac = rem.as_secs_f32().fract();
            let tick = Duration::from_secs_f32(if frac > 0.0 { frac } else { 1.0 });
            ui.ctx()
                .request_repaint_after(tick.min(rem) + Duration::from_millis(5));
        }
        (st.items.clone(), st.items.len(), st.deadline, st.held)
    };
    let secs = deadline
        .saturating_duration_since(Instant::now())
        .as_secs_f32()
        .ceil()
        .max(1.0) as u64;
    if close {
        ui.ctx().request_repaint_of(ViewportId::ROOT);
        return;
    }

    let now_local = chrono::Local::now().naive_local();
    let mut manual_close = ui.input(|i| i.key_pressed(egui::Key::Escape));
    egui::CentralPanel::default()
        .frame(
            Frame::new()
                .fill(theme::active().surface)
                .stroke(Stroke::new(1.0, theme::active().line))
                .inner_margin(Margin::symmetric(16, 12)),
        )
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(4.0, 2.0);
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(Vec2::new(10.0, 18.0), Sense::hover());
                ui.painter()
                    .circle_filled(dot.center(), 3.5, theme::active().red);
                ui.label(
                    RichText::new("任务到期")
                        .size(fs(13.5))
                        .color(theme::active().text)
                        .strong(),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if close_button(ui).clicked() {
                        manual_close = true;
                    }
                });
            });
            ui.add_space(5.0);

            let shown = if total > 3 { 2 } else { total.min(3) };
            for item in items.iter().take(shown) {
                ui.add(
                    egui::Label::new(
                        RichText::new(&item.text)
                            .size(fs(12.5))
                            .color(theme::active().text),
                    )
                    .truncate(),
                );
                ui.label(
                    RichText::new(format!("截止 {}", due_label(item.due, now_local)))
                        .size(fs(10.5))
                        .color(theme::active().dim),
                );
                ui.add_space(3.0);
            }
            if total > shown {
                ui.label(
                    RichText::new(format!("另有 {} 个任务同时到期", total - shown))
                        .size(fs(10.5))
                        .color(theme::active().dim),
                );
            }

            if !held {
                ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        ui.label(
                            RichText::new(secs.to_string())
                                .size(fs(10.5))
                                .color(theme::active().accent)
                                .strong(),
                        );
                        ui.label(
                            RichText::new("秒后自动关闭")
                                .size(fs(10.0))
                                .color(theme::active().dim),
                        );
                    });
                });
            }
        });

    if manual_close {
        if let Ok(mut st) = state.lock() {
            st.visible = false;
            st.root_paint = true;
        }
        ui.ctx().request_repaint_of(ViewportId::ROOT);
    }
}

fn close_button(ui: &mut egui::Ui) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(22.0, 22.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 5.0, theme::active().hover);
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let color = if resp.hovered() {
        theme::active().text
    } else {
        theme::active().dim
    };
    let c = rect.center();
    let d = 4.5;
    let stroke = Stroke::new(1.4, color);
    ui.painter()
        .line_segment([c + Vec2::new(-d, -d), c + Vec2::new(d, d)], stroke);
    ui.painter()
        .line_segment([c + Vec2::new(-d, d), c + Vec2::new(d, -d)], stroke);
    resp.on_hover_text("关闭提醒")
}
