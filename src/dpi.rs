//! Logical UI coordinates; screenshot coordinates remain physical pixels.
use super::*;
#[derive(Default)]
pub struct Layout {
    children: Vec<(HWND, Region)>,
}
pub fn scale(value: i32, dpi: u32) -> i32 {
    ((value as i64 * dpi as i64 + 48) / 96) as i32
}
pub fn logical(value: i32, dpi: u32) -> i32 {
    ((value as i64 * 96 + dpi as i64 / 2) / dpi.max(1) as i64) as i32
}
pub unsafe fn current(hwnd: HWND) -> u32 {
    GetDpiForWindow(hwnd).max(96)
}
unsafe extern "system" fn collect(hwnd: HWND, data: isize) -> i32 {
    let children = &mut *(data as *mut Vec<(HWND, Region)>);
    let mut r: RECT = zeroed();
    GetWindowRect(hwnd, &mut r);
    MapWindowPoints(0, GetParent(hwnd), &mut r as *mut RECT as *mut POINT, 2);
    children.push((
        hwnd,
        Region {
            x: r.left,
            y: r.top,
            w: r.right - r.left,
            h: r.bottom - r.top,
        },
    ));
    1
}
impl Layout {
    pub unsafe fn capture(&mut self, hwnd: HWND) {
        self.children.clear();
        EnumChildWindows(hwnd, Some(collect), &mut self.children as *mut _ as isize);
    }
    pub unsafe fn apply(
        &self,
        hwnd: HWND,
        font: &mut HFONT,
        size: (i32, i32),
        dpi: u32,
        suggested: Option<RECT>,
    ) {
        let next = CreateFontW(
            -scale(13, dpi),
            0,
            0,
            0,
            FW_NORMAL as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            0,
            0,
            CLEARTYPE_QUALITY as u32,
            0,
            wide("Microsoft YaHei UI").as_ptr(),
        );
        if next != 0 {
            for (child, _) in &self.children {
                SendMessageW(*child, WM_SETFONT, next as usize, 0);
            }
            DeleteObject(*font);
            *font = next;
        }
        for (child, r) in &self.children {
            SetWindowPos(
                *child,
                0,
                scale(r.x, dpi),
                scale(r.y, dpi),
                scale(r.w, dpi),
                scale(r.h, dpi),
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        let mut r = RECT {
            left: 0,
            top: 0,
            right: scale(size.0, dpi),
            bottom: scale(size.1, dpi),
        };
        AdjustWindowRectExForDpi(
            &mut r,
            GetWindowLongW(hwnd, GWL_STYLE) as u32,
            0,
            GetWindowLongW(hwnd, GWL_EXSTYLE) as u32,
            dpi,
        );
        let (x, y, flags) = suggested
            .map(|r| (r.left, r.top, 0))
            .unwrap_or((0, 0, SWP_NOMOVE));
        SetWindowPos(
            hwnd,
            0,
            x,
            y,
            r.right - r.left,
            r.bottom - r.top,
            flags | SWP_NOZORDER | SWP_NOACTIVATE,
        );
        InvalidateRect(hwnd, null(), 0);
    }
}
pub unsafe fn map(dc: HDC, dpi: u32) -> i32 {
    let saved = SaveDC(dc);
    SetMapMode(dc, MM_ANISOTROPIC);
    SetWindowExtEx(dc, 96, 96, std::ptr::null_mut());
    SetViewportExtEx(dc, dpi as i32, dpi as i32, std::ptr::null_mut());
    saved
}
pub unsafe fn paint(hwnd: HWND, dc: HDC, draw: impl FnOnce(HDC)) {
    let mut r: RECT = zeroed();
    GetClientRect(hwnd, &mut r);
    if let Ok(buffer) = Bitmap::new(r.right, r.bottom) {
        let saved = map(buffer.dc, current(hwnd));
        draw(buffer.dc);
        RestoreDC(buffer.dc, saved);
        BitBlt(dc, 0, 0, r.right, r.bottom, buffer.dc, 0, 0, SRCCOPY);
    } else {
        let saved = map(dc, current(hwnd));
        draw(dc);
        RestoreDC(dc, saved);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_dpi_changes_keep_control_geometry_and_font_consistent() {
        unsafe {
            let hwnd = CreateWindowExW(
                0,
                wide("STATIC").as_ptr(),
                wide("").as_ptr(),
                WS_POPUP,
                0,
                0,
                704,
                480,
                0,
                0,
                GetModuleHandleW(null()),
                std::ptr::null_mut(),
            );
            assert_ne!(hwnd, 0);
            let edit = CreateWindowExW(
                0,
                wide("EDIT").as_ptr(),
                wide("Ctrl+Alt+A").as_ptr(),
                WS_CHILD,
                442,
                128,
                222,
                20,
                hwnd,
                10,
                GetModuleHandleW(null()),
                std::ptr::null_mut(),
            );
            assert_ne!(edit, 0);
            let mut layout = Layout::default();
            layout.capture(hwnd);
            let mut font = 0;
            for dpi in [144, 192, 120, 96] {
                layout.apply(hwnd, &mut font, (704, 480), dpi, None);
                let mut r: RECT = zeroed();
                GetWindowRect(edit, &mut r);
                MapWindowPoints(0, hwnd, &mut r as *mut RECT as *mut POINT, 2);
                assert_eq!(
                    (r.left, r.top, r.right - r.left, r.bottom - r.top),
                    (
                        scale(442, dpi),
                        scale(128, dpi),
                        scale(222, dpi),
                        scale(20, dpi)
                    )
                );
                let mut lf: LOGFONTW = zeroed();
                assert_ne!(
                    GetObjectW(
                        font,
                        size_of::<LOGFONTW>() as i32,
                        &mut lf as *mut _ as *mut _
                    ),
                    0
                );
                assert_eq!(lf.lfHeight, -scale(13, dpi));
            }
            DestroyWindow(hwnd);
            DeleteObject(font);
        }
    }
}
