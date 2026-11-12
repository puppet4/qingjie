//! Custom annotation colors. COLORREF byte order is intentionally isolated here.
use super::*;
use windows_sys::Win32::UI::Controls::DRAWITEMSTRUCT;
pub fn hex(color: u32) -> String {
    format!(
        "#{:02X}{:02X}{:02X}",
        color & 255,
        (color >> 8) & 255,
        (color >> 16) & 255
    )
}
pub fn parse_hex(value: &str) -> Option<u32> {
    let v = value.trim().strip_prefix('#').unwrap_or(value.trim());
    if v.len() != 6 || !v.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let rgb = u32::from_str_radix(v, 16).ok()?;
    Some(ui::rgb(rgb >> 16, (rgb >> 8) & 255, rgb & 255))
}
pub fn parse_rgb(values: [&str; 3]) -> Option<u32> {
    let [r, g, b] = values.map(|s| s.trim().parse::<u8>().ok());
    Some(ui::rgb(r? as u32, g? as u32, b? as u32))
}
struct Picker {
    color: u32,
    result: Option<u32>,
    fields: [HWND; 4],
    font: HFONT,
    layout: dpi::Layout,
    syncing: bool,
    valid: bool,
    custom: [u32; 16],
}
impl Drop for Picker {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.font);
        }
    }
}
unsafe fn value(h: HWND) -> String {
    let mut s = [0u16; 32];
    let n = GetWindowTextW(h, s.as_mut_ptr(), 32);
    String::from_utf16_lossy(&s[..n.max(0) as usize])
}
unsafe fn sync(p: &mut Picker, exclude: Option<usize>) {
    p.syncing = true;
    let values = [
        hex(p.color),
        (p.color & 255).to_string(),
        ((p.color >> 8) & 255).to_string(),
        ((p.color >> 16) & 255).to_string(),
    ];
    for (i, s) in values.iter().enumerate() {
        if exclude != Some(i) {
            SetWindowTextW(p.fields[i], wide(s).as_ptr());
        }
    }
    p.syncing = false;
}
unsafe fn paint(_hwnd: HWND, dc: HDC, p: &Picker) {
    fill(
        dc,
        Region {
            x: 0,
            y: 0,
            w: 420,
            h: 280,
        },
        ui::rgb(255, 255, 255),
    );
    ui::label(dc, 20, 18, "自定义颜色", 18, ui::INK, true);
    ui::label(dc, 20, 58, "颜色预览", 12, ui::MUTED, false);
    ui::rounded(
        dc,
        Region {
            x: 20,
            y: 76,
            w: 92,
            h: 90,
        },
        5,
        p.color,
        ui::rgb(205, 210, 218),
    );
    ui::label(dc, 132, 58, "HEX", 12, ui::rgb(86, 96, 110), false);
    ui::input_border(
        dc,
        Region {
            x: 132,
            y: 76,
            w: 268,
            h: 32,
        },
        GetFocus() == p.fields[0],
        ui::is_hot(p.fields[0]),
    );
    for i in 0..3 {
        ui::input_border(
            dc,
            Region {
                x: 132 + i * 90,
                y: 134,
                w: 80,
                h: 32,
            },
            GetFocus() == p.fields[i as usize + 1],
            ui::is_hot(p.fields[i as usize + 1]),
        );
    }

    for (i, name) in ["R", "G", "B"].iter().enumerate() {
        ui::label(
            dc,
            132 + i as i32 * 90,
            116,
            name,
            12,
            ui::rgb(86, 96, 110),
            false,
        );
    }
    ui::label(
        dc,
        20,
        180,
        if p.valid {
            "选定颜色用于之后的画笔、形状、箭头和文字。"
        } else {
            "请输入 6 位 HEX 色值或 0–255 的 RGB 数值。"
        },
        12,
        if p.valid {
            ui::rgb(86, 96, 110)
        } else {
            ui::rgb(200, 48, 48)
        },
        false,
    );
    fill(
        dc,
        Region {
            x: 0,
            y: 218,
            w: 420,
            h: 1,
        },
        ui::rgb(233, 236, 240),
    );
}
unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: usize, lp: isize) -> isize {
    window::dispatch(hwnd, msg, wp, lp, handle_message)
}
unsafe fn handle_message(
    hwnd: HWND,
    msg: u32,
    wp: usize,
    lp: isize,
    p: &mut Picker,
) -> Option<isize> {
    Some(match msg {
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = zeroed();
            let dc = BeginPaint(hwnd, &mut ps);
            dpi::paint(hwnd, dc, |dc| paint(hwnd, dc, p));
            EndPaint(hwnd, &ps);
            0
        }
        WM_DPICHANGED => {
            p.layout.apply(
                hwnd,
                &mut p.font,
                (420, 280),
                wp as u32 & 0xffff,
                Some(*(lp as *const RECT)),
            );
            0
        }
        WM_CTLCOLOREDIT => {
            SetTextColor(wp as HDC, ui::rgb(31, 35, 41));
            SetBkColor(wp as HDC, ui::rgb(255, 255, 255));
            GetStockObject(WHITE_BRUSH)
        }
        WM_DRAWITEM => {
            let draw = &*(lp as *const DRAWITEMSTRUCT);
            let label = match draw.CtlID {
                1 => "确定",
                2 => "取消",
                _ => "打开色板",
            };
            ui::button(hwnd, draw, label);
            1
        }
        WM_COMMAND => {
            let id = wp & 0xffff;
            let event = wp >> 16;
            if (10..14).contains(&id) && event == 0x0300 && !p.syncing {
                let i = id - 10;
                let parsed = if i == 0 {
                    parse_hex(&value(p.fields[0]))
                } else {
                    let vals = [value(p.fields[1]), value(p.fields[2]), value(p.fields[3])];
                    parse_rgb([&vals[0], &vals[1], &vals[2]])
                };
                p.valid = parsed.is_some();
                EnableWindow(GetDlgItem(hwnd, 1), p.valid as i32);
                if let Some(c) = parsed {
                    p.color = c;
                    sync(p, Some(i));
                }
                InvalidateRect(hwnd, null(), 0);
                return Some(0);
            }
            match id {
                1 => {
                    if p.valid {
                        p.result = Some(p.color);
                        DestroyWindow(hwnd);
                    } else {
                        InvalidateRect(hwnd, null(), 0);
                    }
                }
                2 => {
                    DestroyWindow(hwnd);
                }
                3 => {
                    let mut cc: CHOOSECOLORW = zeroed();
                    cc.lStructSize = size_of::<CHOOSECOLORW>() as u32;
                    cc.hwndOwner = hwnd;
                    cc.rgbResult = p.color;
                    cc.lpCustColors = p.custom.as_mut_ptr();
                    cc.Flags = CC_FULLOPEN | CC_RGBINIT | CC_ANYCOLOR;
                    if ChooseColorW(&mut cc) != 0 {
                        p.color = cc.rgbResult;
                        p.valid = true;
                        EnableWindow(GetDlgItem(hwnd, 1), 1);
                        sync(p, None);
                        InvalidateRect(hwnd, null(), 0);
                    } else {
                        let e = CommDlgExtendedError();
                        if e != 0 {
                            error(hwnd, &format!("打开色板失败：{e}"));
                        }
                    }
                }
                _ => {}
            }
            0
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        _ => return None,
    })
}
unsafe fn field(hwnd: HWND, id: usize, text: &str, x: i32, y: i32, w: i32, font: HFONT) -> HWND {
    let e = CreateWindowExW(
        0,
        wide("EDIT").as_ptr(),
        wide(text).as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL as u32,
        x,
        y,
        w,
        20,
        hwnd,
        id as isize,
        GetModuleHandleW(null()),
        std::ptr::null_mut(),
    );
    SendMessageW(e, WM_SETFONT, font as usize, 1);
    SendMessageW(e, 0x00c5, if id == 10 { 7 } else { 3 }, 0);
    ui::track_control(e);
    e
}
unsafe fn create(
    owner: HWND,
    color: u32,
    custom: [u32; 16],
) -> Option<(HWND, std::rc::Rc<window::State<Picker>>)> {
    let class = wide("Qingjie.Color");
    let mut wc: WNDCLASSW = zeroed();
    wc.lpfnWndProc = Some(proc);
    wc.lpszClassName = class.as_ptr();
    wc.hInstance = GetModuleHandleW(null());
    wc.hCursor = LoadCursorW(0, IDC_ARROW);
    wc.hIcon = ui::app_icon();
    RegisterClassW(&wc);
    let font = CreateFontW(
        -13,
        0,
        0,
        0,
        400,
        0,
        0,
        0,
        DEFAULT_CHARSET as u32,
        0,
        0,
        CLEARTYPE_QUALITY as u32,
        0,
        wide("Segoe UI").as_ptr(),
    );
    let p = window::State::new(Picker {
        color,
        result: None,
        fields: [0; 4],
        font,
        layout: dpi::Layout::default(),
        syncing: true,
        valid: true,
        custom,
    });
    let mut r = RECT {
        left: 0,
        top: 0,
        right: 420,
        bottom: 280,
    };
    AdjustWindowRectEx(
        &mut r,
        WS_CAPTION | WS_SYSMENU,
        0,
        WS_EX_TOPMOST | WS_EX_CONTROLPARENT,
    );
    let mut mouse: POINT = zeroed();
    GetCursorPos(&mut mouse);
    let monitor = MonitorFromPoint(mouse, MONITOR_DEFAULTTONEAREST);
    let mut info: MONITORINFO = zeroed();
    info.cbSize = size_of::<MONITORINFO>() as u32;
    GetMonitorInfoW(monitor, &mut info);
    let area = info.rcWork;
    let hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_CONTROLPARENT,
        class.as_ptr(),
        wide("自定义颜色").as_ptr(),
        WS_CAPTION | WS_SYSMENU,
        area.left + (area.right - area.left - (r.right - r.left)) / 2,
        area.top + (area.bottom - area.top - (r.bottom - r.top)) / 2,
        r.right - r.left,
        r.bottom - r.top,
        owner,
        0,
        wc.hInstance,
        window::State::param(&p),
    );
    if hwnd == 0 {
        error(owner, "无法创建颜色选择窗口");
        return None;
    }
    p.value.borrow_mut().fields[0] = field(hwnd, 10, &hex(color), 142, 82, 248, font);
    for i in 0..3 {
        p.value.borrow_mut().fields[i + 1] =
            field(hwnd, 11 + i, "", 142 + i as i32 * 90, 140, 60, font);
    }
    sync(&mut p.value.borrow_mut(), None);
    for (id, label, x, w) in [
        (3, "打开色板", 20, 96),
        (2, "取消", 244, 72),
        (1, "确定", 328, 72),
    ] {
        let button = CreateWindowExW(
            0,
            wide("BUTTON").as_ptr(),
            wide(label).as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW as u32,
            x,
            232,
            w,
            32,
            hwnd,
            id,
            GetModuleHandleW(null()),
            std::ptr::null_mut(),
        );
        ui::track_control(button);
    }
    {
        let mut picker = p.value.borrow_mut();
        picker.layout.capture(hwnd);
        let Picker { layout, font, .. } = &mut *picker;
        layout.apply(hwnd, font, (420, 280), dpi::current(hwnd), None);
    }
    ui::set_window_icon(hwnd);
    Some((hwnd, p))
}
pub unsafe fn show(owner: HWND, color: u32, custom: &mut [u32; 16]) -> Option<u32> {
    let (hwnd, p) = create(owner, color, *custom)?;
    EnableWindow(owner, 0);
    ShowWindow(hwnd, SW_SHOW);
    SetForegroundWindow(hwnd);
    let first = p.value.borrow().fields[0];
    SetFocus(first);
    SendMessageW(first, 0x00b1, 0, -1);
    let mut msg: MSG = zeroed();
    while IsWindow(hwnd) != 0 {
        let status = GetMessageW(&mut msg, 0, 0, 0);
        if status <= 0 {
            if status == 0 {
                PostQuitMessage(msg.wParam as i32);
            }
            break;
        }
        if IsDialogMessageW(hwnd, &msg) == 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    if IsWindow(hwnd) != 0 {
        DestroyWindow(hwnd);
    }
    EnableWindow(owner, 1);
    SetForegroundWindow(owner);
    SetFocus(owner);
    *custom = p.value.borrow().custom;
    let result = p.value.borrow().result;
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hex_order_and_roundtrip() {
        assert_eq!(parse_hex("#FF8000"), Some(0x000080ff));
        for c in [0, 0xffffff, 0x123456, 0x7f80ff] {
            assert_eq!(parse_hex(&hex(c)), Some(c));
        }
    }
    #[test]
    fn rejects_invalid() {
        for s in ["#fff", "1234567", "GG0000", "", "#12345Z"] {
            assert!(parse_hex(s).is_none());
        }
        assert_eq!(parse_rgb(["255", "128", "0"]), Some(0x80ff));
        for s in [["256", "0", "0"], ["-1", "0", "0"], ["", "0", "0"]] {
            assert!(parse_rgb(s).is_none());
        }
    }
}
#[cfg(test)]
mod interaction_tests {
    use super::*;
    #[test]
    fn system_close_and_alt_f4_cancel_color_picker() {
        unsafe {
            for (message, wp, lp) in [
                (WM_SYSCOMMAND, SC_CLOSE as usize, 0),
                (WM_SYSKEYDOWN, VK_F4 as usize, 1isize << 29),
            ] {
                let (hwnd, p) = create(0, 0x112233, [0xffffff; 16]).unwrap();
                ShowWindow(hwnd, SW_SHOW);
                SetForegroundWindow(hwnd);
                let first = p.value.borrow().fields[0];
                SetFocus(first);
                SendMessageW(hwnd, message, wp, lp);
                let mut msg: MSG = zeroed();
                while PeekMessageW(&mut msg, hwnd, 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                let still_open = IsWindow(hwnd) != 0;
                if still_open {
                    DestroyWindow(hwnd);
                }
                assert!(!still_open, "颜色窗口未响应关闭消息 {message:#x}");
                assert!(p.value.borrow().result.is_none(), "关闭窗口必须按取消处理");
            }
        }
    }
    #[test]
    fn editor_sync_validation_confirm_and_cancel() {
        unsafe {
            let (hwnd, p) = create(0, 0x112233, [0xffffff; 16]).unwrap();
            let fields = p.value.borrow().fields;
            SetWindowTextW(fields[0], wide("#FF8000").as_ptr());
            assert_eq!(p.value.borrow().color, 0x80ff);
            assert_eq!(value(fields[1]), "255");
            assert_eq!(value(fields[2]), "128");
            assert_eq!(value(fields[3]), "0");
            SetWindowTextW(fields[2], wide("256").as_ptr());
            assert!(!p.value.borrow().valid);
            assert_eq!(IsWindowEnabled(GetDlgItem(hwnd, 1)), 0);
            SendMessageW(hwnd, WM_COMMAND, 1, 0);
            assert_ne!(IsWindow(hwnd), 0);
            assert!(p.value.borrow().result.is_none());
            SetWindowTextW(fields[2], wide("64").as_ptr());
            assert!(p.value.borrow().valid);
            assert_ne!(IsWindowEnabled(GetDlgItem(hwnd, 1)), 0);
            assert_eq!(value(fields[0]), "#FF4000");
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            UpdateWindow(hwnd);
            let b = Bitmap::new(420, 280).unwrap();
            assert_ne!(
                windows_sys::Win32::Storage::Xps::PrintWindow(hwnd, b.dc, 1),
                0
            );
            super::super::preview::write_preview(&b, "docs/color-preview.png");
            SendMessageW(hwnd, WM_COMMAND, 1, 0);
            assert_eq!(p.value.borrow().result, Some(0x40ff));
            let (hwnd, p) = create(0, 0x112233, [0xffffff; 16]).unwrap();
            let fields = p.value.borrow().fields;
            SetWindowTextW(fields[0], wide("#123456").as_ptr());
            SendMessageW(hwnd, WM_COMMAND, 2, 0);
            assert!(p.value.borrow().result.is_none());
        }
    }
}
