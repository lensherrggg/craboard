use super::side::weekday_cn;
use super::*;

/// 左下角今日进度。文字/图标两种形态，点击展开今日总结/完成情况。
/// 返回 true 表示弹层里点了「即时总结」，由调用方写回总结正文。
pub(super) fn today_progress(
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
            // 手算高度总会比实际排版少一到两行间距；多留两行，避免字体度量
            // 偏差把卡片下沿顶到窗口边缘
            6.0 + title_h + gap + 4.0 + 3.0 + gap + 4.0 + small_h + 6.0 + 2.0 * gap
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
