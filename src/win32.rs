//! 需要的少量 Win32 调用：窗口查找、可见性、屏幕边界与开机启动项。

#![allow(non_snake_case)]

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;

pub type HWND = *mut c_void;
type HKEY = *mut c_void;

const SWP_NOACTIVATE: u32 = 0x0010;
const SW_HIDE: i32 = 0;
/// 普通应用：显示时正常激活并进入前台
const SW_SHOW: i32 = 5;

const HKEY_CURRENT_USER: HKEY = 0x8000_0001usize as HKEY;
const KEY_SET_VALUE: u32 = 0x0002;
const REG_SZ: u32 = 1;
const ERROR_FILE_NOT_FOUND: i32 = 2;
const ERROR_PATH_NOT_FOUND: i32 = 3;
const AUTOSTART_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const AUTOSTART_VALUE: &str = "Craboard";

#[link(name = "user32")]
unsafe extern "system" {
    fn FindWindowW(class: *const u16, title: *const u16) -> HWND;
    fn SetWindowPos(h: HWND, after: HWND, x: i32, y: i32, w: i32, hh: i32, flags: u32) -> i32;
    fn GetWindowRect(h: HWND, r: *mut RECT) -> i32;
    fn GetWindowThreadProcessId(h: HWND, pid: *mut u32) -> u32;
    fn ShowWindow(h: HWND, cmd: i32) -> i32;
    fn GetSystemMetrics(idx: i32) -> i32;
    fn SetForegroundWindow(h: HWND) -> i32;
    fn GetCursorPos(p: *mut POINT) -> i32;
    fn MonitorFromWindow(h: HWND, flags: u32) -> HWND;
    fn GetMonitorInfoW(h: HWND, info: *mut MONITORINFO) -> i32;
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmSetWindowAttribute(h: HWND, attr: u32, value: *const u32, size: u32) -> i32;
}

#[link(name = "imm32")]
unsafe extern "system" {
    fn ImmAssociateContextEx(hwnd: HWND, himc: *mut c_void, flags: u32) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcessId() -> u32;
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn RegCreateKeyExW(
        hkey: HKEY,
        sub_key: *const u16,
        reserved: u32,
        class: *const u16,
        options: u32,
        sam: u32,
        security: *mut c_void,
        result: *mut HKEY,
        disposition: *mut u32,
    ) -> i32;
    fn RegOpenKeyExW(
        hkey: HKEY,
        sub_key: *const u16,
        options: u32,
        sam: u32,
        result: *mut HKEY,
    ) -> i32;
    fn RegSetValueExW(
        hkey: HKEY,
        value_name: *const u16,
        reserved: u32,
        ty: u32,
        data: *const u8,
        size: u32,
    ) -> i32;
    fn RegDeleteValueW(hkey: HKEY, value_name: *const u16) -> i32;
    fn RegCloseKey(hkey: HKEY) -> i32;
}

fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// 按标题找我们自己的窗口，并用进程号复核，避免误伤同名窗口
pub fn find_self(title: &str) -> Option<HWND> {
    unsafe {
        let t = wide(title);
        let h = FindWindowW(std::ptr::null(), t.as_ptr());
        if h.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, &mut pid);
        if pid == GetCurrentProcessId() {
            Some(h)
        } else {
            None
        }
    }
}

/// 显示器分辨率会变（本机实测同一会话里从 3840x2160 变成 1920x1200），
/// 存过的窗口位置可能整个跑到屏幕外，启动时按虚拟桌面边界拉回来。
pub fn clamp_to_screen(hwnd: HWND) {
    const SM_XVIRTUALSCREEN: i32 = 76;
    const SM_YVIRTUALSCREEN: i32 = 77;
    const SM_CXVIRTUALSCREEN: i32 = 78;
    const SM_CYVIRTUALSCREEN: i32 = 79;
    unsafe {
        let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        let mut r = RECT::default();
        GetWindowRect(hwnd, &mut r);
        let w = r.right - r.left;
        let h = r.bottom - r.top;
        let overlaps = r.right > vx && r.left < vx + vw && r.bottom > vy && r.top < vy + vh;
        if !overlaps {
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                vx + 40,
                vy + 40,
                w,
                h,
                SWP_NOACTIVATE,
            );
        }
    }
}

#[repr(C)]
#[derive(Default)]
struct RECT {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
#[derive(Default)]
struct POINT {
    x: i32,
    y: i32,
}

/// 取窗口所在显示器的工作区（物理像素，已扣掉任务栏）
pub fn work_area(hwnd: HWND) -> Option<(i32, i32, i32, i32)> {
    const MONITOR_DEFAULTTONEAREST: u32 = 2;
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        if monitor.is_null() {
            return None;
        }
        let mut info = MONITORINFO {
            cb_size: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return None;
        }
        let r = info.rc_work;
        Some((r.left, r.top, r.right, r.bottom))
    }
}

#[repr(C)]
#[derive(Default)]
struct MONITORINFO {
    cb_size: u32,
    rc_monitor: RECT,
    rc_work: RECT,
    dw_flags: u32,
}

const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
const DWMWCP_ROUND: u32 = 2;

/// 暗色标题栏 + Win11 圆角。客户区完全由 egui 画，不依赖 DWM 的背景板。
pub fn style_window(hwnd: HWND) {
    unsafe {
        let dark = 1u32;
        DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark, 4);
        let corner = DWMWCP_ROUND;
        DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &corner, 4);
    }
}

/// 物理屏幕像素。移动/缩放只能按它算增量：窗口内坐标会被窗口自身的位移抵消
pub fn cursor_pos() -> Option<(f32, f32)> {
    unsafe {
        let mut p = POINT::default();
        (GetCursorPos(&mut p) != 0).then_some((p.x as f32, p.y as f32))
    }
}

/// 用户主动按快捷键时要把键盘焦点真正给过来，否则输入框收不到字
pub fn focus(hwnd: HWND) -> bool {
    unsafe { SetForegroundWindow(hwnd) != 0 }
}

/// 部分输入法在窗口第一次进入文本输入时没有可用的 IME 上下文，
/// 表现为中文打不出来、切到别的窗口再回来又正常。重新关联默认上下文成本很低，
/// 每次从非输入状态进入文本输入时补一次。
pub fn restore_ime(hwnd: HWND) {
    const IACE_DEFAULT: u32 = 0x0010;
    unsafe {
        ImmAssociateContextEx(hwnd, std::ptr::null_mut(), IACE_DEFAULT);
    }
}

pub fn set_visible(hwnd: HWND, visible: bool) {
    unsafe { ShowWindow(hwnd, if visible { SW_SHOW } else { SW_HIDE }) };
}

/// 登录 Windows 后自动启动。写当前用户的 Run 项，不需要管理员权限。
pub fn set_autostart(enabled: bool) -> Result<(), String> {
    let key_path = wide(AUTOSTART_KEY);
    let value_name = wide(AUTOSTART_VALUE);
    if enabled {
        let exe = std::env::current_exe().map_err(|e| format!("读取程序路径失败：{e}"))?;
        let command = wide(&format!("\"{}\"", exe.display()));
        let bytes = unsafe {
            std::slice::from_raw_parts(
                command.as_ptr().cast::<u8>(),
                command.len() * std::mem::size_of::<u16>(),
            )
        };
        unsafe {
            let mut key: HKEY = std::ptr::null_mut();
            let rc = RegCreateKeyExW(
                HKEY_CURRENT_USER,
                key_path.as_ptr(),
                0,
                std::ptr::null(),
                0,
                KEY_SET_VALUE,
                std::ptr::null_mut(),
                &mut key,
                std::ptr::null_mut(),
            );
            if rc != 0 {
                return Err(reg_error("打开启动项", rc));
            }
            let rc = RegSetValueExW(
                key,
                value_name.as_ptr(),
                0,
                REG_SZ,
                bytes.as_ptr(),
                bytes.len() as u32,
            );
            RegCloseKey(key);
            if rc != 0 {
                return Err(reg_error("写入启动项", rc));
            }
        }
    } else {
        unsafe {
            let mut key: HKEY = std::ptr::null_mut();
            let rc = RegOpenKeyExW(
                HKEY_CURRENT_USER,
                key_path.as_ptr(),
                0,
                KEY_SET_VALUE,
                &mut key,
            );
            if rc == ERROR_FILE_NOT_FOUND || rc == ERROR_PATH_NOT_FOUND {
                return Ok(());
            }
            if rc != 0 {
                return Err(reg_error("打开启动项", rc));
            }
            let rc = RegDeleteValueW(key, value_name.as_ptr());
            RegCloseKey(key);
            if rc != 0 && rc != ERROR_FILE_NOT_FOUND {
                return Err(reg_error("删除启动项", rc));
            }
        }
    }
    Ok(())
}

fn reg_error(action: &str, code: i32) -> String {
    format!(
        "{action}失败：{}（错误码 {code}）",
        std::io::Error::from_raw_os_error(code)
    )
}
