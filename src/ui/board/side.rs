use super::*;

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

pub(super) fn weekday_cn(d: chrono::Weekday) -> &'static str {
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

pub(super) fn side_panel(app: &mut App, ui: &mut egui::Ui) {
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
