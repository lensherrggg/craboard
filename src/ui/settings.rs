use super::board::stable_id;
use super::{fs, moment_label, theme};
use crate::App;
use craboard::core::{AiSummaryView, SettingsPatch};
use craboard::state::{CloseBehavior, FontSize, ThemeKind, normalize_clock};
use eframe::egui;
use egui::{Align, DragValue, Frame, Layout, Margin, RichText, ScrollArea, Stroke, TextEdit, Vec2};
use std::path::PathBuf;

struct SettingsDraft {
    show_seconds: bool,
    autostart: bool,
    close_behavior: CloseBehavior,
    font_size: FontSize,
    theme: ThemeKind,
    reminders_enabled: bool,
    reminder_seconds: u32,
    wip_limit: usize,
    urgent_within_hours: u32,
    ai_summary_enabled: bool,
    ai_summary_daily_enabled: bool,
    ai_summary_daily_times: Vec<String>,
    ai_summary_interval_enabled: bool,
    ai_summary_interval_hours: u32,
}

pub fn settings_modal(app: &mut App, ctx: &egui::Context) {
    if !app.settings_open {
        return;
    }
    let exp = app.experimental;
    let mut draft = SettingsDraft {
        show_seconds: app.st().show_seconds,
        autostart: app.st().autostart,
        close_behavior: app.st().close_behavior,
        font_size: app.st().font_size,
        theme: app.st().theme,
        reminders_enabled: app.st().reminders_enabled,
        reminder_seconds: app.st().reminder_seconds,
        wip_limit: app.st().wip_limit,
        urgent_within_hours: app.st().urgent_within_hours,
        ai_summary_enabled: app.st().ai_summary_enabled,
        ai_summary_daily_enabled: app.st().ai_summary_daily_enabled,
        ai_summary_daily_times: app.st().ai_summary_daily_times.clone(),
        ai_summary_interval_enabled: app.st().ai_summary_interval_enabled,
        ai_summary_interval_hours: app.st().ai_summary_interval_hours,
    };
    let ai = exp.summary().then(|| app.core.ai_summary_view());
    let data_dir = app.core.data_dir();
    let (mcp_json, cli_found) = mcp_config();
    // 设置弹窗盖在 toast 上面，复制反馈直接做在按钮上，1.6 秒后复原
    let copy_id = egui::Id::unique("mcp-copied-at");
    let now = ctx.input(|i| i.time);
    let copied = ctx
        .data(|d| d.get_temp::<f64>(copy_id))
        .is_some_and(|t| now - t < 1.6);
    let mut changed = false;
    let mut close = false;
    let mut open_dir = false;
    let mut copy_mcp = false;

    {
        let response = egui::Modal::new(stable_id("settings-modal"))
            .frame(
                Frame::new()
                    .fill(theme::active().surface)
                    .stroke(Stroke::new(1.0, theme::active().line))
                    .corner_radius(14.0)
                    .inner_margin(Margin::symmetric(16, 14)),
            )
            .show(ctx, |ui| {
                let content_w = (ctx.content_rect().width() - 56.0).clamp(190.0, 400.0);
                ui.set_width(content_w);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("设置")
                            .size(fs(15.0))
                            .color(theme::active().text)
                            .strong(),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let done = egui::Button::new(
                            RichText::new("完成")
                                .size(fs(12.0))
                                .color(theme::active().on_color)
                                .strong(),
                        )
                        .fill(theme::active().accent)
                        .corner_radius(6.0);
                        if ui.add(done).clicked() {
                            close = true;
                        }
                    });
                });
                ui.add_space(4.0);

                // 浮动滚动条在弹窗里几乎不可见，用固定条提示下方还有内容
                ui.spacing_mut().scroll.floating = false;
                let max_h = (ctx.content_rect().height() - 130.0).max(180.0);
                ScrollArea::vertical()
                    .id_salt("settings-scroll")
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                    .max_height(max_h)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        section(ui, "常规");
                        font_size_row(ui, &mut draft.font_size);
                        theme_row(ui, &mut draft.theme);
                        hint(ui, "低饱和暖色，参考 Solarized 的明度层级；切换即时生效");
                        changed |= toggle_row(ui, "时钟显示秒", &mut draft.show_seconds);
                        changed |= toggle_row(ui, "开机自启", &mut draft.autostart);
                        hint(
                            ui,
                            "登录 Windows 后自动启动；只写当前用户的启动项，不需要管理员权限",
                        );
                        close_behavior_row(ui, &mut draft.close_behavior);
                        hint(
                            ui,
                            "标题栏 ×、Alt+F4 和任务栏关闭都按这里执行；选「每次询问」会先弹出确认框",
                        );

                        section(ui, "提醒");
                        changed |= toggle_row(ui, "启用截止提醒", &mut draft.reminders_enabled);
                        changed |= number_row_u32(
                            ui,
                            "提醒停留",
                            &mut draft.reminder_seconds,
                            3..=60,
                            " 秒",
                            draft.reminders_enabled,
                        );
                        hint(ui, "鼠标移入提醒小窗后不再自动关闭");

                        section(ui, "看板");
                        changed |= number_row_usize(
                            ui,
                            "进行中上限",
                            &mut draft.wip_limit,
                            1..=99,
                            " 个",
                            true,
                        );
                        hint(ui, "超过上限时不能再开始新的任务");

                        section(ui, "四象限");
                        changed |= number_row_u32(
                            ui,
                            "紧急判定窗口",
                            &mut draft.urgent_within_hours,
                            1..=720,
                            " 小时",
                            true,
                        );
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                            for hours in [12u32, 24, 48, 72] {
                                let active = draft.urgent_within_hours == hours;
                                if preset_chip(ui, format!("{hours} 小时"), active).clicked() {
                                    draft.urgent_within_hours = hours;
                                    changed = true;
                                }
                            }
                        });
                        hint(
                            ui,
                            "截止时间落在这个窗口内或已经逾期都算紧急；没有截止时间不紧急",
                        );

                        if let Some(ai) = ai.as_ref() {
                            section(ui, "AI 总结（试验）");
                            if exp.active() {
                                changed |=
                                    toggle_row(ui, "启用定时总结", &mut draft.ai_summary_enabled);
                                let was_daily = draft.ai_summary_daily_enabled;
                                ui.add_enabled_ui(draft.ai_summary_enabled, |ui| {
                                    changed |= toggle_row(
                                        ui,
                                        "每日定点",
                                        &mut draft.ai_summary_daily_enabled,
                                    );
                                    if !was_daily
                                        && draft.ai_summary_daily_enabled
                                        && draft.ai_summary_daily_times.is_empty()
                                    {
                                        draft.ai_summary_daily_times =
                                            vec!["09:00".to_owned(), "18:00".to_owned()];
                                        changed = true;
                                    }
                                    if draft.ai_summary_daily_enabled {
                                        changed |= daily_times_row(
                                            ui,
                                            &mut draft.ai_summary_daily_times,
                                            &mut app.settings_time_input,
                                        );
                                    }
                                    changed |= toggle_row(
                                        ui,
                                        "固定间隔",
                                        &mut draft.ai_summary_interval_enabled,
                                    );
                                    if draft.ai_summary_interval_enabled {
                                        changed |= number_row_u32(
                                            ui,
                                            "间隔",
                                            &mut draft.ai_summary_interval_hours,
                                            1..=168,
                                            " 小时",
                                            true,
                                        );
                                    }
                                });
                                hint(
                                    ui,
                                    "每日定点每次覆盖当天 0:00 到该时刻；固定间隔从上次总结开始计时。即时总结会重置间隔计时，两种模式可同时开启。",
                                );
                                let now = chrono::Local::now().naive_local();
                                let (status, color) = if !draft.ai_summary_enabled {
                                    (
                                        "定时总结已关闭 · 悬浮页仍可点「即时总结」".to_owned(),
                                        theme::active().dim,
                                    )
                                } else if !ai.daily_enabled && !ai.interval_enabled {
                                    (
                                        "两种定时模式都未开启 · 仅能手动即时总结".to_owned(),
                                        theme::active().dim,
                                    )
                                } else if ai.stale {
                                    (
                                        format!(
                                            "总结待更新 · 已过 {} · 窗口 {}",
                                            ai.next_due_at
                                                .as_deref()
                                                .map(|s| moment_label(s, now))
                                                .unwrap_or_else(|| "—".to_owned()),
                                            window_text(ai, now),
                                        ),
                                        theme::active().warn,
                                    )
                                } else if let Some(due) = ai.next_due_at.as_deref() {
                                    (
                                        format!(
                                            "下次更新 {} · 窗口 {}",
                                            moment_label(due, now),
                                            window_text(ai, now),
                                        ),
                                        theme::active().done,
                                    )
                                } else if ai.available {
                                    (
                                        format!(
                                            "总结可用 · 最近生成 {}",
                                            ai.generated_at.as_deref().unwrap_or("—")
                                        ),
                                        theme::active().done,
                                    )
                                } else {
                                    (
                                        "定时总结已开启 · 等待第一个触发时间点".to_owned(),
                                        theme::active().dim,
                                    )
                                };
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(status).size(fs(10.0)).color(color),
                                    )
                                    .wrap(),
                                );
                                ui.add_space(2.0);
                            } else {
                                hint(
                                    ui,
                                    "已开启总结面板；即时总结、每日定点和固定间隔需要在程序目录的 experimental.json 中设置 ai_summary_active 后重启。",
                                );
                                let (status, color) = if ai.available {
                                    (
                                        format!(
                                            "已收到 AI 总结 · 最近生成 {}",
                                            ai.generated_at.as_deref().unwrap_or("—")
                                        ),
                                        theme::active().done,
                                    )
                                } else {
                                    (
                                        "等待 AI 助手通过 MCP 推送".to_owned(),
                                        theme::active().dim,
                                    )
                                };
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(status).size(fs(10.0)).color(color),
                                    )
                                    .wrap(),
                                );
                                ui.add_space(2.0);
                            }
                        }

                        section(ui, "数据");
                        ui.add(
                            egui::Label::new(
                                RichText::new(data_dir.display().to_string())
                                    .size(fs(10.5))
                                    .color(theme::active().dim),
                            )
                            .wrap(),
                        );
                        ui.add_space(3.0);
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("打开数据目录")
                                        .size(fs(11.5))
                                        .color(theme::active().text),
                                )
                                .fill(theme::active().field)
                                .stroke(Stroke::new(1.0, theme::active().line))
                                .corner_radius(6.0),
                            )
                            .clicked()
                        {
                            open_dir = true;
                        }

                        section(ui, "MCP 接入");
                        hint(
                            ui,
                            "MCP 客户端会通过 stdio 启动 craboard-cli mcp，不需要常驻服务，也不用单独开机自启。",
                        );
                        Frame::new()
                            .fill(theme::active().field)
                            .stroke(Stroke::new(1.0, theme::active().line))
                            .corner_radius(6.0)
                            .inner_margin(Margin::symmetric(8, 6))
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&mcp_json)
                                            .monospace()
                                            .size(fs(9.5))
                                            .color(theme::active().text),
                                    )
                                    .wrap()
                                    .selectable(true),
                                );
                            });
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(if copied { "已复制" } else { "复制 JSON" })
                                            .size(fs(11.5))
                                            .color(theme::active().on_color)
                                            .strong(),
                                    )
                                    .fill(theme::active().accent)
                                    .corner_radius(6.0)
                                    .min_size(Vec2::new(70.0, 0.0)),
                                )
                                .clicked()
                            {
                                copy_mcp = true;
                            }
                            ui.add(
                                egui::Label::new(
                                    RichText::new(if exp.summary() {
                                        "可用工具：list_tasks / add_task / update_task / get_ai_summary / set_ai_summary 等"
                                    } else {
                                        "可用工具：list_tasks / add_task / update_task / complete_task / postpone_task 等"
                                    })
                                    .size(fs(9.5))
                                    .color(theme::active().dim),
                                )
                                .wrap(),
                            );
                        });
                        if cli_found {
                            hint(
                                ui,
                                "把整段 JSON 合并进客户端的 mcpServers 后重启客户端；移动程序目录后需要重新复制。",
                            );
                            if exp.summary() {
                                hint(
                                    ui,
                                    if exp.active() {
                                        "AI 总结已开启：agent 可用 get_ai_summary 查看待更新窗口，再用 set_ai_summary 推送正文。"
                                    } else {
                                        "AI 总结已开启（仅推送）：agent 调用 set_ai_summary 后悬浮页会显示正文；即时与定时总结需在 experimental.json 开启 ai_summary_active。"
                                    },
                                );
                            }
                        } else {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(
                                        "程序目录里没找到 craboard-cli.exe，请确认解压了完整的发布包。",
                                    )
                                    .size(fs(10.0))
                                    .color(theme::active().red),
                                )
                                .wrap(),
                            );
                            ui.add_space(2.0);
                        }
                        ui.add_space(4.0);
                    });
            });

        if response.should_close() {
            close = true;
        }
    }

    if changed {
        let _ = app.core.update_settings(SettingsPatch {
            show_seconds: Some(draft.show_seconds),
            reminders_enabled: Some(draft.reminders_enabled),
            reminder_seconds: Some(draft.reminder_seconds),
            wip_limit: Some(draft.wip_limit),
            urgent_within_hours: Some(draft.urgent_within_hours),
            ai_summary_enabled: exp.active().then_some(draft.ai_summary_enabled),
            ai_summary_daily_enabled: exp.active().then_some(draft.ai_summary_daily_enabled),
            ai_summary_daily_times: exp.active().then(|| draft.ai_summary_daily_times.clone()),
            ai_summary_interval_enabled: exp.active().then_some(draft.ai_summary_interval_enabled),
            ai_summary_interval_hours: exp.active().then_some(draft.ai_summary_interval_hours),
            ..Default::default()
        });
    }
    if draft.font_size != app.st().font_size {
        app.core.set_font_size(draft.font_size);
    }
    if draft.theme != app.st().theme {
        app.core.set_theme(draft.theme);
    }
    if draft.autostart != app.st().autostart {
        if let Err(e) = app.set_autostart(draft.autostart) {
            app.notify(format!("开机自启设置失败：{e}"));
        }
    }
    if draft.close_behavior != app.st().close_behavior {
        app.core.set_close_behavior(draft.close_behavior);
    }
    if close {
        app.settings_open = false;
    }
    if open_dir {
        let _ = std::process::Command::new("explorer")
            .arg(&data_dir)
            .spawn();
    }
    if copy_mcp {
        ctx.copy_text(mcp_json);
        ctx.data_mut(|d| d.insert_temp(copy_id, now));
        // 立刻重绘一次让按钮显示「已复制」，再在有效期结束时重绘复原
        ctx.request_repaint();
        ctx.request_repaint_after(std::time::Duration::from_millis(1600));
    }
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(9.0);
    ui.label(
        RichText::new(title)
            .size(fs(11.0))
            .color(theme::active().dim)
            .strong(),
    );
    let y = ui.cursor().top() + 2.0;
    ui.painter().hline(
        ui.max_rect().x_range(),
        y,
        Stroke::new(1.0, theme::active().line),
    );
    ui.add_space(5.0);
}

fn font_size_row(ui: &mut egui::Ui, value: &mut FontSize) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("字号")
                .size(fs(12.0))
                .color(theme::active().text),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(4.0, 2.0);
            for size in FontSize::ALL.iter().rev() {
                if preset_chip(ui, size.label().to_owned(), *value == *size).clicked() {
                    *value = *size;
                }
            }
        });
    });
}

fn theme_row(ui: &mut egui::Ui, value: &mut ThemeKind) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("主题")
                .size(fs(12.0))
                .color(theme::active().text),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(4.0, 2.0);
            for kind in ThemeKind::ALL.iter().rev() {
                if preset_chip(ui, kind.label().to_owned(), *value == *kind).clicked() {
                    *value = *kind;
                }
            }
        });
    });
}

fn close_behavior_row(ui: &mut egui::Ui, value: &mut CloseBehavior) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("关闭窗口")
                .size(fs(12.0))
                .color(theme::active().text),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(4.0, 2.0);
            for behavior in CloseBehavior::ALL.iter().rev() {
                if preset_chip(ui, behavior.label().to_owned(), *value == *behavior).clicked() {
                    *value = *behavior;
                }
            }
        });
    });
}

fn window_text(ai: &AiSummaryView, now: chrono::NaiveDateTime) -> String {
    match (ai.window_start.as_deref(), ai.window_end.as_deref()) {
        (Some(start), Some(end)) => {
            format!("{} → {}", moment_label(start, now), moment_label(end, now))
        }
        _ => "—".to_owned(),
    }
}

fn daily_times_row(ui: &mut egui::Ui, times: &mut Vec<String>, input: &mut String) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
        let mut remove = None;
        for (idx, time) in times.iter().enumerate() {
            let chip = egui::Button::new(
                RichText::new(format!("{time}  ×"))
                    .size(fs(10.0))
                    .color(theme::active().text),
            )
            .fill(theme::active().field)
            .stroke(Stroke::new(1.0, theme::active().line))
            .corner_radius(5.0)
            .min_size(Vec2::new(0.0, 20.0));
            let resp = ui.add(chip);
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                remove = Some(idx);
            }
        }
        if let Some(idx) = remove {
            times.remove(idx);
            changed = true;
        }
    });
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let resp = ui.add(
            TextEdit::singleline(input)
                .hint_text("09:00")
                .desired_width(fs(58.0))
                .text_color(theme::active().text),
        );
        let valid = normalize_clock(input);
        let add = ui.add(
            egui::Button::new(
                RichText::new("添加")
                    .size(fs(10.0))
                    .color(theme::active().text),
            )
            .fill(theme::active().field)
            .stroke(Stroke::new(1.0, theme::active().line))
            .corner_radius(5.0)
            .min_size(Vec2::new(0.0, 20.0)),
        );
        if add.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if add.clicked() || enter {
            if let Some(time) = valid {
                if !times.contains(&time) {
                    times.push(time);
                    times.sort();
                    changed = true;
                }
                input.clear();
            }
        }
    });
    if !input.trim().is_empty() && normalize_clock(input).is_none() {
        ui.add(
            egui::Label::new(
                RichText::new("时间格式应为 HH:MM，例如 09:30")
                    .size(fs(9.5))
                    .color(theme::active().warn),
            )
            .wrap(),
        );
    } else if times.is_empty() {
        hint(ui, "至少添加一个时间点后每日定点才会生效");
    }
    changed
}

fn toggle_row(ui: &mut egui::Ui, label: &str, value: &mut bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(label)
                .size(fs(12.0))
                .color(theme::active().text),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if switch(ui, value).clicked() {
                *value = !*value;
                changed = true;
            }
        });
    });
    changed
}

/// egui 自带 checkbox 深色主题下几乎看不见边，这里画个一眼能读的开关
fn switch(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(34.0, 18.0), egui::Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let t = ui.ctx().animate_bool(resp.id, *on);
    let track = egui::Rect::from_center_size(rect.center(), Vec2::new(32.0, 16.0));
    let bg = theme::active()
        .hover
        .lerp_to_gamma(theme::active().accent, t);
    ui.painter().rect_filled(track, 8.0, bg);
    let r = 6.0;
    let x = track.left() + 8.0 + (track.width() - 16.0) * t;
    let knob = theme::active()
        .text
        .lerp_to_gamma(theme::active().on_color, t);
    ui.painter()
        .circle_filled(egui::pos2(x, track.center().y), r, knob);
    resp
}

fn number_row_u32(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut u32,
    range: std::ops::RangeInclusive<u32>,
    suffix: &str,
    enabled: bool,
) -> bool {
    let mut changed = false;
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(label)
                    .size(fs(12.0))
                    .color(theme::active().text),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(DragValue::new(value).range(range).speed(0.2).suffix(suffix))
                    .changed()
                {
                    changed = true;
                }
            });
        });
    });
    changed
}

fn number_row_usize(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut usize,
    range: std::ops::RangeInclusive<usize>,
    suffix: &str,
    enabled: bool,
) -> bool {
    let mut changed = false;
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(label)
                    .size(fs(12.0))
                    .color(theme::active().text),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(DragValue::new(value).range(range).speed(0.2).suffix(suffix))
                    .changed()
                {
                    changed = true;
                }
            });
        });
    });
    changed
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .size(fs(10.0))
                .color(theme::active().dim),
        )
        .wrap(),
    );
    ui.add_space(2.0);
}

fn preset_chip(ui: &mut egui::Ui, text: String, active: bool) -> egui::Response {
    let btn = egui::Button::new(RichText::new(text).size(fs(10.5)).color(if active {
        theme::active().on_color
    } else {
        theme::active().dim
    }))
    .fill(if active {
        theme::active().accent
    } else {
        egui::Color32::TRANSPARENT
    })
    .stroke(Stroke::new(
        1.0,
        if active {
            theme::active().accent
        } else {
            theme::active().line
        },
    ))
    .corner_radius(6.0)
    .min_size(Vec2::new(0.0, 22.0));
    ui.add(btn)
}

/// 用界面自己的实际安装位置生成配置，避免让用户手填 target/release 之类的路径。
/// 客户端习惯先读 command，手工拼字符串以固定字段顺序（serde_json 会按键名排序）。
fn mcp_config() -> (String, bool) {
    let cli = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("craboard-cli.exe")))
        .unwrap_or_else(|| PathBuf::from("craboard-cli.exe"));
    let command = serde_json::to_string(&cli.to_string_lossy())
        .unwrap_or_else(|_| "\"craboard-cli.exe\"".to_owned());
    let text = format!(
        r#"{{
  "mcpServers": {{
    "craboard": {{
      "command": {command},
      "args": ["mcp"]
    }}
  }}
}}"#
    );
    (text, cli.exists())
}
