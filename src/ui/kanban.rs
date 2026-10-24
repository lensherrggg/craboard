use super::board::{ghost_button, open_editor, stable_id};
use super::{fs, theme};
use crate::App;
use chrono::{Datelike, Timelike};
use craboard::core::{
    KanbanColumn as KCol, TaskEditDraft, TaskInfo, column_tasks, due_label, task_infos,
};
use craboard::state::{
    Repeat, RepeatUnit, Subtask, TaskStatus, TodoItem, normalize_due, parse_clock, parse_due,
};
use eframe::egui;
use egui::{
    Align, Align2, Color32, CursorIcon, FontId, Frame, Label, Layout, Margin, Rect, RichText,
    ScrollArea, Sense, Stroke, StrokeKind, TextEdit, Vec2,
};

const GAP: f32 = 10.0;

fn col_color(col: KCol) -> Color32 {
    match col {
        KCol::Todo => theme::active().dim,
        KCol::Doing => theme::active().doing,
        KCol::Done => theme::active().done,
        KCol::Overdue => theme::active().red,
    }
}

pub struct TaskEditor {
    pub draft: TaskEditDraft,
    pub error: Option<String>,
    creating: bool,
    due_date: String,
    due_hour: u32,
    due_minute: u32,
    due_has_time: bool,
}

impl TaskEditor {
    pub fn new(t: &TodoItem) -> Self {
        let mut editor = Self {
            draft: TaskEditDraft::from_task(t),
            error: None,
            creating: false,
            due_date: String::new(),
            due_hour: 18,
            due_minute: 0,
            due_has_time: false,
        };
        editor.sync_due_from_draft();
        editor
    }

    /// 完整新建：从快速添加的输入内容起步，其余字段在弹窗里补齐。
    pub fn new_task(text: String) -> Self {
        let mut editor = Self {
            draft: TaskEditDraft::new_task(text),
            error: None,
            creating: true,
            due_date: String::new(),
            due_hour: 18,
            due_minute: 0,
            due_has_time: false,
        };
        editor.sync_due_from_draft();
        editor
    }

    /// 快捷延期会直接改 draft.due，改完把结构化控件同步过来。
    fn sync_due_from_draft(&mut self) {
        let raw = self.draft.due.trim();
        let mut parts = raw.split_whitespace();
        self.due_date = parts.next().unwrap_or_default().to_owned();
        if let Some(time) = parts.next().and_then(parse_clock) {
            self.due_has_time = true;
            self.due_hour = time.hour();
            self.due_minute = time.minute();
        } else {
            self.due_has_time = false;
        }
    }

    fn rebuild_due(&mut self) {
        let date = self.due_date.trim();
        if date.is_empty() {
            self.draft.due.clear();
        } else if self.due_has_time {
            self.draft.due = format!("{date} {:02}:{:02}", self.due_hour, self.due_minute);
        } else {
            self.draft.due = date.to_owned();
        }
    }

    /// 从日期框里粘贴完整时间时，把时间拆到时间控件里，日期框只留日期。
    fn split_pasted_time(&mut self) -> bool {
        if !self.due_date.chars().any(char::is_whitespace) {
            return false;
        }
        let mut parts = self.due_date.split_whitespace();
        let date = parts.next().unwrap_or_default().to_owned();
        let time = parts.next().and_then(parse_clock);
        self.due_date = date;
        if let Some(time) = time {
            self.due_has_time = true;
            self.due_hour = time.hour();
            self.due_minute = time.minute();
        }
        true
    }
}

pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    let now = chrono::Local::now().naive_local();
    let infos = task_infos(app.st());
    let avail = ui.available_size();
    let col_w = ((avail.x - GAP * 3.0 - 24.0) / 4.0).clamp(230.0, 340.0);
    let col_h = (avail.y - 16.0).max(140.0);
    let mut rects: Vec<(KCol, Rect)> = Vec::with_capacity(4);
    let mut edit_id = None;

    // 主区连两列都放不下时，横向滚动只剩一条窄缝，改成纵向堆叠
    let stacked = avail.x < 480.0;
    if stacked {
        ScrollArea::vertical()
            .id_salt("kanban-stack")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let w = ui.available_width();
                for col in KCol::ALL {
                    let rect = column(app, ui, col, &infos, now, w, None, &mut edit_id);
                    rects.push((col, rect));
                }
            });
    } else {
        ScrollArea::horizontal()
            .id_salt("kanban-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(GAP, 0.0);
                    ui.add_space(4.0);
                    for col in KCol::ALL {
                        let rect =
                            column(app, ui, col, &infos, now, col_w, Some(col_h), &mut edit_id);
                        rects.push((col, rect));
                    }
                });
            });
    }

    let pointer = ui.ctx().pointer_interact_pos();
    if app.drag_src.is_some() {
        if let Some(pos) = pointer {
            if let Some((_, r)) = rects.iter().find(|(_, r)| r.contains(pos)) {
                ui.painter().rect_stroke(
                    *r,
                    12.0,
                    Stroke::new(1.5, theme::active().accent),
                    StrokeKind::Inside,
                );
            }
            let text = app
                .drag_src
                .and_then(|id| infos.iter().find(|i| i.id == id))
                .map(|i| short(&i.text, 24))
                .unwrap_or_default();
            let painter = ui.ctx().layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                stable_id("kanban-ghost"),
            ));
            let rect = Rect::from_min_size(pos + Vec2::new(14.0, 12.0), Vec2::new(190.0, 30.0));
            painter.rect_filled(
                rect,
                8.0,
                theme::active().tint(theme::active().hover, 238.0 / 255.0),
            );
            painter.rect_stroke(
                rect,
                8.0,
                Stroke::new(1.0, theme::active().accent),
                StrokeKind::Inside,
            );
            painter.text(
                rect.left_center() + Vec2::new(9.0, 0.0),
                Align2::LEFT_CENTER,
                text,
                FontId::proportional(fs(12.0)),
                theme::active().text,
            );
        }
    }

    if let Some(id) = app.drag_src {
        if ui.input(|i| i.pointer.any_released()) {
            if let Some(col) =
                pointer.and_then(|p| rects.iter().find(|(_, r)| r.contains(p)).map(|(c, _)| *c))
            {
                drop_task(app, id, col, now);
            }
            app.drag_src = None;
        }
    }
    if !ui.input(|i| i.pointer.any_down()) {
        app.drag_src = None;
    }

    if let Some(id) = edit_id {
        open_editor(app, id);
    }
}

/// `h` 为 None 时是窄窗堆叠模式：列高随内容自适应，滚动交给外层
fn column(
    app: &mut App,
    ui: &mut egui::Ui,
    col: KCol,
    infos: &[TaskInfo],
    now: chrono::NaiveDateTime,
    w: f32,
    h: Option<f32>,
    edit_id: &mut Option<u64>,
) -> Rect {
    let tasks = column_tasks(infos, col);
    let count = tasks.len();

    if let Some(h) = h {
        let (outer, _) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
        ui.painter()
            .rect_filled(outer, 12.0, theme::active().column);
        ui.painter().rect_stroke(
            outer,
            12.0,
            Stroke::new(1.0, theme::active().line),
            StrokeKind::Inside,
        );

        let inner = outer.shrink2(Vec2::new(10.0, 9.0));
        let mut content = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(Layout::top_down(Align::Min)),
        );
        content.set_clip_rect(ui.clip_rect().intersect(outer));
        content.spacing_mut().item_spacing = Vec2::new(6.0, 5.0);

        column_head(app, &mut content, col, count, false, false);
        if col == KCol::Todo {
            quick_add(app, &mut content);
        }

        let body_h = content.available_height().max(40.0);
        ScrollArea::vertical()
            .id_salt(("kanban-body", col.title()))
            .auto_shrink([false, false])
            .max_height(body_h)
            .show(&mut content, |ui| {
                task_list(app, ui, col, &tasks, now, edit_id);
            });
        return outer;
    }

    let collapsed = app.st().kanban_collapsed[col.index()];
    let card = Frame::new()
        .fill(theme::active().column)
        .stroke(Stroke::new(1.0, theme::active().line))
        .corner_radius(12.0)
        .inner_margin(Margin::symmetric(10, 9))
        .outer_margin(Margin::symmetric(7, 0))
        .show(ui, |ui| {
            ui.set_width((w - 34.0).max(80.0));
            ui.spacing_mut().item_spacing = Vec2::new(6.0, 5.0);
            column_head(app, ui, col, count, true, collapsed);
            if !collapsed {
                if col == KCol::Todo {
                    quick_add(app, ui);
                }
                task_list(app, ui, col, &tasks, now, edit_id);
            }
        });
    ui.add_space(8.0);
    card.response.rect
}

fn column_head(
    app: &mut App,
    ui: &mut egui::Ui,
    col: KCol,
    count: usize,
    collapsible: bool,
    collapsed: bool,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(5.0, 0.0);
        ui.label(
            RichText::new(col.title())
                .size(fs(11.5))
                .color(col_color(col))
                .strong(),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if collapsible {
                let collapsed = app.st().kanban_collapsed[col.index()];
                if collapse_toggle(ui, collapsed).clicked() {
                    app.core.set_kanban_collapsed(col.index(), !collapsed);
                }
            }
            ui.label(
                RichText::new(count.to_string())
                    .size(fs(11.0))
                    .color(theme::active().dim),
            );
        });
    });

    if collapsed {
        return;
    }
    if col == KCol::Doing {
        let limit = app.st().wip_limit.clamp(1, 99);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(4.0, 0.0);
            ui.label(
                RichText::new(format!("上限 {limit}"))
                    .size(fs(10.5))
                    .color(theme::active().dim),
            );
            if count > limit {
                ui.label(
                    RichText::new("已超")
                        .size(fs(10.5))
                        .color(theme::active().red),
                );
            }
        });
    } else if col == KCol::Overdue {
        ui.label(
            RichText::new("按截止时间自动归入")
                .size(fs(10.0))
                .color(theme::active().dim),
        );
    }
}

fn collapse_toggle(ui: &mut egui::Ui, collapsed: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(20.0, 18.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 5.0, theme::active().hover);
    }
    let c = rect.center();
    let pts = if collapsed {
        vec![
            c + Vec2::new(-3.0, -4.0),
            c + Vec2::new(3.5, 0.0),
            c + Vec2::new(-3.0, 4.0),
        ]
    } else {
        vec![
            c + Vec2::new(-4.5, -2.5),
            c + Vec2::new(4.5, -2.5),
            c + Vec2::new(0.0, 3.5),
        ]
    };
    let color = if resp.hovered() {
        theme::active().text
    } else {
        theme::active().dim
    };
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    ui.painter()
        .add(egui::Shape::convex_polygon(pts, color, Stroke::NONE));
    resp.on_hover_text(if collapsed {
        "展开此列"
    } else {
        "收起此列"
    })
}

fn task_list(
    app: &mut App,
    ui: &mut egui::Ui,
    col: KCol,
    tasks: &[&TaskInfo],
    now: chrono::NaiveDateTime,
    edit_id: &mut Option<u64>,
) {
    if tasks.is_empty() {
        ui.add_space(12.0);
        ui.vertical_centered(|ui| {
            let hint = match col {
                KCol::Todo => "没有待办",
                KCol::Doing => "没有进行中的任务",
                KCol::Done => "还没有完成的任务",
                KCol::Overdue => "没有逾期任务",
            };
            ui.label(
                RichText::new(hint)
                    .size(fs(11.0))
                    .color(theme::active().dim),
            );
        });
        return;
    }
    for info in tasks {
        task_card(app, ui, info, now, edit_id);
    }
}

fn task_card(
    app: &mut App,
    ui: &mut egui::Ui,
    info: &TaskInfo,
    now: chrono::NaiveDateTime,
    edit_id: &mut Option<u64>,
) {
    let frame = Frame::new()
        .fill(theme::active().surface)
        .corner_radius(10.0)
        .inner_margin(Margin::symmetric(10, 8))
        .stroke(Stroke::new(1.0, theme::active().line));
    let inner = frame.show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.spacing_mut().item_spacing = Vec2::new(4.0, 3.0);
        let mut title = RichText::new(&info.text).size(fs(12.5));
        title = if info.is_done() {
            title.color(theme::active().dim).strikethrough()
        } else {
            title.color(theme::active().text)
        };
        ui.add(Label::new(title).wrap());
        if info.important {
            ui.label(
                RichText::new("重要")
                    .size(fs(9.5))
                    .color(theme::active().accent)
                    .strong(),
            );
        }
        if let Some(due) = info.due {
            ui.label(
                RichText::new(format!("截止 {}", due_label(due, now)))
                    .size(fs(10.5))
                    .color(if info.overdue {
                        theme::active().red
                    } else {
                        theme::active().dim
                    }),
            );
        }
        let mut extra: Vec<String> = Vec::new();
        if let Some(repeat) = info.repeat {
            extra.push(repeat.label());
        }
        if info.subtask_total() > 0 {
            extra.push(format!(
                "子项 {}/{}",
                info.subtask_done(),
                info.subtask_total()
            ));
        }
        if info.blocked_left > 0 {
            extra.push(format!("前置未完成 {}", info.blocked_left));
        }
        if !extra.is_empty() {
            ui.label(
                RichText::new(extra.join(" · "))
                    .size(fs(10.0))
                    .color(theme::active().dim),
            );
        }
    });

    let rect = inner.response.rect;
    let hit = ui.interact(
        rect,
        stable_id(("kanban-card", info.id)),
        Sense::click_and_drag(),
    );
    if hit.hovered() && !hit.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if hit.drag_started() {
        app.drag_src = Some(info.id);
    }
    if hit.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    }
    if hit.clicked() {
        *edit_id = Some(info.id);
    }
    ui.add_space(4.0);
}

fn quick_add(app: &mut App, ui: &mut egui::Ui) {
    ui.add_space(2.0);
    let mut new_task = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let more_w = fs(64.0);
        let edit_w = (ui.available_width() - more_w - 4.0).max(48.0);
        let resp = ui.add_sized(
            [edit_w, 24.0],
            TextEdit::singleline(&mut app.todo_draft)
                .hint_text("新任务，回车")
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
        let mut added = false;
        if !new_task
            && (resp.lost_focus() || (resp.has_focus() && enter))
            && !app.todo_draft.trim().is_empty()
        {
            let text = std::mem::take(&mut app.todo_draft);
            match app.core.add_text(text.trim()) {
                Ok(_) => added = true,
                Err(e) => app.notify(e.to_string()),
            }
        }
        if added {
            resp.request_focus();
        }
    });
    if new_task {
        let text = std::mem::take(&mut app.todo_draft);
        app.editor = Some(TaskEditor::new_task(text));
    }
    ui.add_space(4.0);
}

fn drop_task(app: &mut App, id: u64, col: KCol, now: chrono::NaiveDateTime) {
    let target = match col {
        KCol::Todo => TaskStatus::Todo,
        KCol::Doing => TaskStatus::Doing,
        KCol::Done => TaskStatus::Done,
        KCol::Overdue => {
            app.notify("OVERDUE 由截止时间自动判定，不能拖入");
            return;
        }
    };
    match app.core.set_status(id, target) {
        Ok(next) => {
            if let Some(next) = next {
                app.notify(format!("已完成本轮，下次 {}", due_label(next, now)));
            } else if target == TaskStatus::Todo && app.core.task(id).is_ok_and(|t| t.past_due(now))
            {
                app.notify("已标记为待办；截止时间已过，仍留在 OVERDUE");
            }
        }
        Err(e) => app.notify(e.to_string()),
    }
}

pub(crate) fn short(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        out.push('…');
    }
    out
}

pub fn task_editor(app: &mut App, ctx: &egui::Context) {
    let Some(mut ed) = app.editor.take() else {
        return;
    };
    let now = chrono::Local::now().naive_local();
    let mut save = false;
    let mut cancel = false;
    let mut delete = false;
    let mut edited = false;
    let tasks = app.core.settings();

    let response = egui::Modal::new(stable_id("task-editor"))
        .frame(
            Frame::new()
                .fill(theme::active().surface)
                .stroke(Stroke::new(1.0, theme::active().line))
                .corner_radius(14.0)
                .inner_margin(Margin::symmetric(16, 14)),
        )
        .show(ctx, |ui| {
            // 弹窗总宽还要加左右各 16 的内边距，必须给窗口边界留出余量
            let content_w = (ctx.content_rect().width() - 56.0).clamp(190.0, 370.0);
            ui.set_width(content_w);
            ui.label(
                RichText::new(if ed.creating {
                    "新建任务"
                } else {
                    "编辑任务"
                })
                .size(fs(15.0))
                .color(theme::active().text)
                .strong(),
            );
            ui.add_space(8.0);

            // 内容可能很长（子项、前置任务），让正文滚动，底部按钮始终留在窗口内
            let body_max = (ctx.content_rect().height() - 150.0).max(200.0);
            ScrollArea::vertical()
                .id_salt("editor-body")
                .max_height(body_max)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("内容")
                            .size(fs(10.5))
                            .color(theme::active().dim),
                    );
                    ui.add(
                        TextEdit::singleline(&mut ed.draft.text)
                            .desired_width(f32::INFINITY)
                            .hint_text("任务内容")
                            .text_color(theme::active().text),
                    );
                    ui.add_space(4.0);

                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("截止")
                                .size(fs(10.5))
                                .color(theme::active().dim),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let clear = ghost_button(ui, "清除", "清除截止时间");
                            let w = ui.available_width().max(60.0);
                            let date = ui.add_sized(
                                [w, 24.0],
                                TextEdit::singleline(&mut ed.due_date)
                                    .hint_text("YYYY-MM-DD，可留空")
                                    .text_color(theme::active().text),
                            );
                            if date.changed() {
                                ed.split_pasted_time();
                                ed.rebuild_due();
                                ed.draft.postponed = false;
                                edited = true;
                            }
                            if clear.clicked() {
                                ed.due_date.clear();
                                ed.due_has_time = false;
                                ed.rebuild_due();
                                ed.draft.postponed = false;
                                edited = true;
                            }
                        });
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                        let today = now.date();
                        let next_monday = today
                            + chrono::Duration::days(match today.weekday() {
                                chrono::Weekday::Mon => 7,
                                day => 7 - day.num_days_from_monday() as i64,
                            });
                        for (label, date) in [
                            ("今天", today),
                            ("明天", today + chrono::Duration::days(1)),
                            ("下周一", next_monday),
                        ] {
                            if small_link(ui, label).clicked() {
                                ed.due_date = date.format("%Y-%m-%d").to_string();
                                ed.rebuild_due();
                                ed.draft.postponed = false;
                                edited = true;
                            }
                        }
                        let time_toggle = ui.checkbox(
                            &mut ed.due_has_time,
                            RichText::new("指定时间")
                                .size(fs(10.0))
                                .color(theme::active().dim),
                        );
                        if time_toggle.hovered() {
                            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                        }
                        if time_toggle.changed() {
                            ed.rebuild_due();
                            ed.draft.postponed = false;
                            edited = true;
                        }
                        if ed.due_has_time {
                            let hour = ui.add_sized(
                                [36.0, 20.0],
                                egui::DragValue::new(&mut ed.due_hour)
                                    .range(0..=23)
                                    .speed(0.25)
                                    .custom_formatter(|n, _| format!("{:02}", n as u32)),
                            );
                            ui.label(
                                RichText::new("时")
                                    .size(fs(10.0))
                                    .color(theme::active().dim),
                            );
                            let minute = ui.add_sized(
                                [36.0, 20.0],
                                egui::DragValue::new(&mut ed.due_minute)
                                    .range(0..=59)
                                    .speed(0.25)
                                    .custom_formatter(|n, _| format!("{:02}", n as u32)),
                            );
                            ui.label(
                                RichText::new("分")
                                    .size(fs(10.0))
                                    .color(theme::active().dim),
                            );
                            if hour.changed() || minute.changed() {
                                ed.rebuild_due();
                                ed.draft.postponed = false;
                                edited = true;
                            }
                        } else {
                            ui.label(
                                RichText::new("按当天 23:59 截止")
                                    .size(fs(10.0))
                                    .color(theme::active().dim),
                            );
                        }
                    });
                    let due_check = normalize_due(&ed.draft.due);
                    match &due_check {
                        Ok(None) => small_hint(ui, "留空表示没有截止时间"),
                        Ok(Some(normalized)) => {
                            let (text, color) = parse_due(normalized).map_or_else(
                                || ("日期不合法".to_owned(), theme::active().red),
                                |dt| {
                                    if dt <= now {
                                        (
                                            format!("将保存为 {normalized} · 该时间已过"),
                                            theme::active().warn,
                                        )
                                    } else {
                                        (
                                            format!(
                                                "将保存为 {normalized} · {}",
                                                due_label(dt, now)
                                            ),
                                            theme::active().done,
                                        )
                                    }
                                },
                            );
                            ui.label(RichText::new(text).size(fs(10.0)).color(color));
                        }
                        Err(()) => {
                            ui.label(
                                RichText::new("日期不合法，请用 YYYY-MM-DD，如 2026-10-21")
                                    .size(fs(10.0))
                                    .color(theme::active().red),
                            );
                        }
                    };
                    ui.add_space(4.0);

                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.label(
                            RichText::new("延期")
                                .size(fs(10.5))
                                .color(theme::active().dim),
                        );
                        for (label, days) in [("+1 天", 1i64), ("+3 天", 3), ("+1 周", 7)] {
                            if small_link(ui, label).clicked() {
                                ed.draft.postpone_days(days, now);
                                ed.sync_due_from_draft();
                                edited = true;
                            }
                        }
                        if ed.draft.repeat.is_some() {
                            ui.label(
                                RichText::new("只改本轮")
                                    .size(fs(10.0))
                                    .color(theme::active().dim),
                            );
                        }
                    });
                    ui.add_space(4.0);

                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.label(
                            RichText::new("重复")
                                .size(fs(10.5))
                                .color(theme::active().dim),
                        );
                        for (unit, label) in [
                            (None, "无"),
                            (Some(RepeatUnit::Day), "每天"),
                            (Some(RepeatUnit::Week), "每周"),
                            (Some(RepeatUnit::Month), "每月"),
                            (Some(RepeatUnit::Year), "每年"),
                        ] {
                            let active = ed.draft.repeat.map(|r| r.unit) == unit;
                            if status_chip(ui, label, active, theme::active().doing).clicked() {
                                ed.draft.repeat = unit.map(|u| Repeat::new(u, 1));
                            }
                        }
                    });
                    if let Some(mut repeat) = ed.draft.repeat {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            ui.label(
                                RichText::new("间隔")
                                    .size(fs(10.5))
                                    .color(theme::active().dim),
                            );
                            let mut interval = repeat.interval;
                            if ui
                                .add(
                                    egui::DragValue::new(&mut interval)
                                        .range(1..=999)
                                        .speed(0.1),
                                )
                                .changed()
                            {
                                repeat.interval = interval;
                                ed.draft.repeat = Some(repeat);
                            }
                            ui.label(
                                RichText::new(format!(
                                    "个{}（{}）",
                                    repeat.unit.label(),
                                    repeat.label()
                                ))
                                .size(fs(10.0))
                                .color(theme::active().dim),
                            );
                        });
                    }
                    if ed.draft.repeat.is_some() && ed.draft.due.trim().is_empty() {
                        ui.label(
                            RichText::new("周期任务需要截止时间")
                                .size(fs(10.0))
                                .color(theme::active().red),
                        );
                    }
                    ui.add_space(4.0);

                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new("状态")
                                .size(fs(10.5))
                                .color(theme::active().dim),
                        );
                        for (status, color) in [
                            (TaskStatus::Todo, theme::active().dim),
                            (TaskStatus::Doing, theme::active().doing),
                            (TaskStatus::Done, theme::active().done),
                        ] {
                            if status_chip(ui, status.label(), ed.draft.status == status, color)
                                .clicked()
                            {
                                ed.draft.status = status;
                            }
                        }
                    });
                    ui.add_space(4.0);

                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("重要性")
                                .size(fs(10.5))
                                .color(theme::active().dim),
                        );
                        if status_chip(ui, "重要", ed.draft.important, theme::active().accent)
                            .clicked()
                        {
                            ed.draft.important = !ed.draft.important;
                        }
                    });
                    ui.add_space(4.0);

                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.label(
                            RichText::new("子项")
                                .size(fs(10.5))
                                .color(theme::active().dim),
                        );
                        ui.label(
                            RichText::new("可勾选清单，完成主任务时全部勾选")
                                .size(fs(10.0))
                                .color(theme::active().dim),
                        );
                    });
                    let mut remove_subtask = None;
                    ScrollArea::vertical()
                        .id_salt("editor-subtasks")
                        .max_height(110.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for (i, sub) in ed.draft.subtasks.iter_mut().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    let cb = ui.add(egui::Checkbox::without_text(&mut sub.done));
                                    if cb.hovered() {
                                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                                    }
                                    let del_w = fs(24.0);
                                    let w = (ui.available_width() - del_w - 4.0).max(60.0);
                                    ui.add_sized(
                                        [w, 24.0],
                                        TextEdit::singleline(&mut sub.text)
                                            .hint_text("子项内容")
                                            .text_color(theme::active().text),
                                    );
                                    let del = ghost_button(ui, "×", "移除子项");
                                    if del.hovered() {
                                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                                    }
                                    if del.clicked() {
                                        remove_subtask = Some(i);
                                    }
                                });
                            }
                        });
                    if let Some(i) = remove_subtask {
                        ed.draft.subtasks.remove(i);
                    }
                    let add_subtask = ghost_button(ui, "＋ 添加子项", "加一条可勾选子项");
                    if add_subtask.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    }
                    if add_subtask.clicked() {
                        ed.draft.subtasks.push(Subtask::new(String::new()));
                    }
                    if ed.draft.repeat.is_some() {
                        small_hint(ui, "周期任务完成本轮后，子项会重置为未勾选");
                    }

                    let selected_deps = ed.draft.blocked_by.len();
                    egui::CollapsingHeader::new(format!("前置任务（{selected_deps}）"))
                        .id_salt("editor-deps")
                        .default_open(selected_deps > 0)
                        .show(ui, |ui| {
                            ScrollArea::vertical()
                                .id_salt("editor-deps-scroll")
                                .max_height(132.0)
                                .auto_shrink([false, true])
                                .show(ui, |ui| {
                                    let candidate_ids: Vec<u64> = tasks
                                        .todos
                                        .iter()
                                        .filter(|t| {
                                            t.id != ed.draft.id
                                                && (ed.draft.blocked_by.contains(&t.id)
                                                    || !tasks.dep_would_cycle(ed.draft.id, t.id))
                                        })
                                        .map(|t| t.id)
                                        .collect();
                                    if candidate_ids.is_empty() {
                                        ui.label(
                                            RichText::new("没有可选任务")
                                                .size(fs(11.0))
                                                .color(theme::active().dim),
                                        );
                                    }
                                    for id in candidate_ids {
                                        let Some(t) = tasks.task(id) else {
                                            continue;
                                        };
                                        let mut on = ed.draft.blocked_by.contains(&id);
                                        let dep = ui.checkbox(
                                            &mut on,
                                            format!(
                                                "{} · {}",
                                                short(&t.text, 26),
                                                t.status.label()
                                            ),
                                        );
                                        if dep.hovered() {
                                            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                                        }
                                        if dep.changed() {
                                            if on {
                                                ed.draft.blocked_by.push(id);
                                            } else {
                                                ed.draft.blocked_by.retain(|b| *b != id);
                                            }
                                        }
                                    }
                                });
                        });

                    if ed.draft.status == TaskStatus::Doing {
                        ui.label(
                            RichText::new(format!(
                                "进行中上限 {}，可在设置里调整",
                                tasks.wip_limit
                            ))
                            .size(fs(10.0))
                            .color(theme::active().dim),
                        );
                    }
                    if edited {
                        ed.error = None;
                    }
                });

            if let Some(err) = &ed.error {
                ui.add_space(4.0);
                ui.label(RichText::new(err).size(fs(11.5)).color(theme::active().red));
            }

            let due_missing = ed.draft.repeat.is_some() && ed.draft.due.trim().is_empty();
            let due_invalid = normalize_due(&ed.draft.due).is_err();
            let can_save = !ed.draft.text.trim().is_empty() && !due_missing && !due_invalid;

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if !ed.creating {
                    let del = egui::Button::new(
                        RichText::new("删除")
                            .size(fs(12.0))
                            .color(theme::active().red),
                    )
                    .fill(Color32::TRANSPARENT)
                    .stroke(Stroke::NONE)
                    .corner_radius(6.0);
                    let del = ui.add(del);
                    if del.clicked() {
                        delete = true;
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let ok = egui::Button::new(
                        RichText::new(if ed.creating { "创建" } else { "保存" })
                            .size(fs(12.0))
                            .color(Color32::WHITE)
                            .strong(),
                    )
                    .fill(theme::active().accent)
                    .corner_radius(6.0);
                    let ok = ui.add_enabled(can_save, ok);
                    if ok.clicked() {
                        save = true;
                    }
                    let cancel_resp = ghost_button(ui, "取消", "放弃修改");
                    if cancel_resp.clicked() {
                        cancel = true;
                    }
                });
            });
        });

    if response.should_close() && !save && !delete {
        cancel = true;
    }
    if delete {
        match app.core.remove(ed.draft.id) {
            Ok(view) => app.notify(format!("已删除「{}」", short(&view.text, 12))),
            Err(e) => app.notify(e.to_string()),
        }
        return;
    }
    if save {
        if ed.creating {
            match app.core.create_task_draft(&ed.draft, now) {
                Ok(view) => {
                    app.notify(format!("已创建「{}」", short(&view.text, 12)));
                    app.focus_todo = true;
                }
                Err(msg) => {
                    ed.error = Some(msg);
                    app.editor = Some(ed);
                }
            }
            return;
        }
        match app.core.save_task_draft(&ed.draft, now) {
            Ok(next_due) => {
                if let Some(next_due) = next_due {
                    app.notify(format!(
                        "完成「{}」本轮，下次 {}",
                        ed.draft.text.trim(),
                        due_label(next_due, now)
                    ));
                }
            }
            Err(msg) => {
                ed.error = Some(msg);
                app.editor = Some(ed);
            }
        }
        return;
    }
    if cancel {
        // 从快速添加打开时，取消后把已输入的内容还回输入框
        if ed.creating && app.todo_draft.is_empty() {
            app.todo_draft = std::mem::take(&mut ed.draft.text);
            app.focus_todo = true;
        }
        return;
    }
    app.editor = Some(ed);
}

fn status_chip(ui: &mut egui::Ui, text: &str, active: bool, color: Color32) -> egui::Response {
    let btn = egui::Button::new(RichText::new(text).size(fs(11.5)).color(if active {
        theme::active().on_color
    } else {
        color
    }))
    .fill(if active { color } else { Color32::TRANSPARENT })
    .stroke(Stroke::new(
        1.0,
        if active { color } else { theme::active().line },
    ))
    .corner_radius(6.0)
    .min_size(Vec2::new(0.0, 24.0));
    ui.add(btn)
}

fn small_link(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            RichText::new(text)
                .size(fs(10.5))
                .color(theme::active().dim),
        )
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::NONE)
        .corner_radius(5.0)
        .min_size(Vec2::new(0.0, 20.0)),
    )
}

fn small_hint(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(fs(10.0))
            .color(theme::active().dim),
    );
}
