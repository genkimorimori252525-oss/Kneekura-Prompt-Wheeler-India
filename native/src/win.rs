//! Thin Win32 helpers. Everything here works in physical pixels.
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HWND, POINT, RECT, SIZE};
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetMonitorInfoW, MonitorFromWindow, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, GetWindowRect, SetWindowLongPtrW, SetWindowPos, ShowWindow, UpdateLayeredWindow, GWL_EXSTYLE, HWND_NOTOPMOST,
    HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_SHOWNOACTIVATE, ULW_ALPHA, WS_EX_LAYERED,
};

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// [left, top, right, bottom] of the window on the virtual screen.
pub fn window_rect(hwnd: isize) -> Option<[i32; 4]> {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    (unsafe { GetWindowRect(hwnd as HWND, &mut r) } != 0).then_some([r.left, r.top, r.right, r.bottom])
}

/// Work area (screen minus taskbar) of the monitor the window is mostly on.
pub fn work_area(hwnd: isize) -> Option<[i32; 4]> {
    unsafe {
        let monitor = MonitorFromWindow(hwnd as HWND, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return None;
        }
        let w = info.rcWork;
        Some([w.left, w.top, w.right, w.bottom])
    }
}

pub fn move_to(hwnd: isize, x: i32, y: i32) {
    unsafe {
        SetWindowPos(hwnd as HWND, null_mut(), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

pub fn move_resize(hwnd: isize, x: i32, y: i32, w: i32, h: i32) {
    unsafe {
        SetWindowPos(hwnd as HWND, null_mut(), x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
    }
}



/// Cursor position on the virtual screen.
pub fn cursor_pos() -> Option<(i32, i32)> {
    let mut p = POINT { x: 0, y: 0 };
    (unsafe { GetCursorPos(&mut p) } != 0).then_some((p.x, p.y))
}

/// Returns false when another copy of the app already holds the named mutex.
pub fn claim_single_instance(name: &str) -> bool {
    let name = wide(name);
    unsafe {
        let handle = CreateMutexW(null(), 0, name.as_ptr());
        // The handle is deliberately leaked: it must live as long as the process.
        !handle.is_null() && GetLastError() != ERROR_ALREADY_EXISTS
    }
}

pub fn set_topmost(hwnd: isize, on: bool) {
    unsafe {
        SetWindowPos(hwnd as HWND, if on { HWND_TOPMOST } else { HWND_NOTOPMOST }, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// Mark the window as layered (per-pixel alpha). Safe to call repeatedly.
pub fn make_layered(hwnd: isize) {
    unsafe {
        let style = GetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE);
        if style & WS_EX_LAYERED as isize == 0 {
            SetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE, style | WS_EX_LAYERED as isize);
        }
    }
}

/// Show without stealing keyboard focus from whatever the user is typing in.
pub fn show_no_activate(hwnd: isize) {
    unsafe {
        ShowWindow(hwnd as HWND, SW_SHOWNOACTIVATE);
    }
}

/// A per-pixel-alpha surface for a window: a 32-bit bitmap we paint into and hand to the
/// compositor with `UpdateLayeredWindow`. Pixels with alpha 0 are see-through and click-through.
pub struct Layered {
    hwnd: isize,
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *mut [u8; 4],
    width: usize,
    height: usize,
}

impl Layered {
    pub fn new(hwnd: isize) -> Self {
        Self { hwnd, dc: unsafe { CreateCompatibleDC(null_mut()) }, bitmap: null_mut(), previous: null_mut(), bits: null_mut(), width: 0, height: 0 }
    }

    /// Premultiplied BGRA pixels, top row first. Reallocates only when the size changes.
    pub fn pixels(&mut self, width: usize, height: usize) -> &mut [[u8; 4]] {
        if width != self.width || height != self.height || self.bits.is_null() {
            unsafe {
                if !self.bitmap.is_null() {
                    SelectObject(self.dc, self.previous);
                    DeleteObject(self.bitmap);
                }
                let mut info: BITMAPINFO = std::mem::zeroed();
                info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
                info.bmiHeader.biWidth = width as i32;
                info.bmiHeader.biHeight = -(height as i32); // negative = top-down
                info.bmiHeader.biPlanes = 1;
                info.bmiHeader.biBitCount = 32;
                let mut bits: *mut core::ffi::c_void = null_mut();
                self.bitmap = CreateDIBSection(self.dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
                self.previous = SelectObject(self.dc, self.bitmap);
                self.bits = bits.cast();
                self.width = width;
                self.height = height;
            }
        }
        if self.bits.is_null() {
            return &mut [];
        }
        unsafe { std::slice::from_raw_parts_mut(self.bits, self.width * self.height) }
    }

    /// Hand the bitmap to the compositor. `alpha` fades the whole window on top of the per-pixel alpha.
    pub fn present(&self, alpha: u8) {
        if self.bits.is_null() {
            return;
        }
        unsafe {
            let hwnd = self.hwnd as HWND;
            // The layered bit can be dropped when the windowing library rewrites styles; put it back.
            make_layered(self.hwnd);
            let size = SIZE { cx: self.width as i32, cy: self.height as i32 };
            let origin = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION { BlendOp: 0, BlendFlags: 0, SourceConstantAlpha: alpha, AlphaFormat: 1 };
            UpdateLayeredWindow(hwnd, null_mut(), null(), &size, self.dc, &origin, 0, &blend, ULW_ALPHA);
        }
    }
}

impl Drop for Layered {
    fn drop(&mut self) {
        unsafe {
            if !self.bitmap.is_null() {
                SelectObject(self.dc, self.previous);
                DeleteObject(self.bitmap);
            }
            DeleteDC(self.dc);
        }
    }
}
