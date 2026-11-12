//! Icons use 2× supersampling; all UI text is rendered at final pixel resolution.
use super::*;
use windows_sys::Win32::UI::Controls::WM_MOUSELEAVE;
pub const fn rgb(r: u32, g: u32, b: u32) -> u32 {
    r | (g << 8) | (b << 16)
}
pub const COLORS: [u32; 8] = [
    rgb(232, 80, 80),
    rgb(245, 158, 11),
    rgb(250, 204, 21),
    rgb(34, 197, 94),
    rgb(45, 127, 249),
    rgb(139, 92, 246),
    rgb(255, 255, 255),
    rgb(30, 41, 59),
];
pub const INK: u32 = rgb(31, 35, 41);
pub const BLUE: u32 = rgb(51, 112, 255);
pub const MUTED: u32 = rgb(100, 106, 115);
pub const BORDER: u32 = rgb(234, 236, 240);
pub const SURFACE: u32 = rgb(247, 248, 250);
use super::tools::{ORDER, TIPS};
pub unsafe fn rounded(dc: HDC, r: Region, radius: i32, color: u32, stroke: u32) {
    let pen = CreatePen(PS_SOLID, 1, stroke);
    let brush = CreateSolidBrush(color);
    let op = SelectObject(dc, pen);
    let ob = SelectObject(dc, brush);
    RoundRect(dc, r.x, r.y, r.x + r.w, r.y + r.h, radius * 2, radius * 2);
    SelectObject(dc, op);
    SelectObject(dc, ob);
    DeleteObject(pen);
    DeleteObject(brush);
}
struct CachedFont(HFONT);
impl Drop for CachedFont {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.0);
        }
    }
}
thread_local! { static FONTS: std::cell::RefCell<std::collections::HashMap<(i32,bool,bool),CachedFont>> = std::cell::RefCell::new(std::collections::HashMap::new()); }
fn cached_font(s: &str, size: i32, bold: bool) -> HFONT {
    unsafe {
        FONTS.with(|cache| {
            let mut cache = cache.borrow_mut();
            cache
                .entry((size, bold, s.is_ascii()))
                .or_insert_with(|| {
                    CachedFont(CreateFontW(
                        -size,
                        0,
                        0,
                        0,
                        if bold { FW_SEMIBOLD } else { FW_NORMAL } as i32,
                        0,
                        0,
                        0,
                        1,
                        0,
                        0,
                        CLEARTYPE_QUALITY as u32,
                        0,
                        wide(if s.is_ascii() {
                            "Segoe UI"
                        } else {
                            "Microsoft YaHei UI"
                        })
                        .as_ptr(),
                    ))
                })
                .0
        })
    }
}
// All text is rasterized at the destination DPI, never resized as a bitmap.
pub(super) unsafe fn label(dc: HDC, x: i32, y: i32, s: &str, size: i32, color: u32, bold: bool) {
    let old = SelectObject(dc, cached_font(s, size, bold));
    SetTextColor(dc, color);
    SetBkMode(dc, TRANSPARENT as i32);
    text(dc, x, y, s);
    SelectObject(dc, old);
}
#[allow(clippy::too_many_arguments)]
pub unsafe fn text_in(
    dc: HDC,
    r: Region,
    s: &str,
    size: i32,
    color: u32,
    bold: bool,
    center: bool,
) {
    let old = SelectObject(dc, cached_font(s, size, bold));
    SetTextColor(dc, color);
    SetBkMode(dc, TRANSPARENT as i32);
    let mut bounds = RECT {
        left: r.x,
        top: r.y,
        right: r.x + r.w,
        bottom: r.y + r.h,
    };
    let value = wide(s);
    DrawTextW(
        dc,
        value.as_ptr(),
        (value.len() - 1) as i32,
        &mut bounds,
        DT_SINGLELINE
            | DT_VCENTER
            | DT_NOPREFIX
            | DT_END_ELLIPSIS
            | if center { DT_CENTER } else { DT_LEFT },
    );
    SelectObject(dc, old);
}
pub unsafe fn keycap(dc: HDC, right: i32, y: i32, key: &str) {
    let old = SelectObject(dc, cached_font(key, 12, false));
    let text = wide(key);
    let mut size: SIZE = zeroed();
    GetTextExtentPoint32W(dc, text.as_ptr(), (text.len() - 1) as i32, &mut size);
    SelectObject(dc, old);
    let r = Region {
        x: right - (size.cx + 16).max(26),
        y,
        w: (size.cx + 16).max(26),
        h: 24,
    };
    rounded(dc, r, 4, SURFACE, rgb(222, 225, 230));
    text_in(dc, r, key, 12, INK, false, true);
}
pub unsafe fn input_border(dc: HDC, r: Region, focused: bool, hover: bool) {
    rounded(
        dc,
        r,
        4,
        rgb(255, 255, 255),
        if focused {
            BLUE
        } else if hover {
            rgb(166, 174, 186)
        } else {
            rgb(210, 215, 223)
        },
    );
}
const INTERACTION_ID: usize = 0x514a;
pub unsafe fn track_control(hwnd: HWND) {
    SetWindowSubclass(hwnd, Some(interaction_proc), INTERACTION_ID, 0);
}
pub unsafe fn is_hot(hwnd: HWND) -> bool {
    let mut hot = 0;
    GetWindowSubclass(hwnd, Some(interaction_proc), INTERACTION_ID, &mut hot);
    hot != 0
}
unsafe extern "system" fn interaction_proc(
    hwnd: HWND,
    msg: u32,
    wp: usize,
    lp: isize,
    _id: usize,
    hot: usize,
) -> isize {
    match msg {
        WM_MOUSEMOVE if hot == 0 => {
            SetWindowSubclass(hwnd, Some(interaction_proc), INTERACTION_ID, 1);
            let mut tracking = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            TrackMouseEvent(&mut tracking);
            InvalidateRect(hwnd, null(), 0);
            InvalidateRect(GetParent(hwnd), null(), 0);
        }
        WM_MOUSELEAVE => {
            SetWindowSubclass(hwnd, Some(interaction_proc), INTERACTION_ID, 0);
            InvalidateRect(hwnd, null(), 0);
            InvalidateRect(GetParent(hwnd), null(), 0);
        }
        WM_SETFOCUS | WM_KILLFOCUS => {
            InvalidateRect(hwnd, null(), 0);
            InvalidateRect(GetParent(hwnd), null(), 0);
        }
        WM_NCDESTROY => {
            RemoveWindowSubclass(hwnd, Some(interaction_proc), INTERACTION_ID);
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wp, lp)
}
unsafe fn line(dc: HDC, points: &[(i32, i32)]) {
    if let Some((x, y)) = points.first() {
        MoveToEx(dc, *x, *y, std::ptr::null_mut());
        for (x, y) in &points[1..] {
            LineTo(dc, *x, *y);
        }
    }
}
pub(super) unsafe fn icon(dc: HDC, id: usize, x: i32, y: i32, color: u32) {
    let saved = SaveDC(dc);
    SetWindowOrgEx(dc, -x, -y, std::ptr::null_mut());
    let pen = CreatePen(PS_SOLID, 2, color);
    let op = SelectObject(dc, pen);
    let ob = SelectObject(dc, GetStockObject(NULL_BRUSH));
    match id {
        0 => {
            line(dc, &[(6, 2), (6, 18), (22, 18)]);
            line(dc, &[(2, 6), (18, 6), (18, 22)]);
        }
        1 => {
            line(dc, &[(4, 20), (6, 13), (17, 2), (22, 7), (11, 18), (4, 20)]);
            line(dc, &[(14, 5), (19, 10)]);
        }
        2 => {
            RoundRect(dc, 3, 4, 22, 21, 4, 4);
        }
        3 => {
            Ellipse(dc, 3, 3, 22, 22);
        }
        4 => {
            line(dc, &[(4, 20), (21, 3), (10, 3)]);
            line(dc, &[(21, 3), (21, 14)]);
        }
        5 => {
            Rectangle(dc, 3, 4, 22, 21);
            for y in [8, 12, 16] {
                line(dc, &[(6, y), (19, y)]);
            }
        }
        6 => {
            line(dc, &[(3, 6), (3, 3), (21, 3), (21, 6)]);
            line(dc, &[(12, 3), (12, 21)]);
            line(dc, &[(7, 21), (17, 21)]);
        }
        8 => {
            line(dc, &[(8, 3), (3, 8), (8, 13)]);
            line(
                dc,
                &[(4, 8), (15, 8), (20, 12), (20, 17), (16, 21), (9, 21)],
            );
        }
        9 => {
            line(dc, &[(16, 3), (21, 8), (16, 13)]);
            line(dc, &[(20, 8), (9, 8), (4, 12), (4, 17), (8, 21), (15, 21)]);
        }
        10 => {
            line(dc, &[(12, 2), (12, 16), (7, 11)]);
            line(dc, &[(12, 16), (17, 11)]);
            line(dc, &[(3, 16), (3, 22), (21, 22), (21, 16)]);
        }
        11 => {
            line(
                dc,
                &[
                    (8, 2),
                    (20, 14),
                    (16, 14),
                    (11, 19),
                    (5, 13),
                    (10, 8),
                    (8, 2),
                ],
            );
            line(dc, &[(8, 16), (2, 23)]);
        }
        12 => line(dc, &[(3, 12), (9, 18), (22, 5)]),
        13 => {
            for yy in 0..3 {
                for xx in 0..3 {
                    if (xx + yy) % 2 == 0 {
                        fill(
                            dc,
                            Region {
                                x: 3 + xx * 7,
                                y: 3 + yy * 7,
                                w: 5,
                                h: 5,
                            },
                            color,
                        );
                    } else {
                        Rectangle(dc, 3 + xx * 7, 3 + yy * 7, 8 + xx * 7, 8 + yy * 7);
                    }
                }
            }
        }
        14 => {
            line(dc, &[(6, 6), (19, 19)]);
            line(dc, &[(19, 6), (6, 19)]);
        }
        _ => {}
    }
    SelectObject(dc, op);
    SelectObject(dc, ob);
    DeleteObject(pen);
    RestoreDC(dc, saved);
}
// Render only the small chrome surface at 2×; desktop pixels remain at native resolution.
unsafe fn panel(dc: HDC, r: Region) -> Result<Bitmap, String> {
    let p = Bitmap::new(r.w * 2, r.h * 2)?;
    SetStretchBltMode(p.dc, COLORONCOLOR);
    StretchBlt(
        p.dc,
        0,
        0,
        r.w * 2,
        r.h * 2,
        dc,
        r.x,
        r.y,
        r.w,
        r.h,
        SRCCOPY,
    );
    SetMapMode(p.dc, MM_ANISOTROPIC);
    SetWindowExtEx(p.dc, r.w, r.h, std::ptr::null_mut());
    SetViewportExtEx(p.dc, r.w * 2, r.h * 2, std::ptr::null_mut());
    rounded(
        p.dc,
        Region {
            x: 1,
            y: 1,
            w: r.w - 2,
            h: r.h - 2,
        },
        6,
        rgb(255, 255, 255),
        rgb(222, 225, 230),
    );
    Ok(p)
}
unsafe fn present(dc: HDC, r: Region, p: &Bitmap) {
    SetMapMode(p.dc, MM_TEXT);
    SetWindowOrgEx(p.dc, 0, 0, std::ptr::null_mut());
    SetViewportOrgEx(p.dc, 0, 0, std::ptr::null_mut());
    let old = SetStretchBltMode(dc, HALFTONE);
    StretchBlt(
        dc,
        r.x,
        r.y,
        r.w,
        r.h,
        p.dc,
        0,
        0,
        r.w * 2,
        r.h * 2,
        SRCCOPY,
    );
    SetStretchBltMode(dc, old);
}
type Buttons = Vec<(usize, Region)>;
#[allow(clippy::too_many_arguments)]
pub unsafe fn toolbar(
    dc: HDC,
    selection: Region,
    w: i32,
    h: i32,
    tool: usize,
    color: u32,
    hover: Option<usize>,
    no_undo: bool,
    no_redo: bool,
    stroke_width: i32,
) -> Result<(Region, Buttons), String> {
    let columns = ((w - 32) / 38).clamp(4, 14);
    let rows = (14 + columns - 1) / columns;
    let width = columns * 38 + 20;
    let height = rows * 38 + 40;
    let x = (selection.x + selection.w - width).clamp(8, (w - width - 8).max(8));
    let y = if selection.y + selection.h + height + 14 < h {
        selection.y + selection.h + 12
    } else if selection.y > height + 14 {
        selection.y - height - 12
    } else {
        (selection.y + selection.h - height - 12).clamp(8, (h - height - 8).max(8))
    };
    let r = Region {
        x,
        y,
        w: width,
        h: height,
    };
    let p = panel(dc, r)?;
    let mut buttons = vec![];
    let mut labels: Vec<(Region, String, i32, u32, bool)> = vec![];
    for (i, id) in ORDER.iter().enumerate() {
        let bx = 10 + i as i32 % columns * 38;
        let by = 5 + i as i32 / columns * 38;
        let disabled = (*id == 8 && no_undo) || (*id == 9 && no_redo);
        let active = *id == tool;
        let bg = if *id == 12 {
            BLUE
        } else if active {
            rgb(232, 240, 255)
        } else if hover == Some(*id) && !disabled {
            rgb(243, 245, 248)
        } else {
            rgb(255, 255, 255)
        };
        rounded(
            p.dc,
            Region {
                x: bx,
                y: by,
                w: 32,
                h: 32,
            },
            4,
            bg,
            bg,
        );
        let fg = if *id == 12 {
            rgb(255, 255, 255)
        } else if disabled {
            rgb(192, 200, 211)
        } else if active {
            BLUE
        } else {
            INK
        };
        icon(p.dc, *id, bx + 4, by + 4, fg);
        if [0, 7, 9, 11].contains(&i) && (i as i32 + 1) % columns != 0 {
            fill(
                p.dc,
                Region {
                    x: bx + 35,
                    y: by + 9,
                    w: 1,
                    h: 20,
                },
                rgb(229, 234, 241),
            );
        }
        if !disabled {
            buttons.push((
                *id,
                Region {
                    x: x + bx,
                    y: y + by,
                    w: 32,
                    h: 32,
                },
            ));
        }
    }
    let bottom = rows * 38 + 2;
    fill(
        p.dc,
        Region {
            x: 16,
            y: bottom - 3,
            w: width - 32,
            h: 1,
        },
        rgb(237, 240, 245),
    );
    for (i, c) in COLORS.iter().enumerate() {
        let cx = 18 + i as i32 * 21;
        if *c == color {
            rounded(
                p.dc,
                Region {
                    x: cx - 3,
                    y: bottom + 5,
                    w: 19,
                    h: 19,
                },
                1,
                rgb(255, 255, 255),
                BLUE,
            );
        }
        rounded(
            p.dc,
            Region {
                x: cx,
                y: bottom + 8,
                w: 13,
                h: 13,
            },
            1,
            *c,
            if i == 6 { rgb(208, 216, 225) } else { *c },
        );
        buttons.push((
            100 + i,
            Region {
                x: x + cx - 3,
                y: y + bottom + 5,
                w: 19,
                h: 19,
            },
        ));
    }
    let custom = Region {
        x: 186,
        y: bottom + 5,
        w: 20,
        h: 20,
    };
    rounded(
        p.dc,
        custom,
        3,
        if COLORS.contains(&color) {
            rgb(255, 255, 255)
        } else {
            color
        },
        if hover == Some(7) || !COLORS.contains(&color) {
            BLUE
        } else {
            rgb(180, 190, 204)
        },
    );
    let fg = if !COLORS.contains(&color)
        && ((color & 255) * 299 + ((color >> 8) & 255) * 587 + ((color >> 16) & 255) * 114) < 128000
    {
        rgb(255, 255, 255)
    } else {
        INK
    };
    labels.push((custom, "+".into(), 13, fg, true));
    buttons.push((
        7,
        Region {
            x: r.x + custom.x,
            y: r.y + custom.y,
            ..custom
        },
    ));
    if width > 330 {
        labels.push((
            Region {
                x: 210,
                y: bottom + 5,
                w: 40,
                h: 22,
            },
            format!("{} px", stroke_width),
            12,
            INK,
            true,
        ));
        for (id, x, symbol) in [(200, 254, "−"), (201, 278, "+")] {
            rounded(
                p.dc,
                Region {
                    x,
                    y: bottom + 5,
                    w: 20,
                    h: 22,
                },
                1,
                rgb(255, 255, 255),
                rgb(191, 196, 201),
            );
            labels.push((
                Region {
                    x,
                    y: bottom + 5,
                    w: 20,
                    h: 22,
                },
                symbol.to_string(),
                12,
                INK,
                true,
            ));
            buttons.push((
                id,
                Region {
                    x: r.x + x,
                    y: r.y + bottom + 5,
                    w: 20,
                    h: 22,
                },
            ));
        }
    }
    if width > 490 {
        let tip = hover
            .filter(|v| *v < 15)
            .map(|v| TIPS[v])
            .unwrap_or("Enter 复制     Esc 取消");
        labels.push((
            Region {
                x: 314,
                y: bottom + 5,
                w: width - 328,
                h: 22,
            },
            tip.to_string(),
            12,
            MUTED,
            false,
        ));
    }
    present(dc, r, &p);
    for (bounds, s, size, color, center) in labels {
        text_in(
            dc,
            Region {
                x: r.x + bounds.x,
                y: r.y + bounds.y,
                ..bounds
            },
            &s,
            size,
            color,
            false,
            center,
        );
    }
    Ok((r, buttons))
}
pub unsafe fn selection(dc: HDC, r: Region, w: i32, pixels: (i32, i32)) {
    let pen = CreatePen(PS_SOLID, 1, BLUE);
    let op = SelectObject(dc, pen);
    let ob = SelectObject(dc, GetStockObject(NULL_BRUSH));
    Rectangle(dc, r.x - 1, r.y - 1, r.x + r.w + 1, r.y + r.h + 1);
    SelectObject(dc, op);
    SelectObject(dc, ob);
    DeleteObject(pen);
    // Visible corner bounds; drag edges in selection mode to resize.
    let pen = CreatePen(PS_SOLID, 3, rgb(255, 255, 255));
    let op = SelectObject(dc, pen);
    for (x, y, dx, dy) in [
        (r.x, r.y, 1, 1),
        (r.x + r.w, r.y, -1, 1),
        (r.x, r.y + r.h, 1, -1),
        (r.x + r.w, r.y + r.h, -1, -1),
    ] {
        line(dc, &[(x, y + dy * 12), (x, y), (x + dx * 12, y)]);
    }
    SelectObject(dc, op);
    DeleteObject(pen);
    let x = r.x.clamp(4, (w - 160).max(4));
    let y = if r.y > 38 { r.y - 34 } else { r.y + 8 };
    rounded(
        dc,
        Region {
            x,
            y,
            w: 128,
            h: 22,
        },
        1,
        rgb(255, 255, 255),
        rgb(218, 225, 234),
    );
    label(
        dc,
        x + 9,
        y + 3,
        &format!("{} × {} px", pixels.0, pixels.1),
        12,
        INK,
        false,
    );
}
pub unsafe fn welcome(dc: HDC, w: i32, h: i32) -> Result<(), String> {
    let width = 350.min(w - 20);
    let r = Region {
        x: (w - width) / 2,
        y: (h - 66).max(10),
        w: width,
        h: 42,
    };
    let p = panel(dc, r)?;
    icon(p.dc, 0, 12, 9, BLUE);
    present(dc, r, &p);
    label(
        dc,
        r.x + 47,
        r.y + 14,
        "拖动选择区域   ·   Enter 复制   ·   Esc 取消",
        12,
        INK,
        false,
    );
    Ok(())
}

pub unsafe fn app_icon() -> HICON {
    static ICON: std::sync::OnceLock<isize> = std::sync::OnceLock::new();
    *ICON.get_or_init(|| {
        let icon = LoadIconW(GetModuleHandleW(null()), resource_id(1));
        if icon != 0 {
            icon
        } else {
            tray_icon()
        }
    })
}
pub unsafe fn set_window_icon(hwnd: HWND) {
    SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, app_icon());
    SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, app_icon());
}
pub unsafe fn button(
    hwnd: HWND,
    d: &windows_sys::Win32::UI::Controls::DRAWITEMSTRUCT,
    label: &str,
) {
    let dpi = dpi::current(hwnd);
    let saved = dpi::map(d.hDC, dpi);
    let r = Region {
        x: dpi::logical(d.rcItem.left, dpi),
        y: dpi::logical(d.rcItem.top, dpi),
        w: dpi::logical(d.rcItem.right - d.rcItem.left, dpi),
        h: dpi::logical(d.rcItem.bottom - d.rcItem.top, dpi),
    };
    button_face(
        d.hDC,
        r,
        label,
        d.CtlID == 1,
        d.itemState,
        is_hot(d.hwndItem),
    );
    RestoreDC(d.hDC, saved);
}
pub unsafe fn button_face(dc: HDC, r: Region, label: &str, primary: bool, state: u32, hover: bool) {
    use windows_sys::Win32::UI::Controls::{
        ODS_DISABLED, ODS_FOCUS, ODS_NOFOCUSRECT, ODS_SELECTED,
    };
    let disabled = state & ODS_DISABLED != 0;
    let pressed = state & ODS_SELECTED != 0;
    let bg = if disabled {
        SURFACE
    } else if primary {
        if pressed {
            rgb(31, 84, 205)
        } else if hover {
            rgb(43, 100, 230)
        } else {
            BLUE
        }
    } else if pressed {
        rgb(232, 235, 240)
    } else if hover {
        SURFACE
    } else {
        rgb(255, 255, 255)
    };
    rounded(
        dc,
        r,
        4,
        bg,
        if primary && !disabled {
            bg
        } else {
            rgb(210, 215, 223)
        },
    );
    text_in(
        dc,
        r,
        label,
        13,
        if disabled {
            rgb(151, 158, 168)
        } else if primary {
            rgb(255, 255, 255)
        } else {
            INK
        },
        false,
        true,
    );
    if state & ODS_FOCUS != 0 && state & ODS_NOFOCUSRECT == 0 {
        DrawFocusRect(
            dc,
            &RECT {
                left: r.x + 4,
                top: r.y + 4,
                right: r.x + r.w - 4,
                bottom: r.y + r.h - 4,
            },
        );
    }
}
pub unsafe fn navigation(dc: HDC, page: usize, selected: bool, hover: bool, focused: bool) {
    let bg = if selected {
        rgb(232, 240, 255)
    } else if hover {
        rgb(237, 240, 244)
    } else {
        SURFACE
    };
    rounded(
        dc,
        Region {
            x: 0,
            y: 0,
            w: 124,
            h: 32,
        },
        4,
        bg,
        bg,
    );
    let fg = if selected { BLUE } else { MUTED };
    let saved = SaveDC(dc);
    SetWindowOrgEx(dc, -12, -8, std::ptr::null_mut());
    let pen = CreatePen(PS_SOLID, 1, fg);
    let op = SelectObject(dc, pen);
    let ob = SelectObject(dc, GetStockObject(NULL_BRUSH));
    match page {
        0 => {
            RoundRect(dc, 0, 2, 16, 14, 3, 3);
            for y in [5, 8] {
                for x in [3, 6, 9, 12] {
                    SetPixel(dc, x, y, fg);
                }
            }
            MoveToEx(dc, 4, 11, std::ptr::null_mut());
            LineTo(dc, 12, 11);
        }
        1 => {
            line(dc, &[(1, 14), (3, 8), (11, 0), (15, 4), (7, 12), (1, 14)]);
            line(dc, &[(9, 2), (13, 6)]);
        }
        _ => {
            Ellipse(dc, 0, 0, 16, 16);
            text_in(
                dc,
                Region {
                    x: 0,
                    y: 0,
                    w: 16,
                    h: 16,
                },
                "?",
                12,
                fg,
                false,
                true,
            );
        }
    }
    SelectObject(dc, ob);
    SelectObject(dc, op);
    DeleteObject(pen);
    RestoreDC(dc, saved);
    text_in(
        dc,
        Region {
            x: 36,
            y: 0,
            w: 82,
            h: 32,
        },
        ["全局快捷键", "编辑快捷键", "使用帮助"][page],
        13,
        fg,
        false,
        false,
    );
    if focused {
        DrawFocusRect(
            dc,
            &RECT {
                left: 3,
                top: 3,
                right: 121,
                bottom: 29,
            },
        );
    }
}

pub unsafe fn tray_icon() -> HICON {
    let mut pixels = [0u8; 32 * 32 * 4];
    let mut mask = [0u8; 128];
    for y in 0i32..32 {
        for x in 0i32..32 {
            let dx = if x < 7 { 7 - x } else { (x - 24).max(0) };
            let dy = if y < 7 { 7 - y } else { (y - 24).max(0) };
            let inside = dx * dx + dy * dy <= 49;
            let i = ((y * 32 + x) * 4) as usize;
            if inside {
                let white = ((x == 9 || x == 10) && (6..=22).contains(&y))
                    || ((y == 21 || y == 22) && (9..=26).contains(&x))
                    || ((y == 9 || y == 10) && (6..=22).contains(&x))
                    || ((x == 21 || x == 22) && (9..=26).contains(&y));
                pixels[i..i + 4].copy_from_slice(if white {
                    &[255, 255, 255, 255]
                } else {
                    &[242, 112, 45, 255]
                });
            } else {
                mask[(y * 4 + x / 8) as usize] |= 0x80 >> (x % 8);
            }
        }
    }
    CreateIcon(
        GetModuleHandleW(null()),
        32,
        32,
        1,
        32,
        mask.as_ptr(),
        pixels.as_ptr(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn button_caption_is_visually_centered() {
        unsafe {
            for dpi in [96, 120, 144, 192] {
                for (width, caption) in [
                    (72, "确定"),
                    (72, "取消"),
                    (104, "保存并应用"),
                    (96, "打开色板"),
                    (88, "恢复默认"),
                    (96, "Ctrl+S"),
                ] {
                    let w = dpi::scale(width, dpi);
                    let h = dpi::scale(32, dpi);
                    let b = Bitmap::new(w, h).unwrap();
                    fill(b.dc, Region { x: 0, y: 0, w, h }, BLUE);
                    let saved = dpi::map(b.dc, dpi);
                    button_face(
                        b.dc,
                        Region {
                            x: 0,
                            y: 0,
                            w: width,
                            h: 32,
                        },
                        caption,
                        true,
                        0,
                        false,
                    );
                    RestoreDC(b.dc, saved);
                    let bytes = b.pixels().unwrap();
                    let mut bounds = (w, h, 0, 0);
                    for y in 0..h {
                        for x in 0..w {
                            let i = ((y * w + x) * 4) as usize;
                            if bytes[i] > 220 && bytes[i + 1] > 220 && bytes[i + 2] > 220 {
                                bounds.0 = bounds.0.min(x);
                                bounds.1 = bounds.1.min(y);
                                bounds.2 = bounds.2.max(x);
                                bounds.3 = bounds.3.max(y);
                            }
                        }
                    }
                    assert!(bounds.0 <= bounds.2, "按钮必须绘制文字");
                    let dx = (bounds.0 + bounds.2 - (w - 1)).abs();
                    let dy = (bounds.1 + bounds.3 - (h - 1)).abs();
                    assert!(
                        dx <= dpi::scale(4, dpi) && dy <= dpi::scale(4, dpi),
                        "{caption}, {dpi} DPI: 文字边界 {bounds:?}；中心偏移 ({}, {}) px",
                        dx as f32 / 2.,
                        dy as f32 / 2.
                    );
                }
            }
            let b = Bitmap::new(536, 144).unwrap();
            fill(
                b.dc,
                Region {
                    x: 0,
                    y: 0,
                    w: 536,
                    h: 144,
                },
                rgb(255, 255, 255),
            );
            use windows_sys::Win32::UI::Controls::{ODS_DISABLED, ODS_SELECTED};
            for (i, (name, state, hover)) in [
                ("默认", 0, false),
                ("悬停", 0, true),
                ("按下", ODS_SELECTED, false),
                ("禁用", ODS_DISABLED, false),
            ]
            .iter()
            .enumerate()
            {
                let x = 16 + i as i32 * 132;
                text_in(
                    b.dc,
                    Region {
                        x,
                        y: 8,
                        w: 108,
                        h: 24,
                    },
                    name,
                    12,
                    MUTED,
                    false,
                    true,
                );
                button_face(
                    b.dc,
                    Region {
                        x,
                        y: 40,
                        w: 108,
                        h: 32,
                    },
                    "保存并应用",
                    true,
                    *state,
                    *hover,
                );
                button_face(
                    b.dc,
                    Region {
                        x,
                        y: 88,
                        w: 108,
                        h: 32,
                    },
                    "打开色板",
                    false,
                    *state,
                    *hover,
                );
            }
            super::super::preview::write_preview(&b, "docs/controls-preview.png");
        }
    }
    #[test]
    fn toolbar_buttons_fit_and_disabled_undo_is_not_clickable() {
        unsafe {
            for (w, h) in [(1280, 800), (640, 480), (360, 640)] {
                let b = Bitmap::new(w, h).unwrap();
                fill(b.dc, Region { x: 0, y: 0, w, h }, rgb(220, 220, 220));
                let (bar, buttons) = toolbar(
                    b.dc,
                    Region {
                        x: w - 180,
                        y: h - 160,
                        w: 170,
                        h: 150,
                    },
                    w,
                    h,
                    4,
                    COLORS[0],
                    None,
                    true,
                    true,
                    3,
                )
                .unwrap();
                assert!(bar.x >= 0 && bar.y >= 0 && bar.x + bar.w <= w && bar.y + bar.h <= h);
                assert!(!buttons.iter().any(|(id, _)| *id == 8 || *id == 9));
                for (_, r) in buttons {
                    assert!(bar.contains(Point { x: r.x, y: r.y }));
                    assert!(bar.contains(Point {
                        x: r.x + r.w - 1,
                        y: r.y + r.h - 1
                    }));
                }
            }
        }
    }
}
