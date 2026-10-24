use super::*;

pub(super) fn list_view(app: &mut App, ui: &mut egui::Ui) {
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
pub(super) enum RowAction {
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

pub(super) fn apply_actions(app: &mut App, actions: Vec<RowAction>, now: NaiveDateTime) {
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

pub(super) fn add_task_input(app: &mut App, ui: &mut egui::Ui, hint: &str) {
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
pub(super) fn task_row(
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
