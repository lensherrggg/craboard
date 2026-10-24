#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ui;
mod win32;

use craboard::core::{Core, ListFilter};
use craboard::experimental::Experimental;
use craboard::state::{self, CloseBehavior, Settings};
use eframe::egui;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

const TITLE: &str = "Craboard";
const TRAY_DOUBLE_CLICK: Duration = Duration::from_millis(350);

#[derive(Clone, Copy, PartialEq)]
enum TrayAction {
    Show,
    Hide,
    Quit,
}

#[derive(Default)]
pub struct Events {
    tray: Vec<TrayIconEvent>,
    menu: Vec<MenuEvent>,
    hotkey: Vec<GlobalHotKeyEvent>,
}

pub struct App {
    pub core: Core,
    pub experimental: Experimental,
    pub todo_draft: String,
    pub filter: ListFilter,
    pub focus_todo: bool,
    last_save: Instant,
    tray: Option<TrayIcon>,
    tray_menu: Vec<(MenuId, TrayAction)>,
    events: Arc<Mutex<Events>>,
    hot: Option<GlobalHotKeyManager>,
    hk_tried: bool,
    hk_toggle: u32,
    hk_add: u32,
    hwnd: Option<win32::HWND>,
    visible: bool,
    tray_click_pending: Option<Instant>,
    tray_ignore_click: Option<Instant>,
    pub drag_src: Option<u64>,
    /// 移动/缩放由我们自己跟全局光标算，winit 的 StartDrag/BeginResize 在垫底窗口上不可靠
    pub move_drag: Option<(egui::Pos2, (f32, f32))>,
    pub resize_drag: Option<ui::board::ResizeDrag>,
    pub editor: Option<ui::kanban::TaskEditor>,
    pub settings_open: bool,
    pub close_prompt: bool,
    pub toast: Option<(String, Instant)>,
    pub notes_drag_start: Option<(f32, egui::Vec2)>,
    pub focus_note: Option<u64>,
    pub settings_time_input: String,
    reminder: ui::reminder::Reminder,
    last_due_check: Option<chrono::NaiveDateTime>,
    /// 磁盘上的任务库/设置是否被命令行或 MCP 改过；后台线程只置位，真正重载在 logic 里
    watch_flag: Arc<AtomicBool>,
    watch_paths: Vec<PathBuf>,
    watch_stamps: Vec<Option<SystemTime>>,
    ime_focus: bool,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        mut core: Core,
        experimental: Experimental,
        notice: Option<String>,
    ) -> Self {
        setup_fonts(&cc.egui_ctx);
        ui::theme::apply(&cc.egui_ctx, core.settings().theme);
        ui::apply_font_size(&cc.egui_ctx, core.settings().font_size);
        // 把迁移/规范化后的设置先落一份，旧存档字段也会在这里完成迁移
        core.touch_settings();
        if let Err(e) = core.save() {
            log(&format!("任务存储初始化失败: {e}"));
        }
        // 程序目录可能换过，开启自启时把启动项更新到当前 exe
        if core.settings().autostart {
            if let Err(e) = win32::set_autostart(true) {
                log(&format!("更新开机自启失败: {e}"));
            }
        }
        let watch_paths = watch_paths_of(&core);
        let watch_stamps = file_stamps(&watch_paths);
        let watch_flag = Arc::new(AtomicBool::new(false));
        spawn_watcher(
            cc.egui_ctx.clone(),
            watch_paths.clone(),
            watch_stamps.clone(),
            watch_flag.clone(),
        );
        Self {
            core,
            experimental,
            todo_draft: String::new(),
            filter: ListFilter::Active,
            focus_todo: false,
            last_save: Instant::now(),
            tray: None,
            tray_menu: Vec::new(),
            events: Arc::new(Mutex::new(Events::default())),
            hot: None,
            hk_tried: false,
            hk_toggle: 0,
            hk_add: 0,
            hwnd: None,
            visible: true,
            tray_click_pending: None,
            tray_ignore_click: None,
            drag_src: None,
            move_drag: None,
            resize_drag: None,
            editor: None,
            settings_open: false,
            close_prompt: false,
            toast: notice.map(|msg| (msg, Instant::now())),
            notes_drag_start: None,
            focus_note: None,
            settings_time_input: String::new(),
            reminder: ui::reminder::Reminder::new(),
            last_due_check: None,
            watch_flag,
            watch_paths,
            watch_stamps,
            ime_focus: false,
        }
    }

    pub fn st(&self) -> &Settings {
        self.core.settings()
    }

    pub fn notify(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    pub fn set_visible(&mut self, ctx: &egui::Context, v: bool) {
        if self.visible == v {
            return;
        }
        self.visible = v;
        // 根窗口的可见性 winit 0.30 不支持运行时改，只能走原生 ShowWindow
        if let Some(h) = self.hwnd {
            win32::set_visible(h, v);
        }
        ctx.request_repaint();
    }

    pub fn toggle(&mut self, ctx: &egui::Context) {
        let v = !self.visible;
        self.set_visible(ctx, v);
    }

    /// 标题栏 ×、Alt+F4 和任务栏关闭都走这里；托盘菜单的「退出」仍然直接退出。
    pub fn request_close(&mut self, ctx: &egui::Context) {
        match self.st().close_behavior {
            CloseBehavior::Ask => {
                self.close_prompt = true;
                if let Some(h) = self.hwnd {
                    win32::focus(h);
                }
                ctx.request_repaint();
            }
            CloseBehavior::Tray => self.set_visible(ctx, false),
            CloseBehavior::Quit => self.quit(),
        }
    }

    pub fn set_autostart(&mut self, enabled: bool) -> Result<(), String> {
        win32::set_autostart(enabled)?;
        self.core.set_autostart(enabled);
        Ok(())
    }

    pub fn quit(&mut self) {
        self.save_now();
        // 直接退出：托盘图标随进程消失，不留残余
        std::process::exit(0);
    }

    fn save_now(&mut self) {
        if let Err(e) = self.core.save() {
            log(&format!("任务保存失败: {e}"));
        }
        self.last_save = Instant::now();
        // 自己的写入不算外部改动，刷新基线
        self.watch_stamps = file_stamps(&self.watch_paths);
    }

    /// 命令行 / MCP 改过库之后重载。界面正有未保存修改时先不动，等保存完再来。
    fn poll_external_changes(&mut self, ctx: &egui::Context) {
        if !self.watch_flag.swap(false, Ordering::AcqRel) {
            return;
        }
        if self.core.is_dirty() {
            self.watch_flag.store(true, Ordering::Release);
            return;
        }
        let stamps = file_stamps(&self.watch_paths);
        if stamps == self.watch_stamps {
            return;
        }
        match self.core.reload_all() {
            Ok(()) => {
                self.notify("已同步外部改动");
                ctx.request_repaint();
            }
            Err(e) => log(&format!("外部改动重载失败: {e}")),
        }
        self.watch_stamps = stamps;
    }

    fn build_tray(&mut self, ctx: &egui::Context) {
        let menu = Menu::new();
        let show = MenuItem::with_id(MenuId::from("show"), "显示", true, None);
        let hide = MenuItem::with_id(MenuId::from("hide"), "隐藏", true, None);
        let sep = PredefinedMenuItem::separator();
        let quit = MenuItem::with_id(MenuId::from("quit"), "退出", true, None);
        let _ = menu.append(&show);
        let _ = menu.append(&hide);
        let _ = menu.append(&sep);
        let _ = menu.append(&quit);

        self.tray_menu = vec![
            (show.id().clone(), TrayAction::Show),
            (hide.id().clone(), TrayAction::Hide),
            (quit.id().clone(), TrayAction::Quit),
        ];

        let icon = match TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_tooltip(TITLE)
            .with_icon(tray_icon_image())
            .build()
        {
            Ok(t) => Some(t),
            Err(e) => {
                log(&format!("托盘创建失败: {e}"));
                None
            }
        };
        self.tray = icon;

        // 托盘/菜单/快捷键改走回调，主循环才能闲下来；回调只负责入队和唤醒
        let queue = self.events.clone();
        let wake = ctx.clone();
        TrayIconEvent::set_event_handler(Some(move |ev| {
            if let Ok(mut q) = queue.lock() {
                q.tray.push(ev);
            }
            wake.request_repaint();
        }));
        let queue = self.events.clone();
        let wake = ctx.clone();
        MenuEvent::set_event_handler(Some(move |ev| {
            if let Ok(mut q) = queue.lock() {
                q.menu.push(ev);
            }
            wake.request_repaint();
        }));
    }

    fn build_hotkeys(&mut self, ctx: &egui::Context) {
        let queue = self.events.clone();
        let wake = ctx.clone();
        GlobalHotKeyEvent::set_event_handler(Some(move |ev| {
            if let Ok(mut q) = queue.lock() {
                q.hotkey.push(ev);
            }
            wake.request_repaint();
        }));
        let Ok(mgr) = GlobalHotKeyManager::new() else {
            log("GlobalHotKeyManager::new 失败");
            return;
        };
        let toggle = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyB);
        let add = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyN);
        let hk_toggle = toggle.id();
        let hk_add = add.id();
        if let Err(e) = mgr.register(toggle) {
            log(&format!("注册显示/隐藏快捷键失败: {e}"));
            return;
        }
        if let Err(e) = mgr.register(add) {
            log(&format!("注册快速添加快捷键(Ctrl+Alt+N)失败: {e}"));
        }
        self.hk_toggle = hk_toggle;
        self.hk_add = hk_add;
        self.hot = Some(mgr);
    }

    fn poll_tray(&mut self, ctx: &egui::Context) {
        let (menu, tray) = match self.events.lock() {
            Ok(mut q) => (std::mem::take(&mut q.menu), std::mem::take(&mut q.tray)),
            Err(_) => return,
        };
        for ev in menu {
            match self
                .tray_menu
                .iter()
                .find(|(id, _)| *id == ev.id)
                .map(|(_, a)| *a)
            {
                Some(TrayAction::Show) => self.set_visible(ctx, true),
                Some(TrayAction::Hide) => self.set_visible(ctx, false),
                Some(TrayAction::Quit) => self.quit(),
                None => {}
            }
        }
        for ev in tray {
            match ev {
                // Click 同时有 Down/Up 两条；两条都 toggle 等于点了没反应。
                // 双击会连发两次 Up，第二次不能当成单击，先攒一拍再决定。
                TrayIconEvent::Click {
                    button,
                    button_state,
                    ..
                } if button == MouseButton::Left && button_state == MouseButtonState::Up => {
                    let now = Instant::now();
                    if let Some(until) = self.tray_ignore_click {
                        if now < until {
                            self.tray_ignore_click = None;
                            continue;
                        }
                        self.tray_ignore_click = None;
                    }
                    if !self.visible {
                        // 隐藏时收不到延迟重绘，打开必须立刻做；双击的后一条 Up 吞掉即可
                        self.tray_click_pending = None;
                        self.tray_ignore_click = Some(now + TRAY_DOUBLE_CLICK);
                        self.show_and_focus(ctx);
                        continue;
                    }
                    match self.tray_click_pending {
                        Some(first) if now.duration_since(first) <= TRAY_DOUBLE_CLICK => {
                            self.tray_click_pending = None;
                            self.tray_ignore_click = Some(now + TRAY_DOUBLE_CLICK);
                            self.show_and_focus(ctx);
                        }
                        _ => {
                            self.tray_click_pending = Some(now);
                            ctx.request_repaint_after(TRAY_DOUBLE_CLICK);
                        }
                    }
                }
                TrayIconEvent::DoubleClick { button, .. } if button == MouseButton::Left => {
                    self.tray_click_pending = None;
                    self.tray_ignore_click = Some(Instant::now() + TRAY_DOUBLE_CLICK);
                    self.show_and_focus(ctx);
                }
                _ => {}
            }
        }
        if let Some(first) = self.tray_click_pending {
            if first.elapsed() >= TRAY_DOUBLE_CLICK {
                self.tray_click_pending = None;
                if self.visible {
                    self.toggle(ctx);
                }
            } else {
                ctx.request_repaint_after(TRAY_DOUBLE_CLICK - first.elapsed());
            }
        }
    }

    fn show_and_focus(&mut self, ctx: &egui::Context) {
        self.set_visible(ctx, true);
        if let Some(h) = self.hwnd {
            win32::focus(h);
        }
        ctx.request_repaint();
    }

    fn poll_hotkeys(&mut self, ctx: &egui::Context) {
        let events = match self.events.lock() {
            Ok(mut q) => std::mem::take(&mut q.hotkey),
            Err(_) => return,
        };
        for ev in events {
            if ev.state() != global_hotkey::HotKeyState::Pressed {
                continue;
            }
            if ev.id() == self.hk_toggle {
                self.toggle(ctx);
            } else if ev.id() == self.hk_add {
                self.set_visible(ctx, true);
                if let Some(h) = self.hwnd {
                    win32::focus(h);
                }
                self.focus_todo = true;
                ctx.request_repaint();
            }
        }
    }

    /// 每帧调用；只取上一帧到现在之间跨过截止时间的任务
    fn check_due(&mut self, ctx: &egui::Context) {
        let now = chrono::Local::now().naive_local();
        let Some(last) = self.last_due_check.replace(now) else {
            return;
        };
        if now <= last || !self.st().reminders_enabled {
            return;
        }
        let hits = self.core.take_due_reminders(last, now);
        if hits.is_empty() {
            return;
        }
        let was_open = self.reminder.is_visible();
        self.reminder.notify(
            hits.into_iter()
                .map(|(id, text, due)| ui::reminder::Item { id, text, due })
                .collect(),
            self.st().reminder_seconds as u64,
        );
        if was_open {
            ctx.request_repaint_of(ui::reminder::viewport_id());
        }
        ctx.request_repaint();
    }

    fn remember_geometry(&mut self, ctx: &egui::Context) {
        if let Some(r) = ctx.input(|i| i.viewport().outer_rect) {
            self.core.set_window_pos([r.min.x, r.min.y]);
        }
        if let Some(r) = ctx.input(|i| i.viewport().inner_rect) {
            self.core.set_window_size([r.width(), r.height()]);
        }
    }
}

fn watch_paths_of(core: &Core) -> Vec<PathBuf> {
    let mut paths = core.watch_paths();
    paths.push(state::state_path());
    paths
}

fn file_stamps(paths: &[PathBuf]) -> Vec<Option<SystemTime>> {
    paths
        .iter()
        .map(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
        .collect()
}

/// 后台每秒看一次文件时间戳，有变化才唤醒界面；平时主循环仍可睡到下一分钟
fn spawn_watcher(
    ctx: egui::Context,
    paths: Vec<PathBuf>,
    mut stamps: Vec<Option<SystemTime>>,
    flag: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(900));
            let next = file_stamps(&paths);
            if next != stamps {
                stamps = next;
                flag.store(true, Ordering::Release);
                ctx.request_repaint();
            }
        }
    });
}

pub fn log(msg: &str) {
    if let Some(dir) = dirs::config_dir() {
        let path = dir.join("craboard").join("crash.log");
        let _ = std::fs::create_dir_all(dir.join("craboard"));
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(f, "{msg}");
        }
    }
}

fn tray_icon_image() -> Icon {
    Icon::from_rgba(include_bytes!("../assets/tray-32.rgba").to_vec(), 32, 32)
        .expect("图标尺寸合法")
}

fn window_icon() -> Arc<egui::IconData> {
    Arc::new(egui::IconData {
        rgba: include_bytes!("../assets/window-64.rgba").to_vec(),
        width: 64,
        height: 64,
    })
}

fn setup_fonts(ctx: &egui::Context) {
    let candidates = [
        "C:\\Windows\\Fonts\\simhei.ttf",
        "C:\\Windows\\Fonts\\Deng.ttf",
        "C:\\Windows\\Fonts\\NotoSansSC-VF.ttf",
        "C:\\Windows\\Fonts\\msyh.ttc",
    ];
    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "cjk".to_owned(),
                Arc::new(egui::FontData::from_owned(bytes)),
            );
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                if let Some(v) = fonts.families.get_mut(&family) {
                    v.push("cjk".to_owned());
                }
            }
            ctx.set_fonts(fonts);
            return;
        }
    }
    log("没找到中文字体，界面里的中文会缺字");
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.hwnd.is_none() {
            self.hwnd = win32::find_self(TITLE);
            if let Some(h) = self.hwnd {
                win32::clamp_to_screen(h);
                win32::style_window(h);
                // 从启动项或资源管理器拉起时窗口不一定会被激活；首开时第一次点击
                // 只完成激活，输入法看起来就像没生效，所以启动后主动激活一次。
                if self.visible {
                    win32::focus(h);
                }
            }
        }
        if self.tray.is_none() {
            self.build_tray(ctx);
        }
        if !self.hk_tried {
            self.hk_tried = true;
            self.build_hotkeys(ctx);
        }
        if self.hwnd.is_none() {
            self.hwnd = win32::find_self(TITLE);
        }

        self.poll_tray(ctx);
        self.poll_hotkeys(ctx);
        self.poll_external_changes(ctx);

        // winit 默认会在 CloseRequested 后退出；先取消，再交给统一关闭流程决定去向。
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.request_close(ctx);
        }

        // 进入文本输入时补关联一次 IME 上下文，缓解部分输入法首开不生效。
        let text_focus = ctx.text_edit_focused();
        if text_focus && !self.ime_focus {
            if let Some(h) = self.hwnd {
                win32::restore_ime(h);
            }
        }
        self.ime_focus = text_focus;

        let theme = self.st().theme;
        if ui::theme::active_kind() != theme {
            ui::theme::apply(ctx, theme);
            ctx.request_repaint();
        }
        let font_size = self.st().font_size;
        if (ui::font_scale() - font_size.scale()).abs() > 0.001 {
            ui::apply_font_size(ctx, font_size);
            ctx.request_repaint();
        }
        self.check_due(ctx);

        self.remember_geometry(ctx);
        if self.core.is_dirty() && self.last_save.elapsed() > Duration::from_millis(400) {
            self.save_now();
        }

        // 平时睡到下一分钟；秒显、等窗口句柄、防抖存盘才提前醒。
        // 托盘和快捷键事件由各自的 handler 直接 request_repaint，不再靠轮询。
        let mut wait = if self.hwnd.is_none() {
            Duration::from_millis(100)
        } else if self.st().show_seconds {
            Duration::from_millis(250)
        } else {
            use chrono::Timelike;
            let to_minute = 60 - chrono::Local::now().second() as u64;
            Duration::from_secs(to_minute) + Duration::from_millis(30)
        };
        if self.core.is_dirty() {
            let save_due = Duration::from_millis(400).saturating_sub(self.last_save.elapsed());
            wait = wait.min(save_due + Duration::from_millis(20));
        }
        if self.reminder.is_visible() || self.reminder.needs_root_paint() {
            // 主窗即使收进托盘也要持续出帧：deferred 子窗口每次都要由父窗重新登记。
            // 顺便保证提醒窗自己关闭后能在一个刷新周期内被回收。
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaintWhileHidden);
            wait = wait.min(Duration::from_millis(200));
        }
        ctx.request_repaint_after(wait);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.reminder.is_visible() {
            let pos = self.reminder.origin(self.hwnd, ui.ctx().pixels_per_point());
            self.reminder.show(ui.ctx(), pos);
        }
        self.reminder.clear_root_paint();
        ui::board::draw(self, ui);
        ui::close::confirm(self, ui.ctx());
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let bg = ui::theme::active().bg;
        [
            bg.r() as f32 / 255.0,
            bg.g() as f32 / 255.0,
            bg.b() as f32 / 255.0,
            1.0,
        ]
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_now();
    }
}

fn main() -> eframe::Result {
    let (core, warning) = Core::open_local();
    if let Some(w) = warning {
        log(&format!("任务存储: {w}"));
    }
    let (experimental, exp_warning) = Experimental::load();
    if let Some(w) = &exp_warning {
        log(w);
    }
    let [x, y, w, h] = {
        let settings = core.settings();
        [
            settings.pos[0],
            settings.pos[1],
            settings.size[0],
            settings.size[1],
        ]
    };
    let viewport = egui::ViewportBuilder::default()
        .with_title(TITLE)
        .with_app_id(TITLE)
        .with_icon(window_icon())
        .with_decorations(false)
        .with_position([x, y])
        .with_inner_size([w, h])
        .with_resizable(true)
        // 再矮今日进度就会压住左侧导航
        .with_min_inner_size([260.0, 360.0]);

    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        TITLE,
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, core, experimental, exp_warning)))),
    )
}
