#[path = "color.rs"]
mod color;
#[path = "dpi.rs"]
mod dpi;
#[path = "export.rs"]
mod export;
#[path = "render.rs"]
mod render;
#[path = "settings.rs"]
mod settings;
#[path = "tools.rs"]
mod tools;
#[path = "ui.rs"]
mod ui;
#[path = "window.rs"]
mod window;
use crate::geometry::{Point, Region};
use render::{output, render};
use std::{
    mem::{size_of, zeroed},
    ptr::null,
};
use tools::Tool;
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{DataExchange::*, LibraryLoader::*, Memory::*},
    UI::{
        Controls::Dialogs::*, HiDpi::*, Input::KeyboardAndMouse::*, Shell::*,
        WindowsAndMessaging::*,
    },
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
// Win32 MAKEINTRESOURCE: this value is an integer resource ID, never dereferenced.
const fn resource_id(id: u16) -> *const u16 {
    id as usize as *const u16
}
const TRAY: u32 = WM_APP + 1;
const WM_APP_SETTINGS: u32 = WM_APP + 2;
const CLASS: &str = "Qingjie.Native";
const PIN_CLASS: &str = "Qingjie.Pin";
struct Bitmap {
    dc: HDC,
    handle: HBITMAP,
    old: HGDIOBJ,
    w: i32,
    h: i32,
}
impl Bitmap {
    unsafe fn new(w: i32, h: i32) -> Result<Self, String> {
        if w <= 0 || h <= 0 || (w as i64) * (h as i64) > 150_000_000 {
            return Err("截图尺寸无效或过大".into());
        }
        let screen = GetDC(0);
        let dc = CreateCompatibleDC(screen);
        let handle = CreateCompatibleBitmap(screen, w, h);
        ReleaseDC(0, screen);
        if dc == 0 || handle == 0 {
            if dc != 0 {
                DeleteDC(dc);
            }
            if handle != 0 {
                DeleteObject(handle);
            }
            return Err("无法分配截图内存".into());
        }
        let old = SelectObject(dc, handle);
        Ok(Self {
            dc,
            handle,
            old,
            w,
            h,
        })
    }
    unsafe fn capture() -> Result<(Self, i32, i32), String> {
        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let b = Self::new(
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )?;
        let screen = GetDC(0);
        let ok = BitBlt(b.dc, 0, 0, b.w, b.h, screen, x, y, SRCCOPY | CAPTUREBLT);
        ReleaseDC(0, screen);
        if ok == 0 {
            return Err("无法读取屏幕画面".into());
        }
        Ok((b, x, y))
    }
    unsafe fn pixels(&self) -> Result<Vec<u8>, String> {
        let mut info: BITMAPINFO = zeroed();
        info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = self.w;
        info.bmiHeader.biHeight = -self.h;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        let mut data = vec![0u8; self.w as usize * self.h as usize * 4];
        SelectObject(self.dc, self.old);
        let ok = GetDIBits(
            self.dc,
            self.handle,
            0,
            self.h as u32,
            data.as_mut_ptr().cast(),
            &mut info,
            DIB_RGB_COLORS,
        );
        SelectObject(self.dc, self.handle);
        if ok == 0 {
            return Err("无法读取图像像素".into());
        }
        Ok(data)
    }
}
impl Drop for Bitmap {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.handle);
            DeleteDC(self.dc);
        }
    }
}
#[derive(Clone)]
struct Mark {
    tool: Tool,
    points: Vec<Point>,
    color: u32,
    text: String,
    width: i32,
}
#[derive(Default)]
struct RenderCache {
    frame: Option<Bitmap>,
    previous_selection: Option<Region>,
    composed: Option<Bitmap>,
    committed: usize,
    active_points: usize,
}
struct App {
    image: Option<Bitmap>,
    selection: Region,
    start: Point,
    drag: bool,
    marks: Vec<Mark>,
    active_mark: Option<Mark>,
    cache: RenderCache,
    redo: Vec<Mark>,
    tool: Tool,
    color: u32,
    pin: bool,
    edit: HWND,
    origin: Point,
    bar: Region,
    hover: Option<usize>,
    buttons: Vec<(usize, Region)>,
    config: settings::Config,
    hotkeys: settings::Registry,
    last_region: Option<Region>,
    drag_region: Region,
    drag_mode: u8,
    stroke_width: i32,
    custom_colors: [u32; 16],
    ui_dpi: u32,
}
impl Default for App {
    fn default() -> Self {
        Self {
            image: None,
            selection: Region::default(),
            start: Point::default(),
            drag: false,
            marks: vec![],
            active_mark: None,
            cache: RenderCache::default(),
            redo: vec![],
            tool: Tool::Select,
            color: 0x005050e8,
            pin: false,
            edit: 0,
            origin: Point::default(),
            bar: Region::default(),
            hover: None,
            buttons: vec![],
            config: settings::Config::default(),
            hotkeys: settings::Registry::default(),
            last_region: None,
            drag_region: Region::default(),
            drag_mode: 0,
            stroke_width: 3,
            custom_colors: [0xffffff; 16],
            ui_dpi: 96,
        }
    }
}
impl App {
    fn finish_gesture(&mut self, commit: bool) {
        self.drag = false;
        if let Some(mark) = self.active_mark.take() {
            if commit {
                self.marks.push(mark);
                self.redo.clear();
            } else if self.cache.active_points != 0 {
                self.cache.composed = None;
            }
        }
        self.cache.active_points = 0;
    }
    fn undo(&mut self) {
        let active = self.active_mark.is_some();
        self.finish_gesture(false);
        if !active {
            if let Some(mark) = self.marks.pop() {
                self.redo.push(mark);
            }
            self.cache.composed = None;
        }
    }
}
unsafe fn error(hwnd: HWND, s: &str) {
    MessageBoxW(
        hwnd,
        wide(s).as_ptr(),
        wide("轻截").as_ptr(),
        MB_OK | MB_ICONERROR,
    );
}
unsafe fn text(dc: HDC, x: i32, y: i32, s: &str) {
    let t = wide(s);
    TextOutW(dc, x, y, t.as_ptr(), (t.len() - 1) as i32);
}
unsafe fn fill(dc: HDC, r: Region, color: u32) {
    let brush = CreateSolidBrush(color);
    let rect = RECT {
        left: r.x,
        top: r.y,
        right: r.x + r.w,
        bottom: r.y + r.h,
    };
    FillRect(dc, &rect, brush);
    DeleteObject(brush);
}
unsafe fn finish_text(a: &mut App) {
    if a.edit != 0 {
        let n = GetWindowTextLengthW(a.edit);
        let mut t = vec![0u16; n as usize + 1];
        GetWindowTextW(a.edit, t.as_mut_ptr(), n + 1);
        let s = String::from_utf16_lossy(&t[..n as usize]);
        if !s.trim().is_empty() {
            a.marks.push(Mark {
                tool: Tool::Text,
                points: vec![a.start],
                color: a.color,
                text: s,
                width: a.stroke_width,
            });
            a.redo.clear();
        }
        DestroyWindow(a.edit);
        a.edit = 0;
    }
}
unsafe fn copy(hwnd: HWND, b: &Bitmap) -> Result<(), String> {
    let data = b.pixels()?;
    let len = size_of::<BITMAPINFOHEADER>() + data.len();
    let mem = GlobalAlloc(GMEM_MOVEABLE, len);
    if mem.is_null() {
        return Err("剪贴板内存不足".into());
    }
    let p = GlobalLock(mem);
    if p.is_null() {
        GlobalFree(mem);
        return Err("剪贴板内存锁定失败".into());
    }
    let header = p as *mut BITMAPINFOHEADER;
    *header = zeroed();
    (*header).biSize = size_of::<BITMAPINFOHEADER>() as u32;
    (*header).biWidth = b.w;
    (*header).biHeight = -b.h;
    (*header).biPlanes = 1;
    (*header).biBitCount = 32;
    std::ptr::copy_nonoverlapping(
        data.as_ptr(),
        (p as *mut u8).add(size_of::<BITMAPINFOHEADER>()),
        data.len(),
    );
    GlobalUnlock(mem);
    if OpenClipboard(hwnd) == 0 {
        GlobalFree(mem);
        return Err("剪贴板正被占用，请重试".into());
    }
    EmptyClipboard();
    let ok = SetClipboardData(8, mem as isize);
    CloseClipboard();
    if ok == 0 {
        GlobalFree(mem);
        return Err("写入剪贴板失败".into());
    }
    Ok(())
}
unsafe fn save(hwnd: HWND, b: &Bitmap) -> Result<bool, String> {
    let mut path = [0u16; 32768];
    // Timestamp without an extra runtime dependency.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let name = wide(&format!("轻截-{stamp}.png"));
    path[..name.len()].copy_from_slice(&name);
    let filter = wide("PNG 图片\0*.png\0");
    let mut ofn: OPENFILENAMEW = zeroed();
    ofn.lStructSize = size_of::<OPENFILENAMEW>() as u32;
    ofn.hwndOwner = hwnd;
    ofn.lpstrFilter = filter.as_ptr();
    ofn.lpstrFile = path.as_mut_ptr();
    ofn.nMaxFile = path.len() as u32;
    let ext = wide("png");
    ofn.lpstrDefExt = ext.as_ptr();
    ofn.Flags = OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;

    if GetSaveFileNameW(&mut ofn) == 0 {
        let e = CommDlgExtendedError();
        return if e == 0 {
            Ok(false)
        } else {
            Err(format!("保存对话框错误：{e}"))
        };
    }
    let path = String::from_utf16_lossy(&path[..path.iter().position(|x| *x == 0).unwrap()]);
    let mut rgba = b.pixels()?;
    for p in rgba.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
        p[3] = 255;
    }
    export::png(std::path::Path::new(&path), b.w as u32, b.h as u32, &rgba)?;
    Ok(true)
}
unsafe fn close_capture(hwnd: HWND, a: &mut App) {
    finish_text(a);
    ShowWindow(hwnd, SW_HIDE);
    if a.selection.valid() {
        a.last_region = Some(Region {
            x: a.selection.x + a.origin.x,
            y: a.selection.y + a.origin.y,
            ..a.selection
        });
    }
    a.image = None;
    a.active_mark = None;
    a.cache = RenderCache::default();
    a.marks.clear();
    a.redo.clear();
    a.selection = Region::default();
    a.drag = false;
    a.hover = None;
    a.buttons.clear();
    ReleaseCapture();
}
unsafe fn begin(hwnd: HWND, a: &mut App, full: bool) {
    if a.image.is_some() {
        return;
    }
    match Bitmap::capture() {
        Ok((b, x, y)) => {
            let w = b.w;
            let h = b.h;
            a.origin = Point { x, y };
            a.image = Some(b);
            a.selection = if full {
                Region { x: 0, y: 0, w, h }
            } else {
                Region::default()
            };
            a.tool = Tool::Select;
            SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h, SWP_SHOWWINDOW);
            a.ui_dpi = dpi::current(hwnd);
            SetForegroundWindow(hwnd);
            SetFocus(hwnd);
            InvalidateRect(hwnd, null(), 0);
        }
        Err(e) => error(hwnd, &e),
    }
}
unsafe fn trigger(hwnd: HWND, a: &mut App, action: usize) {
    if a.image.is_some() {
        return;
    }
    match action {
        0 => begin(hwnd, a, false),
        1 => begin(hwnd, a, true),
        2 => {
            if let Some(r) = a.last_region {
                begin(hwnd, a, false);
                if let Some(b) = &a.image {
                    a.selection = Region::between(
                        Point {
                            x: r.x - a.origin.x,
                            y: r.y - a.origin.y,
                        },
                        Point {
                            x: r.x + r.w - a.origin.x,
                            y: r.y + r.h - a.origin.y,
                        },
                        b.w,
                        b.h,
                    );
                    InvalidateRect(hwnd, null(), 0);
                }
            } else {
                error(hwnd, "本次运行尚无截图区域。请先完成一次区域截图。");
            }
        }
        3 => {
            SetTimer(hwnd, 1, 3000, None);
        }
        _ => {}
    }
}
unsafe fn command(hwnd: HWND, a: &mut App, id: usize) {
    finish_text(a);
    if id == 8 {
        a.undo();
    } else {
        a.finish_gesture(true);
    }
    if hwnd != 0 && GetCapture() == hwnd {
        ReleaseCapture();
    }
    match id {
        0 => {
            a.tool = Tool::Select;
        }
        1..=6 | 13 => a.tool = Tool::from_id(id).unwrap(),
        14 => {
            close_capture(hwnd, a);
            return;
        }
        100..=107 => a.color = ui::COLORS[id - 100],
        200 => a.stroke_width = (a.stroke_width - 1).max(1),
        201 => a.stroke_width = (a.stroke_width + 1).min(12),
        7 => {
            if let Some(c) = color::show(hwnd, a.color, &mut a.custom_colors) {
                a.color = c;
            }
        }
        8 => {}
        9 => {
            if let Some(m) = a.redo.pop() {
                a.marks.push(m);
            }
        }
        10..=12 => match output(a) {
            Ok(b) => {
                let result = match id {
                    10 => save(hwnd, &b),
                    12 => copy(hwnd, &b).map(|_| true),
                    _ => {
                        let mut p = App {
                            pin: true,
                            selection: Region {
                                x: 0,
                                y: 0,
                                w: b.w,
                                h: b.h,
                            },
                            ..App::default()
                        };
                        let w = b.w;
                        let h = b.h;
                        p.image = Some(b);
                        let state = window::State::new(p);
                        let win = CreateWindowExW(
                            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                            wide(PIN_CLASS).as_ptr(),
                            wide("轻截 · 贴图（拖动移动，Esc 关闭）").as_ptr(),
                            WS_POPUP | WS_BORDER | WS_VISIBLE,
                            a.origin.x + a.selection.x,
                            a.origin.y + a.selection.y,
                            w,
                            h,
                            0,
                            0,
                            GetModuleHandleW(null()),
                            window::State::param(&state),
                        );
                        if win == 0 {
                            Err("贴图窗口创建失败".into())
                        } else {
                            Ok(true)
                        }
                    }
                };
                match result {
                    Ok(true) => {
                        if a.pin {
                            DestroyWindow(hwnd);
                        } else {
                            close_capture(hwnd, a);
                        }
                    }
                    Ok(false) => {}
                    Err(e) => error(hwnd, &e),
                }
            }
            Err(e) => error(hwnd, &e),
        },
        _ => {}
    }
    InvalidateRect(hwnd, null(), 0);
}
unsafe fn paint(hwnd: HWND, a: &mut App) {
    let mut ps: PAINTSTRUCT = zeroed();
    let dc = BeginPaint(hwnd, &mut ps);
    if let Ok(buf) = render(a) {
        BitBlt(dc, 0, 0, buf.w, buf.h, buf.dc, 0, 0, SRCCOPY);
    }
    EndPaint(hwnd, &ps);
}
unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    window::dispatch(hwnd, msg, wp, lp, handle_message)
}
unsafe fn handle_message(hwnd: HWND, msg: u32, wp: usize, lp: isize, a: &mut App) -> Option<isize> {
    let p = Point {
        x: lp as i16 as i32,
        y: (lp >> 16) as i16 as i32,
    };
    Some(match msg {
        WM_SETCURSOR => {
            SetCursor(LoadCursorW(
                0,
                if a.pin {
                    IDC_ARROW
                } else if a.hover.is_some() {
                    IDC_HAND
                } else if a.tool == Tool::Select && a.selection.valid() {
                    let mut pos: POINT = zeroed();
                    GetCursorPos(&mut pos);
                    match a.selection.hit(Point {
                        x: pos.x - a.origin.x,
                        y: pos.y - a.origin.y,
                    }) {
                        1 | 2 => IDC_SIZEWE,
                        4 | 8 => IDC_SIZENS,
                        5 | 10 => IDC_SIZENWSE,
                        6 | 9 => IDC_SIZENESW,
                        16 => IDC_SIZEALL,
                        _ => IDC_CROSS,
                    }
                } else {
                    IDC_CROSS
                },
            ));
            1
        }
        WM_APP_SETTINGS => {
            settings::show_page(hwnd, &a.config, wp.min(2));
            0
        }
        WM_HOTKEY => {
            if let Some(action) = a.hotkeys.action(wp) {
                if !settings::record_registered(a.config.keys[action]) {
                    trigger(hwnd, a, action);
                }
            }
            0
        }
        WM_TIMER => {
            KillTimer(hwnd, 1);
            begin(hwnd, a, false);
            0
        }
        WM_PAINT => {
            paint(hwnd, a);
            0
        }
        WM_ERASEBKGND => 1,
        WM_DPICHANGED => {
            a.ui_dpi = wp as u32 & 0xffff;
            a.cache.previous_selection = None;
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_LBUTTONDOWN => {
            if a.pin {
                ReleaseCapture();
                SendMessageW(hwnd, WM_NCLBUTTONDOWN, HTCAPTION as usize, 0);
                return Some(0);
            }
            if a.selection.valid() && a.bar.contains(p) {
                if let Some((id, _)) = a.buttons.iter().find(|(_, r)| r.contains(p)) {
                    let id = *id;
                    command(hwnd, a, id);
                }
                return Some(0);
            }
            finish_text(a);
            if let Some(b) = &a.image {
                if a.tool == Tool::Select {
                    a.start = p;
                    a.drag_region = a.selection;
                    a.drag_mode = a.selection.hit(p);
                    if a.drag_mode == 0 {
                        a.selection = Region::default();
                        a.marks.clear();
                        a.redo.clear();
                        a.cache.composed = None;
                    }
                    a.drag = true;
                } else if a.selection.contains(p) {
                    a.start = p;
                    if a.tool == Tool::Text {
                        a.edit = CreateWindowExW(
                            0,
                            wide("EDIT").as_ptr(),
                            wide("").as_ptr(),
                            WS_CHILD | WS_VISIBLE | WS_BORDER | ES_AUTOHSCROLL as u32,
                            p.x,
                            p.y,
                            220,
                            30,
                            hwnd,
                            0,
                            GetModuleHandleW(null()),
                            std::ptr::null_mut(),
                        );
                        SetFocus(a.edit);
                    } else {
                        a.active_mark = Some(Mark {
                            tool: a.tool,
                            points: vec![p, p],
                            color: a.color,
                            text: String::new(),
                            width: a.stroke_width,
                        });
                        a.drag = true;
                    }
                }
                let _ = b;
                if a.drag {
                    SetCapture(hwnd);
                }
            }
            0
        }
        WM_MOUSEMOVE => {
            if !a.drag {
                let hover = a
                    .buttons
                    .iter()
                    .find(|(_, r)| r.contains(p))
                    .map(|(id, _)| *id);
                if hover != a.hover {
                    a.hover = hover;
                    InvalidateRect(hwnd, null(), 0);
                }
            }
            if a.drag {
                if let Some(b) = &a.image {
                    let q = Point {
                        x: p.x.clamp(0, b.w),
                        y: p.y.clamp(0, b.h),
                    };
                    if a.tool == Tool::Select {
                        a.selection = if a.drag_mode == 0 {
                            Region::between(a.start, q, b.w, b.h)
                        } else {
                            a.drag_region.adjusted(
                                a.drag_mode,
                                q.x - a.start.x,
                                q.y - a.start.y,
                                b.w,
                                b.h,
                            )
                        };
                    } else if let Some(m) = a.active_mark.as_mut() {
                        if m.tool == Tool::Pen {
                            if m.points.len() < 100_000 {
                                m.points.push(q);
                            }
                        } else {
                            *m.points.last_mut().unwrap() = q;
                        }
                    }
                    InvalidateRect(hwnd, null(), 0);
                }
            }
            0
        }
        WM_CAPTURECHANGED | WM_CANCELMODE => {
            a.finish_gesture(false);
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_LBUTTONUP => {
            a.finish_gesture(true);
            ReleaseCapture();
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_RBUTTONUP => {
            if a.pin {
                DestroyWindow(hwnd);
            } else {
                close_capture(hwnd, a);
            }
            0
        }
        WM_KEYDOWN => {
            let ctrl = GetKeyState(VK_CONTROL as i32) < 0;
            match wp as u16 {
                VK_ESCAPE => {
                    if a.pin {
                        DestroyWindow(hwnd);
                    } else {
                        close_capture(hwnd, a);
                    }
                }
                VK_RETURN => command(hwnd, a, 12),
                0x53 if ctrl => command(hwnd, a, 10),
                0x43 if ctrl => command(hwnd, a, 12),
                0x5a if ctrl => command(hwnd, a, 8),
                0x59 if ctrl => command(hwnd, a, 9),
                0x54 if ctrl => command(hwnd, a, 11),
                VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN if !a.pin && a.selection.valid() => {
                    let step = if GetKeyState(VK_SHIFT as i32) < 0 {
                        10
                    } else {
                        1
                    };
                    let dx = if wp == VK_LEFT as usize {
                        -step
                    } else if wp == VK_RIGHT as usize {
                        step
                    } else {
                        0
                    };
                    let dy = if wp == VK_UP as usize {
                        -step
                    } else if wp == VK_DOWN as usize {
                        step
                    } else {
                        0
                    };
                    if let Some(b) = &a.image {
                        a.selection =
                            a.selection
                                .adjusted(if ctrl { 2 | 8 } else { 16 }, dx, dy, b.w, b.h);
                        InvalidateRect(hwnd, null(), 0);
                    }
                }
                0x31..=0x38 if !ctrl => command(hwnd, a, 100 + (wp - 0x31)),
                VK_OEM_PLUS | VK_ADD => {
                    a.stroke_width = (a.stroke_width + 1).min(12);
                    InvalidateRect(hwnd, null(), 0);
                }
                VK_OEM_MINUS | VK_SUBTRACT => {
                    a.stroke_width = (a.stroke_width - 1).max(1);
                    InvalidateRect(hwnd, null(), 0);
                }
                0x43 if !ctrl => command(hwnd, a, 7),
                key if !ctrl && Tool::from_key(key).is_some() => {
                    command(hwnd, a, Tool::from_key(key).unwrap() as usize)
                }
                _ => {}
            }
            0
        }
        TRAY => {
            if lp as u32 == WM_LBUTTONUP {
                begin(hwnd, a, false);
            } else if lp as u32 == WM_RBUTTONUP {
                let menu = CreatePopupMenu();
                for (id, label) in [
                    (101, "区域截图"),
                    (102, "全屏截图"),
                    (103, "延时 3 秒截图"),
                    (104, "使用帮助"),
                    (106, "上次区域截图"),
                    (107, "快捷键设置…"),
                    (105, "退出轻截"),
                ] {
                    let label = if id == 101 {
                        format!("{label}\t{}", a.config.keys[0].label())
                    } else if id == 102 {
                        format!("{label}\t{}", a.config.keys[1].label())
                    } else {
                        label.to_string()
                    };
                    AppendMenuW(menu, MF_STRING, id, wide(&label).as_ptr());
                }
                let mut pos: POINT = zeroed();
                GetCursorPos(&mut pos);
                SetForegroundWindow(hwnd);
                let choice = TrackPopupMenu(
                    menu,
                    TPM_RETURNCMD | TPM_RIGHTBUTTON,
                    pos.x,
                    pos.y,
                    0,
                    hwnd,
                    null(),
                );
                DestroyMenu(menu);
                match choice {
                    106 => trigger(hwnd, a, 2),
                    107 => settings::show(hwnd, &a.config),
                    101 => begin(hwnd, a, false),
                    102 => begin(hwnd, a, true),
                    103 => {
                        SetTimer(hwnd, 1, 3000, None);
                    }
                    104 => settings::show_page(hwnd, &a.config, 2),
                    105 => {
                        DestroyWindow(hwnd);
                    }
                    _ => {}
                }
            }
            0
        }
        WM_CLOSE => {
            if a.pin {
                DestroyWindow(hwnd);
            } else {
                close_capture(hwnd, a);
            }
            0
        }
        WM_DESTROY => {
            if !a.pin {
                let mut n: NOTIFYICONDATAW = zeroed();
                n.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
                n.hWnd = hwnd;
                n.uID = 1;
                Shell_NotifyIconW(NIM_DELETE, &n);
                a.hotkeys.clear(hwnd);
                PostQuitMessage(0);
            }
            0
        }
        _ => return None,
    })
}
pub fn run() {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let class = wide(CLASS);
        if FindWindowW(class.as_ptr(), null()) != 0 {
            let existing = FindWindowW(class.as_ptr(), null());
            let page = if std::env::args().any(|x| x == "--help") {
                2
            } else {
                0
            };
            PostMessageW(existing, WM_APP_SETTINGS, page, 0);
            return;
        }
        let instance = GetModuleHandleW(null());
        let app_icon = ui::app_icon();
        let mut wc: WNDCLASSW = zeroed();
        wc.hIcon = app_icon;
        wc.lpfnWndProc = Some(proc);
        wc.hInstance = instance;
        wc.lpszClassName = class.as_ptr();
        wc.hCursor = LoadCursorW(0, IDC_CROSS);
        RegisterClassW(&wc);
        let pin_class = wide(PIN_CLASS);
        wc.lpszClassName = pin_class.as_ptr();
        RegisterClassW(&wc);
        let state = window::State::new(App::default());
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class.as_ptr(),
            wide("轻截").as_ptr(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            0,
            0,
            instance,
            window::State::param(&state),
        );
        if hwnd == 0 {
            error(0, "无法创建截图窗口");
            return;
        }
        let mut n: NOTIFYICONDATAW = zeroed();
        n.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        n.hWnd = hwnd;
        n.uID = 1;
        n.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        n.uCallbackMessage = TRAY;
        n.hIcon = if app_icon != 0 {
            app_icon
        } else {
            LoadIconW(0, IDI_APPLICATION)
        };
        let tip = wide("轻截 · 左键截图 · 右键设置快捷键");
        n.szTip[..tip.len()].copy_from_slice(&tip);
        if Shell_NotifyIconW(NIM_ADD, &n) == 0 {
            error(hwnd, "无法创建托盘图标");
            DestroyWindow(hwnd);
            return;
        }
        match settings::Config::load() {
            Ok(c) => {
                state.value.borrow_mut().config = c;
            }
            Err(e) => error(hwnd, &format!("读取设置失败，将使用默认快捷键：{e}")),
        }
        let c = state.value.borrow_mut().config.clone();
        if let Err(e) = state.value.borrow_mut().hotkeys.apply(hwnd, &c, false) {
            error(hwnd, &format!("{e}\n请从托盘打开快捷键设置。"));
        }
        if std::env::args().any(|x| x == "--help") {
            settings::show_page(hwnd, &state.value.borrow().config, 2);
        }
        if std::env::args().any(|x| x == "--settings") {
            settings::show(hwnd, &state.value.borrow_mut().config);
        }
        if std::env::args().any(|x| x == "--capture") {
            begin(hwnd, &mut state.value.borrow_mut(), false);
        }
        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, 0, 0, 0) > 0 {
            if settings::record(&msg) {
                continue;
            }
            if settings::dialog_message(&msg) {
                continue;
            }
            if msg.message == WM_KEYDOWN && msg.hwnd != hwnd && IsChild(hwnd, msg.hwnd) != 0 {
                if msg.wParam == VK_RETURN as usize {
                    finish_text(&mut state.value.borrow_mut());
                    SetFocus(hwnd);
                    InvalidateRect(hwnd, null(), 0);
                    continue;
                }
                if msg.wParam == VK_ESCAPE as usize {
                    let a = &mut *state.value.borrow_mut();
                    DestroyWindow(a.edit);
                    a.edit = 0;
                    SetFocus(hwnd);
                    continue;
                }
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn undo_during_drag_does_not_modify_previous_mark() {
        unsafe {
            let previous = Mark {
                tool: Tool::Rectangle,
                points: vec![Point { x: 1, y: 1 }, Point { x: 8, y: 8 }],
                color: 0,
                text: String::new(),
                width: 3,
            };
            let mut a = App {
                drag: true,
                tool: Tool::Pen,
                marks: vec![previous.clone()],
                active_mark: Some(previous),
                ..App::default()
            };
            command(0, &mut a, 8);
            assert!(
                !a.drag,
                "撤销必须结束活动拖动，避免后续鼠标移动修改历史标注"
            );
            assert_eq!(a.marks.len(), 1);
            assert!(a.active_mark.is_none());
        }
    }
    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn crop_and_redaction_export_exact_pixels() {
        unsafe {
            let b = Bitmap::new(100, 80).unwrap();
            fill(
                b.dc,
                Region {
                    x: 0,
                    y: 0,
                    w: 100,
                    h: 80,
                },
                0x00ffffff,
            );
            #[allow(clippy::field_reassign_with_default)]
            let mut app = App::default();
            app.image = Some(b);
            app.selection = Region {
                x: 20,
                y: 10,
                w: 40,
                h: 30,
            };
            app.marks.push(Mark {
                tool: Tool::Redact,
                points: vec![Point { x: 25, y: 15 }, Point { x: 35, y: 25 }],
                color: 0,
                text: String::new(),
                width: 3,
            });
            let out = output(&mut app).unwrap();
            assert_eq!((out.w, out.h), (40, 30));
            let pixels = out.pixels().unwrap();
            assert_eq!(&pixels[..3], &[255, 255, 255]);
            let i = (8 * 40 + 8) * 4;
            assert_eq!(&pixels[i..i + 3], &[32, 32, 32]);
        }
    }
    #[test]
    fn gdi_resources_released_on_repeated_exports() {
        if std::env::var_os("QINGJIE_GDI_PROBE").is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "win::tests::gdi_resources_released_on_repeated_exports",
                    "--test-threads=1",
                ])
                .env("QINGJIE_GDI_PROBE", "1")
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }
        unsafe {
            use windows_sys::Win32::System::Threading::{
                GetCurrentProcess, GetGuiResources, GR_GDIOBJECTS,
            };
            {
                let warm = Bitmap::new(20, 20).unwrap();
                fill(
                    warm.dc,
                    Region {
                        x: 0,
                        y: 0,
                        w: 20,
                        h: 20,
                    },
                    0x00332211,
                );
                warm.pixels().unwrap();
            }
            let before = GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS);
            for _ in 0..100 {
                let b = Bitmap::new(20, 20).unwrap();
                fill(
                    b.dc,
                    Region {
                        x: 0,
                        y: 0,
                        w: 20,
                        h: 20,
                    },
                    0x00332211,
                );
                let p = b.pixels().unwrap();
                assert_eq!(&p[..3], &[51, 34, 17]);
            }
            let after = GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS);
            assert_eq!(before, after, "重复导出后 GDI 对象应回到基线");
        }
    }
}

#[cfg(test)]
mod visual_regression {
    use super::*;
    #[test]
    fn outside_selection_keeps_desktop_detail() {
        unsafe {
            let b = Bitmap::new(1200, 800).unwrap();
            fill(
                b.dc,
                Region {
                    x: 0,
                    y: 0,
                    w: 600,
                    h: 800,
                },
                0x00ffffff,
            );
            fill(
                b.dc,
                Region {
                    x: 600,
                    y: 0,
                    w: 600,
                    h: 800,
                },
                0x00d08040,
            );
            let mut a = App {
                image: Some(b),
                selection: Region {
                    x: 300,
                    y: 200,
                    w: 500,
                    h: 300,
                },
                ..App::default()
            };
            let frame = render(&mut a).unwrap();
            assert_ne!(
                GetPixel(frame.dc, 100, 700),
                GetPixel(frame.dc, 1000, 700),
                "遮罩不应抹掉桌面的颜色和细节"
            );
        }
    }
}

#[cfg(test)]
mod preview {
    use super::*;
    #[test]
    fn render_ui_preview() {
        unsafe {
            let b = Bitmap::new(1280, 820).unwrap();
            let dc = b.dc;
            fill(
                dc,
                Region {
                    x: 0,
                    y: 0,
                    w: 1280,
                    h: 820,
                },
                ui::rgb(234, 239, 245),
            );
            ui::rounded(
                dc,
                Region {
                    x: 72,
                    y: 58,
                    w: 1136,
                    h: 692,
                },
                16,
                ui::rgb(255, 255, 255),
                ui::rgb(216, 224, 234),
            );
            fill(
                dc,
                Region {
                    x: 73,
                    y: 110,
                    w: 206,
                    h: 622,
                },
                ui::rgb(248, 250, 252),
            );
            for (i, c) in [
                ui::rgb(248, 113, 113),
                ui::rgb(250, 204, 21),
                ui::rgb(74, 222, 128),
            ]
            .iter()
            .enumerate()
            {
                ui::rounded(
                    dc,
                    Region {
                        x: 92 + i as i32 * 20,
                        y: 78,
                        w: 10,
                        h: 10,
                    },
                    5,
                    *c,
                    *c,
                );
            }
            ui::label(
                dc,
                170,
                74,
                "Workspace  /  设计工作台",
                15,
                ui::rgb(100, 116, 139),
                false,
            );
            ui::label(dc, 100, 140, "Northstar", 23, ui::rgb(30, 41, 59), true);
            for (i, t) in ["项目概览", "设计文件", "团队协作", "灵感收藏"]
                .iter()
                .enumerate()
            {
                if i == 0 {
                    ui::rounded(
                        dc,
                        Region {
                            x: 88,
                            y: 198,
                            w: 172,
                            h: 42,
                        },
                        8,
                        ui::rgb(229, 239, 255),
                        ui::rgb(229, 239, 255),
                    );
                }
                ui::label(
                    dc,
                    112,
                    208 + i as i32 * 52,
                    t,
                    16,
                    ui::rgb(71, 85, 105),
                    i == 0,
                );
            }
            ui::label(
                dc,
                330,
                150,
                "把想法，变成看得见的进展。",
                29,
                ui::rgb(28, 40, 61),
                true,
            );
            ui::label(
                dc,
                332,
                199,
                "产品设计 / 本周概览",
                16,
                ui::rgb(100, 116, 139),
                false,
            );
            for (i, (title, num)) in [
                ("进行中的项目", "12"),
                ("本周已完成", "28"),
                ("团队成员", "08"),
            ]
            .iter()
            .enumerate()
            {
                let x = 330 + i as i32 * 262;
                ui::rounded(
                    dc,
                    Region {
                        x,
                        y: 250,
                        w: 240,
                        h: 116,
                    },
                    12,
                    ui::rgb(248, 250, 253),
                    ui::rgb(230, 235, 243),
                );
                ui::label(dc, x + 22, 270, title, 15, ui::rgb(100, 116, 139), false);
                ui::label(dc, x + 22, 298, num, 34, ui::rgb(36, 57, 86), true);
            }
            ui::label(dc, 332, 403, "本周工作节奏", 19, ui::rgb(40, 55, 75), true);
            for i in 0..7 {
                let height = [54, 88, 66, 112, 94, 136, 102][i];
                ui::rounded(
                    dc,
                    Region {
                        x: 350 + i as i32 * 102,
                        y: 602 - height,
                        w: 48,
                        h: height,
                    },
                    6,
                    if i == 5 {
                        ui::rgb(45, 112, 242)
                    } else {
                        ui::rgb(210, 226, 252)
                    },
                    ui::rgb(210, 226, 252),
                );
                ui::label(
                    dc,
                    360 + i as i32 * 102,
                    618,
                    &format!("{:02}", i + 1),
                    13,
                    ui::rgb(116, 128, 146),
                    false,
                );
            }
            let mut a = App {
                image: Some(b),
                selection: Region {
                    x: 307,
                    y: 128,
                    w: 827,
                    h: 530,
                },
                tool: Tool::Arrow,
                hover: Some(4),
                ..App::default()
            };
            a.marks.push(Mark {
                tool: Tool::Arrow,
                points: vec![Point { x: 1030, y: 444 }, Point { x: 917, y: 489 }],
                color: ui::rgb(232, 80, 80),
                text: String::new(),
                width: 3,
            });
            let frame = render(&mut a).unwrap();
            write_preview(frame, "docs/ui-preview.png");
            a.selection = Region::default();
            let frame = render(&mut a).unwrap();
            write_preview(frame, "docs/capture-preview.png");
        }
    }
    pub(super) unsafe fn write_preview(b: &Bitmap, path: &str) {
        std::fs::create_dir_all("docs").unwrap();
        let mut bytes = b.pixels().unwrap();
        for p in bytes.as_chunks_mut::<4>().0 {
            p.swap(0, 2);
            p[3] = 255;
        }
        export::png(std::path::Path::new(path), b.w as u32, b.h as u32, &bytes).unwrap();
    }
}
