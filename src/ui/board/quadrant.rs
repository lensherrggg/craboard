use super::list::{RowAction, add_task_input, apply_actions, task_row};
use super::*;

fn quad_color(q: Quadrant) -> Color32 {
    match q {
        Quadrant::ImportantUrgent => theme::active().red,
        Quadrant::Important => theme::active().accent,
        Quadrant::Urgent => theme::active().warn,
        Quadrant::Normal => theme::active().dim,
    }
}

pub(super) fn quadrant_view(app: &mut App, ui: &mut egui::Ui) {
    let now = chrono::Local::now().naive_local();
    let infos = task_infos(app.st());
    let hours = app.st().urgent_within_hours;
    let open = infos.iter().filter(|i| !i.is_done()).count();
    let mut actions = Vec::new();

    Frame::new()
        .fill(Color32::TRANSPARENT)
        .inner_margin(egui::Margin {
            left: 14,
            right: 14,
            top: 6,
            bottom: 14,
        })
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
