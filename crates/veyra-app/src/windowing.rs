use eframe::egui::{Pos2, Vec2};

#[cfg(windows)]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
#[cfg(windows)]
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{HWND, POINT, RECT},
    Graphics::Dwm::{
        DWMSBT_MAINWINDOW, DWMSBT_NONE, DWMSBT_TRANSIENTWINDOW, DWMWA_SYSTEMBACKDROP_TYPE,
        DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUNDSMALL,
        DwmSetWindowAttribute,
    },
    Graphics::Gdi::{
        GetMonitorInfoW, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
        MonitorFromWindow,
    },
    UI::{
        HiDpi::{
            DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor,
            GetDpiForWindow, MDT_EFFECTIVE_DPI, SetProcessDpiAwarenessContext,
            SetThreadDpiAwarenessContext,
        },
        WindowsAndMessaging::{
            FindWindowW, GetCursorPos, GetWindowRect, SW_SHOWNORMAL, SWP_NOACTIVATE, SWP_NOSIZE,
            SWP_NOZORDER, SWP_SHOWWINDOW, SetForegroundWindow, SetWindowPos, ShowWindow,
        },
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowLayoutMode {
    LauncherCompact,
    LauncherResults,
    LauncherAi,
    Settings,
}

#[cfg(windows)]
static WINDOW_DPI: AtomicU32 = AtomicU32::new(96);
#[cfg(windows)]
static WINDOW_HWND: AtomicIsize = AtomicIsize::new(0);
#[cfg(windows)]
static WINDOW_TARGET_MONITOR: AtomicIsize = AtomicIsize::new(0);

#[cfg(windows)]
pub(crate) fn configure_process_dpi_awareness() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

#[cfg(not(windows))]
pub(crate) fn configure_process_dpi_awareness() {}

#[cfg(windows)]
fn native_enter_dpi_context() -> DPI_AWARENESS_CONTEXT {
    unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) }
}

#[cfg(windows)]
fn native_restore_dpi_context(previous: DPI_AWARENESS_CONTEXT) {
    if !previous.is_null() {
        unsafe {
            let _ = SetThreadDpiAwarenessContext(previous);
        }
    }
}

#[cfg(windows)]
pub(crate) fn apply_native_backdrop(cc: &eframe::CreationContext<'_>) {
    let Ok(handle) = cc.window_handle() else {
        return;
    };

    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };

    let hwnd = handle.hwnd.get() as HWND;
    WINDOW_HWND.store(hwnd as isize, Ordering::Relaxed);
    unsafe {
        let dpi = GetDpiForWindow(hwnd);
        if dpi >= 96 {
            WINDOW_DPI.store(dpi, Ordering::Relaxed);
        }

        let dark_mode = 1_i32;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            std::ptr::addr_of!(dark_mode).cast(),
            std::mem::size_of_val(&dark_mode) as u32,
        );

        let corner = DWMWCP_ROUNDSMALL;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            std::ptr::addr_of!(corner).cast(),
            std::mem::size_of_val(&corner) as u32,
        );

        let backdrop = DWMSBT_TRANSIENTWINDOW;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE as u32,
            std::ptr::addr_of!(backdrop).cast(),
            std::mem::size_of_val(&backdrop) as u32,
        );
    }
}

#[cfg(not(windows))]
pub(crate) fn apply_native_backdrop(_cc: &eframe::CreationContext<'_>) {}

#[cfg(windows)]
pub(crate) fn apply_native_backdrop_for_config(config: &veyra_core::config::VeyraConfig) {
    let hwnd = native_veyra_hwnd();
    if hwnd.is_null() {
        return;
    }

    let backdrop = if config.appearance.blur {
        match config.appearance.theme.as_str() {
            "dark-acrylic" => DWMSBT_TRANSIENTWINDOW,
            "dark-compact" => DWMSBT_MAINWINDOW,
            _ => DWMSBT_TRANSIENTWINDOW,
        }
    } else {
        DWMSBT_NONE
    };

    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE as u32,
            std::ptr::addr_of!(backdrop).cast(),
            std::mem::size_of_val(&backdrop) as u32,
        );
    }
}

#[cfg(not(windows))]
pub(crate) fn apply_native_backdrop_for_config(_config: &veyra_core::config::VeyraConfig) {}

pub(crate) fn min_window_size(mode: WindowLayoutMode) -> Vec2 {
    match mode {
        WindowLayoutMode::LauncherCompact => Vec2::new(520.0, 76.0),
        WindowLayoutMode::LauncherResults => Vec2::new(560.0, 120.0),
        WindowLayoutMode::LauncherAi => Vec2::new(560.0, 280.0),
        WindowLayoutMode::Settings => Vec2::new(560.0, 360.0),
    }
}

pub(crate) fn effective_layout_scale(pixels_per_point: f32) -> f32 {
    pixels_per_point.max(native_window_scale()).max(1.0)
}

#[cfg(windows)]
fn native_window_scale() -> f32 {
    let hwnd = native_veyra_hwnd();
    if !hwnd.is_null() {
        unsafe {
            let previous_dpi_context = native_enter_dpi_context();
            if let Some(monitor) = native_layout_monitor(hwnd) {
                let scale = native_monitor_scale(monitor);
                if scale > 0.0 {
                    native_restore_dpi_context(previous_dpi_context);
                    return scale;
                }
            }

            let dpi = GetDpiForWindow(hwnd);
            if dpi >= 96 {
                WINDOW_DPI.store(dpi, Ordering::Relaxed);
                native_restore_dpi_context(previous_dpi_context);
                return dpi as f32 / 96.0;
            }
            native_restore_dpi_context(previous_dpi_context);
        }
    }

    WINDOW_DPI.load(Ordering::Relaxed).max(96) as f32 / 96.0
}

#[cfg(windows)]
fn native_monitor_scale(monitor: HMONITOR) -> f32 {
    unsafe {
        let mut dpi_x = 0_u32;
        let mut dpi_y = 0_u32;
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) == 0 && dpi_x >= 96
        {
            WINDOW_DPI.store(dpi_x, Ordering::Relaxed);
            return dpi_x as f32 / 96.0;
        }
    }

    0.0
}

#[cfg(windows)]
fn native_veyra_hwnd() -> HWND {
    const VEYRA_TITLE: [u16; 6] = [
        'V' as u16, 'e' as u16, 'y' as u16, 'r' as u16, 'a' as u16, 0,
    ];

    unsafe {
        let hwnd = FindWindowW(std::ptr::null(), VEYRA_TITLE.as_ptr());
        if !hwnd.is_null() {
            WINDOW_HWND.store(hwnd as isize, Ordering::Relaxed);
            return hwnd;
        }
    }

    WINDOW_HWND.load(Ordering::Relaxed) as HWND
}

#[cfg(not(windows))]
fn native_window_scale() -> f32 {
    1.0
}

const LAUNCHER_WIDTH: f32 = 680.0;
const COMPACT_HEIGHT: f32 = 76.0;
const RESULT_ROW_HEIGHT: f32 = 58.0;
const RESULTS_BASE_HEIGHT: f32 = 90.0;
const MAX_RESULTS_HEIGHT: f32 = 520.0;

pub(crate) fn window_size_for_monitor(
    mode: WindowLayoutMode,
    monitor_size: Vec2,
    _pixels_per_point: f32,
    result_count: usize,
) -> Vec2 {
    let logical_monitor = monitor_size;
    let max_width = (logical_monitor.x - 48.0).max(360.0);
    let max_height = (logical_monitor.y - 96.0).max(240.0);

    let logical_size = match mode {
        WindowLayoutMode::LauncherCompact => {
            Vec2::new(LAUNCHER_WIDTH.clamp(540.0, max_width), COMPACT_HEIGHT)
        }
        WindowLayoutMode::LauncherResults => {
            let results_height = RESULTS_BASE_HEIGHT + result_count as f32 * RESULT_ROW_HEIGHT;
            Vec2::new(
                LAUNCHER_WIDTH.clamp(540.0, max_width),
                results_height.clamp(120.0, MAX_RESULTS_HEIGHT),
            )
        }
        WindowLayoutMode::LauncherAi => Vec2::new(
            LAUNCHER_WIDTH.clamp(560.0, max_width),
            (logical_monitor.y * 0.68).clamp(320.0, 560.0),
        ),
        WindowLayoutMode::Settings => Vec2::new(
            (logical_monitor.x * 0.72).clamp(560.0, 840.0),
            (logical_monitor.y * 0.72).clamp(420.0, 720.0),
        ),
    };

    Vec2::new(
        logical_size.x.min(max_width),
        logical_size.y.min(max_height),
    )
}

pub(crate) fn layout_size_matches(left: Vec2, right: Vec2) -> bool {
    (left.x - right.x).abs() < 0.5 && (left.y - right.y).abs() < 0.5
}

#[cfg_attr(windows, allow(dead_code))]
pub(crate) fn window_position(
    mode: WindowLayoutMode,
    monitor_size: Vec2,
    window_size: Vec2,
    _pixels_per_point: f32,
) -> Pos2 {
    let logical_monitor = monitor_size;
    let (vertical_factor, top_padding) = match mode {
        WindowLayoutMode::LauncherCompact | WindowLayoutMode::LauncherResults => (0.0, 96.0),
        WindowLayoutMode::LauncherAi => (0.0, 80.0),
        WindowLayoutMode::Settings => (0.5, 0.0),
    };
    Pos2::new(
        ((logical_monitor.x - window_size.x) / 2.0).max(0.0),
        ((logical_monitor.y - window_size.y) * vertical_factor + top_padding).max(0.0),
    )
}

#[cfg(windows)]
pub(crate) fn native_center_window(mode: WindowLayoutMode) {
    let hwnd = native_veyra_hwnd();
    if hwnd.is_null() {
        return;
    }

    let (vertical_factor, top_padding) = match mode {
        WindowLayoutMode::LauncherCompact | WindowLayoutMode::LauncherResults => (0.0, 96.0),
        WindowLayoutMode::LauncherAi => (0.0, 80.0),
        WindowLayoutMode::Settings => (0.5, 0.0),
    };

    unsafe {
        let previous_dpi_context = native_enter_dpi_context();
        let Some(monitor) = native_layout_monitor(hwnd) else {
            native_restore_dpi_context(previous_dpi_context);
            return;
        };

        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if GetWindowRect(hwnd, &mut rect) == 0 {
            native_restore_dpi_context(previous_dpi_context);
            return;
        }
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        if width <= 0 || height <= 0 {
            native_restore_dpi_context(previous_dpi_context);
            return;
        }

        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            rcMonitor: std::mem::zeroed(),
            rcWork: std::mem::zeroed(),
            dwFlags: 0,
        };
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            native_restore_dpi_context(previous_dpi_context);
            return;
        }

        let scale = native_monitor_scale(monitor).max(1.0);
        let work_left = info.rcWork.left;
        let work_top = info.rcWork.top;
        let work_right = info.rcWork.right;
        let work_bottom = info.rcWork.bottom;
        let work_width = work_right - work_left;
        let work_height = work_bottom - work_top;
        let left = work_left + ((work_width - width).max(0) / 2);
        let top = work_top
            + (((work_height - height).max(0) as f32) * vertical_factor) as i32
            + (top_padding * scale) as i32;
        let _ = SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            left,
            top,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        native_restore_dpi_context(previous_dpi_context);
    }
}

#[cfg(not(windows))]
pub(crate) fn native_center_window(_mode: WindowLayoutMode) {}

#[cfg(windows)]
pub(crate) fn native_monitor_logical_size(scale: f32) -> Option<Vec2> {
    let hwnd = native_veyra_hwnd();
    if hwnd.is_null() {
        return None;
    }

    unsafe {
        let previous_dpi_context = native_enter_dpi_context();
        let Some(monitor) = native_layout_monitor(hwnd) else {
            native_restore_dpi_context(previous_dpi_context);
            return None;
        };
        let scale = scale.max(native_monitor_scale(monitor)).max(1.0);

        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            rcMonitor: std::mem::zeroed(),
            rcWork: std::mem::zeroed(),
            dwFlags: 0,
        };
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            native_restore_dpi_context(previous_dpi_context);
            return None;
        }

        let width = (info.rcWork.right - info.rcWork.left) as f32 / scale;
        let height = (info.rcWork.bottom - info.rcWork.top) as f32 / scale;
        native_restore_dpi_context(previous_dpi_context);
        (width > 0.0 && height > 0.0).then_some(Vec2::new(width, height))
    }
}

#[cfg(not(windows))]
pub(crate) fn native_monitor_logical_size(_scale: f32) -> Option<Vec2> {
    None
}

#[cfg(windows)]
pub(crate) fn native_capture_target_monitor() {
    unsafe {
        let mut point = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut point) == 0 {
            return;
        }

        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
        if !monitor.is_null() {
            WINDOW_TARGET_MONITOR.store(monitor as isize, Ordering::Relaxed);
        }
    }
}

#[cfg(not(windows))]
pub(crate) fn native_capture_target_monitor() {}

#[cfg(windows)]
fn native_layout_monitor(hwnd: HWND) -> Option<HMONITOR> {
    unsafe {
        let target = WINDOW_TARGET_MONITOR.load(Ordering::Relaxed) as HMONITOR;
        if !target.is_null() {
            return Some(target);
        }

        let mut point = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut point) != 0 {
            let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
            if !monitor.is_null() {
                WINDOW_TARGET_MONITOR.store(monitor as isize, Ordering::Relaxed);
                return Some(monitor);
            }
        }

        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        (!monitor.is_null()).then_some(monitor)
    }
}

#[cfg(windows)]
pub(crate) fn native_show_launcher_window() {
    let hwnd = native_veyra_hwnd();
    if hwnd.is_null() {
        return;
    }

    unsafe {
        ShowWindow(hwnd, SW_SHOWNORMAL);
        SetForegroundWindow(hwnd);
    }
}

#[cfg(not(windows))]
pub(crate) fn native_show_launcher_window() {}
