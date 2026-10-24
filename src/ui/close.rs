use super::board::stable_id;
use super::{fs, theme};
use crate::App;
use eframe::egui;
use egui::{Align, Frame, Layout, Margin, RichText, Stroke, Vec2};

#[derive(Clone, Copy, PartialEq)]
enum Choice {
    Cancel,
    Tray,
    Quit,
}

pub fn confirm(app: &mut App, ctx: &egui::Context) {
    if !app.close_prompt {
        return;
    }
    let mut choice = None;

    let response = egui::Modal::new(stable_id("close-confirm"))
        .frame(
            Frame::new()
                .fill(theme::active().surface)
                .stroke(Stroke::new(1.0, theme::active().line))
                .corner_radius(14.0)
                .inner_margin(Margin::symmetric(16, 14)),
        )
        .show(ctx, |ui| {
            let width = (ctx.content_rect().width() - 56.0).clamp(200.0, 320.0);
            ui.set_width(width);
            ui.label(
                RichText::new("关闭 Craboard")
                    .size(fs(15.0))
                    .color(theme::active().text)
                    .strong(),
            );
            ui.add_space(6.0);
            ui.add(
                egui::Label::new(
                    RichText::new("最小化后程序仍在托盘运行，双击托盘图标可以重新打开。")
                        .size(fs(11.5))
                        .color(theme::active().dim),
                )
                .wrap(),
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if plain_button(ui, "取消", false).clicked() {
                    choice = Some(Choice::Cancel);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if plain_button(ui, "最小化到托盘", true).clicked() {
                        choice = Some(Choice::Tray);
                    }
                    if plain_button(ui, "退出程序", false).clicked() {
                        choice = Some(Choice::Quit);
                    }
                });
            });
            ui.add_space(6.0);
            ui.add(
                egui::Label::new(
                    RichText::new("可在 设置 → 常规 → 关闭窗口 中修改默认行为")
                        .size(fs(9.5))
                        .color(theme::active().dim),
                )
                .wrap(),
            );
        });

    if response.should_close() {
        choice = Some(Choice::Cancel);
    }

    match choice {
        Some(Choice::Tray) => {
            app.close_prompt = false;
            app.set_visible(ctx, false);
        }
        Some(Choice::Quit) => {
            app.close_prompt = false;
            app.quit();
        }
        Some(Choice::Cancel) => app.close_prompt = false,
        None => {}
    }
}

fn plain_button(ui: &mut egui::Ui, text: &str, primary: bool) -> egui::Response {
    let color = if primary {
        theme::active().on_color
    } else {
        theme::active().text
    };
    let mut button = egui::Button::new(RichText::new(text).size(fs(11.5)).color(color))
        .corner_radius(6.0)
        .min_size(Vec2::new(0.0, 26.0));
    button = if primary {
        button
            .fill(theme::active().accent)
            .stroke(Stroke::new(1.0, theme::active().accent))
    } else {
        button
            .fill(theme::active().field)
            .stroke(Stroke::new(1.0, theme::active().line))
    };
    ui.add(button)
}
