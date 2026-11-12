use windows_sys::Win32::UI::Controls::DRAWITEMSTRUCT;
// Persistent global shortcuts and transactional registration.
use super::*;
use std::path::{Path, PathBuf};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub modifiers: u32,
    pub key: u32,
}
impl Hotkey {
    pub fn parse(s: &str) -> Result<Self, String> {
        let mut modifiers = 0;
        let mut key = None;
        for part in s.split('+').map(|s| s.trim().to_ascii_uppercase()) {
            let m = match part.as_str() {
                "CTRL" | "CONTROL" => Some(MOD_CONTROL),
                "ALT" => Some(MOD_ALT),
                "SHIFT" => Some(MOD_SHIFT),
                _ => None,
            };
            if let Some(m) = m {
                if modifiers & m != 0 {
                    return Err("修饰键不能重复".into());
                }
                modifiers |= m;
                continue;
            }
            if key.is_some() {
                return Err("每组快捷键只能包含一个主键".into());
            }
            key = Some(if part == "PRINTSCREEN" || part == "PRTSC" {
                VK_SNAPSHOT as u32
            } else if let Some(f) = part
                .strip_prefix('F')
                .and_then(|v| v.parse::<u32>().ok())
                .filter(|f| (1..=24).contains(f))
            {
                VK_F1 as u32 + f - 1
            } else if part.len() == 1 && part.as_bytes()[0].is_ascii_alphanumeric() {
                part.as_bytes()[0] as u32
            } else {
                return Err("支持 A–Z、0–9、F1–F24、PrintScreen".into());
            });
        }
        let key = key.ok_or("请指定主键，例如 Ctrl+Alt+A")?;
        if modifiers & (MOD_CONTROL | MOD_ALT) == 0
            && !(VK_F1 as u32..=VK_F24 as u32).contains(&key)
            && key != VK_SNAPSHOT as u32
        {
            return Err("字母或数字快捷键需包含 Ctrl 或 Alt，避免影响打字".into());
        }
        Ok(Self { modifiers, key })
    }
    pub fn label(self) -> String {
        let mut parts = vec![];
        if self.modifiers & MOD_CONTROL != 0 {
            parts.push("Ctrl".into());
        }
        if self.modifiers & MOD_ALT != 0 {
            parts.push("Alt".into());
        }
        if self.modifiers & MOD_SHIFT != 0 {
            parts.push("Shift".into());
        }
        parts.push(if (VK_F1 as u32..=VK_F24 as u32).contains(&self.key) {
            format!("F{}", self.key - VK_F1 as u32 + 1)
        } else if self.key == VK_SNAPSHOT as u32 {
            "PrintScreen".into()
        } else {
            char::from_u32(self.key).unwrap_or('?').to_string()
        });
        parts.join("+")
    }
}
pub const NAMES: [&str; 4] = ["区域截图", "全屏截图", "上次区域", "延时截图"];
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub keys: [Hotkey; 4],
}
impl Default for Config {
    fn default() -> Self {
        Self {
            keys: ["Ctrl+Alt+A", "Ctrl+Alt+F", "Ctrl+Alt+R", "Ctrl+Alt+D"]
                .map(|s| Hotkey::parse(s).unwrap()),
        }
    }
}
impl Config {
    pub fn from_labels(labels: [String; 4]) -> Result<Self, String> {
        let mut c = Self::default();
        for (i, label) in labels.iter().enumerate() {
            c.keys[i] = Hotkey::parse(label).map_err(|e| format!("{}：{}", NAMES[i], e))?;
        }
        for (i, _) in NAMES.iter().enumerate() {
            for (j, _) in NAMES.iter().enumerate().take(i) {
                if c.keys[i] == c.keys[j] {
                    return Err(format!("{}和{}不能使用同一快捷键", NAMES[i], NAMES[j]));
                }
            }
        }
        Ok(c)
    }
    fn encode(&self) -> String {
        self.keys
            .iter()
            .enumerate()
            .map(|(i, k)| format!("shortcut{}={}\n", i + 1, k.label()))
            .collect()
    }
    fn decode(s: &str) -> Result<Self, String> {
        let mut rows: [Option<String>; 4] = std::array::from_fn(|_| None);
        for line in s.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (k, v) = line.split_once('=').ok_or("配置格式不正确")?;
            let i = k
                .strip_prefix("shortcut")
                .and_then(|s| s.parse::<usize>().ok())
                .filter(|i| (1..=4).contains(i))
                .ok_or("未知配置项")?
                - 1;
            if rows[i].is_some() {
                return Err("配置项重复".into());
            }
            rows[i] = Some(v.into());
        }
        Self::from_labels([
            rows[0].take().ok_or("缺少区域快捷键")?,
            rows[1].take().ok_or("缺少全屏快捷键")?,
            rows[2].take().ok_or("缺少上次区域快捷键")?,
            rows[3].take().ok_or("缺少延时快捷键")?,
        ])
    }
    pub fn path() -> PathBuf {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("Qingjie/settings.ini")
    }
    pub fn load() -> Result<Self, String> {
        match std::fs::read_to_string(Self::path()) {
            Ok(s) => Self::decode(&s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.to_string()),
        }
    }
    pub fn save_at(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        use std::io::Write;
        export::atomic_write(path, |file| {
            file.write_all(self.encode().as_bytes())
                .map_err(|e| e.to_string())
        })
    }
}
#[derive(Default)]
pub struct Registry {
    pub slots: Vec<(i32, Hotkey)>,
}
impl Registry {
    pub unsafe fn apply(&mut self, hwnd: HWND, c: &Config, persist: bool) -> Result<(), String> {
        let mut staged = vec![];
        let mut next = vec![];
        for (i, k) in c.keys.iter().enumerate() {
            if let Some(slot) = self.slots.iter().find(|(_, old)| old == k) {
                next.push(*slot);
                continue;
            }
            let id = (100..1000)
                .find(|id| {
                    !self
                        .slots
                        .iter()
                        .chain(staged.iter())
                        .any(|(old, _)| old == id)
                })
                .unwrap();
            if RegisterHotKey(hwnd, id, k.modifiers | MOD_NOREPEAT, k.key) == 0 {
                for (id, _) in &staged {
                    UnregisterHotKey(hwnd, *id);
                }
                return Err(format!(
                    "{}（{}）被其他程序占用或系统拒绝。原设置保持不变。",
                    NAMES[i],
                    k.label()
                ));
            }
            staged.push((id, *k));
            next.push((id, *k));
        }
        if persist {
            if let Err(e) = c.save_at(&Config::path()) {
                for (id, _) in staged {
                    UnregisterHotKey(hwnd, id);
                }
                return Err(format!("保存配置失败：{e}。原设置保持不变。"));
            }
        }
        for (id, _) in &self.slots {
            if !next.iter().any(|(new, _)| new == id) {
                UnregisterHotKey(hwnd, *id);
            }
        }
        self.slots = next;
        Ok(())
    }
    pub fn action(&self, id: usize) -> Option<usize> {
        self.slots
            .iter()
            .position(|(registered, _)| *registered as usize == id)
    }
    pub unsafe fn clear(&mut self, hwnd: HWND) {
        for (id, _) in self.slots.drain(..) {
            UnregisterHotKey(hwnd, id);
        }
    }
}
struct Dialog {
    owner: HWND,
    owner_state: std::rc::Weak<window::State<App>>,
    edits: [HWND; 4],
    font: HFONT,
    layout: dpi::Layout,
    page: usize,
}
impl Drop for Dialog {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.font);
        }
    }
}
#[allow(clippy::too_many_arguments)]
unsafe fn control(
    hwnd: HWND,
    class: &str,
    label: &str,
    style: u32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    id: isize,
) -> HWND {
    let child = CreateWindowExW(
        0,
        wide(class).as_ptr(),
        wide(label).as_ptr(),
        WS_CHILD | WS_VISIBLE | style,
        x,
        y,
        w,
        h,
        hwnd,
        id,
        GetModuleHandleW(null()),
        std::ptr::null_mut(),
    );
    SendMessageW(
        child,
        WM_SETFONT,
        window::get::<Dialog>(hwnd).unwrap().value.borrow().font as usize,
        1,
    );
    ui::track_control(child);
    child
}
unsafe extern "system" fn settings_proc(hwnd: HWND, msg: u32, wp: usize, lp: isize) -> isize {
    window::dispatch(hwnd, msg, wp, lp, handle_message)
}
unsafe fn handle_message(
    hwnd: HWND,
    msg: u32,
    wp: usize,
    lp: isize,
    d: &mut Dialog,
) -> Option<isize> {
    Some(match msg {
        WM_ERASEBKGND => 1,
        m if m == WM_APP + 10 => {
            set_page(hwnd, d, wp);
            0
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = zeroed();
            let dc = BeginPaint(hwnd, &mut ps);
            dpi::paint(hwnd, dc, |dc| paint_settings(hwnd, dc, d.page));
            EndPaint(hwnd, &ps);
            0
        }
        WM_DPICHANGED => {
            d.layout.apply(
                hwnd,
                &mut d.font,
                (704, 480),
                wp as u32 & 0xffff,
                Some(*(lp as *const RECT)),
            );
            set_page(hwnd, d, d.page);
            0
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT => {
            SetBkColor(wp as HDC, ui::rgb(255, 255, 255));
            SetTextColor(wp as HDC, ui::rgb(31, 35, 41));
            GetStockObject(WHITE_BRUSH)
        }
        WM_LBUTTONDOWN => {
            let x = dpi::logical(lp as i16 as i32, dpi::current(hwnd));
            let y = dpi::logical((lp >> 16) as i16 as i32, dpi::current(hwnd));
            if (10..132).contains(&x) && (88..202).contains(&y) {
                set_page(hwnd, d, ((y - 88) / 38) as usize);
            }
            0
        }
        WM_DRAWITEM => {
            let draw = &*(lp as *const DRAWITEMSTRUCT);
            if (100..=102).contains(&draw.CtlID) {
                let saved = dpi::map(draw.hDC, dpi::current(hwnd));
                let page = (draw.CtlID - 100) as usize;
                use windows_sys::Win32::UI::Controls::{ODS_FOCUS, ODS_NOFOCUSRECT};
                ui::navigation(
                    draw.hDC,
                    page,
                    d.page == page,
                    ui::is_hot(draw.hwndItem),
                    draw.itemState & ODS_FOCUS != 0 && draw.itemState & ODS_NOFOCUSRECT == 0,
                );
                RestoreDC(draw.hDC, saved);
                return Some(1);
            }
            let label = match draw.CtlID {
                1 => "保存并应用",
                2 => "关闭",
                _ => "恢复默认",
            };
            ui::button(hwnd, draw, label);
            1
        }
        WM_COMMAND => {
            match wp & 0xffff {
                100..=102 => set_page(hwnd, d, (wp & 0xffff) - 100),
                1 => {
                    if d.page != 0 {
                        return Some(0);
                    }
                    let labels = std::array::from_fn(|i| {
                        let mut s = [0u16; 128];
                        let n = GetWindowTextW(d.edits[i], s.as_mut_ptr(), 128);
                        String::from_utf16_lossy(&s[..n as usize])
                    });
                    match Config::from_labels(labels) {
                        Ok(c) => {
                            let Some(owner) = d.owner_state.upgrade().filter(|owner| {
                                GetWindowLongPtrW(d.owner, GWLP_USERDATA)
                                    == std::rc::Rc::as_ptr(owner) as isize
                            }) else {
                                error(hwnd, "截图程序已关闭，请重新打开设置。");
                                return Some(0);
                            };
                            let Ok(mut app) = owner.value.try_borrow_mut() else {
                                error(hwnd, "请先关闭当前对话框后再应用设置。");
                                return Some(0);
                            };
                            match app.hotkeys.apply(d.owner, &c, true) {
                                Ok(()) => {
                                    app.config = c;
                                    DestroyWindow(hwnd);
                                }
                                Err(e) => error(hwnd, &e),
                            }
                        }
                        Err(e) => error(hwnd, &e),
                    }
                }
                2 => {
                    DestroyWindow(hwnd);
                }
                3 => {
                    for (i, k) in Config::default().keys.iter().enumerate() {
                        SetWindowTextW(d.edits[i], wide(&k.label()).as_ptr());
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
unsafe fn set_page(hwnd: HWND, d: &mut Dialog, page: usize) {
    d.page = page.min(2);
    for e in d.edits {
        ShowWindow(e, if d.page == 0 { SW_SHOW } else { SW_HIDE });
    }
    for id in [1, 3] {
        ShowWindow(
            GetDlgItem(hwnd, id),
            if d.page == 0 { SW_SHOW } else { SW_HIDE },
        );
    }
    let scale = |v| dpi::scale(v, dpi::current(hwnd));
    SetWindowPos(
        GetDlgItem(hwnd, 2),
        0,
        scale(if d.page == 0 { 492 } else { 608 }),
        scale(436),
        scale(72),
        scale(32),
        SWP_NOZORDER | SWP_NOACTIVATE,
    );
    for id in 100..=102 {
        InvalidateRect(GetDlgItem(hwnd, id), null(), 0);
    }
    InvalidateRect(hwnd, null(), 0);
}
pub unsafe fn show(owner: HWND, c: &Config) {
    show_page(owner, c, 0);
}
pub unsafe fn show_page(owner: HWND, c: &Config, page: usize) {
    let class = wide("Qingjie.Settings");
    let existing = FindWindowW(class.as_ptr(), null());
    if existing != 0 {
        SendMessageW(existing, WM_APP + 10, page, 0);
        SetForegroundWindow(existing);
        return;
    }
    let mut wc: WNDCLASSW = zeroed();
    wc.lpfnWndProc = Some(settings_proc);
    wc.hInstance = GetModuleHandleW(null());
    wc.lpszClassName = class.as_ptr();
    wc.hCursor = LoadCursorW(0, IDC_ARROW);
    wc.hIcon = ui::app_icon();
    wc.hbrBackground = (COLOR_BTNFACE + 1) as HBRUSH;
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
        1,
        0,
        0,
        5,
        0,
        wide("Segoe UI").as_ptr(),
    );
    let state = window::State::new(Dialog {
        owner,
        owner_state: window::get::<App>(owner)
            .map(|state| std::rc::Rc::downgrade(&state))
            .unwrap_or_default(),
        edits: [0; 4],
        font,
        layout: dpi::Layout::default(),
        page,
    });
    let mut bounds = RECT {
        left: 0,
        top: 0,
        right: 704,
        bottom: 480,
    };
    AdjustWindowRectEx(
        &mut bounds,
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
        0,
        WS_EX_CONTROLPARENT,
    );
    let hwnd = CreateWindowExW(
        WS_EX_CONTROLPARENT,
        class.as_ptr(),
        wide("轻截设置").as_ptr(),
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        bounds.right - bounds.left,
        bounds.bottom - bounds.top,
        0,
        0,
        wc.hInstance,
        window::State::param(&state),
    );
    if hwnd == 0 {
        return;
    }
    SetWindowTextW(hwnd, wide("轻截设置").as_ptr());
    for (i, label) in ["全局快捷键", "编辑快捷键", "使用帮助"].iter().enumerate() {
        control(
            hwnd,
            "BUTTON",
            label,
            WS_TABSTOP | BS_OWNERDRAW as u32,
            8,
            88 + i as i32 * 38,
            124,
            32,
            100 + i as isize,
        );
    }
    for (i, k) in c.keys.iter().enumerate() {
        let e = control(
            hwnd,
            "EDIT",
            &k.label(),
            WS_TABSTOP | ES_AUTOHSCROLL as u32,
            442,
            128 + i as i32 * 52,
            222,
            20,
            10 + i as isize,
        );
        SendMessageW(e, 0x00c5, 80, 0);
        state.value.borrow_mut().edits[i] = e;
    }
    control(
        hwnd,
        "BUTTON",
        "恢复默认",
        WS_TABSTOP | BS_OWNERDRAW as u32,
        164,
        436,
        88,
        32,
        3,
    );
    control(
        hwnd,
        "BUTTON",
        "关闭",
        WS_TABSTOP | BS_OWNERDRAW as u32,
        492,
        436,
        72,
        32,
        2,
    );
    control(
        hwnd,
        "BUTTON",
        "保存并应用",
        WS_TABSTOP | BS_OWNERDRAW as u32,
        576,
        436,
        104,
        32,
        1,
    );
    {
        let mut d = state.value.borrow_mut();
        d.layout.capture(hwnd);
        let Dialog { layout, font, .. } = &mut *d;
        layout.apply(hwnd, font, (704, 480), dpi::current(hwnd), None);
    }
    SendMessageW(hwnd, WM_APP + 10, page, 0);
    ui::set_window_icon(hwnd);
    ShowWindow(hwnd, SW_SHOW);
    SetForegroundWindow(hwnd);
    if page == 0 {
        let first = state.value.borrow().edits[0];
        SetFocus(first);
    }
}
pub unsafe fn dialog_message(msg: &MSG) -> bool {
    let hwnd = FindWindowW(wide("Qingjie.Settings").as_ptr(), null());
    hwnd != 0 && IsDialogMessageW(hwnd, msg) != 0
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_and_roundtrip() {
        for s in ["Ctrl+Shift+A", "Alt+F24", "PrintScreen", "Ctrl+Alt+7"] {
            let k = Hotkey::parse(s).unwrap();
            assert_eq!(Hotkey::parse(&k.label()).unwrap(), k);
        }
        let c = Config::default();
        assert_eq!(Config::decode(&c.encode()).unwrap(), c);
    }
    #[test]
    fn reject_bad_and_duplicate() {
        for s in [
            "A",
            "Shift+A",
            "Ctrl+Ctrl+A",
            "Ctrl+A+B",
            "Win+A",
            "Ctrl+",
            "F25",
        ] {
            assert!(Hotkey::parse(s).is_err(), "{s}");
        }
        assert!(Config::from_labels(std::array::from_fn(|_| "Alt+A".into())).is_err());
    }
}
/// Record a real key chord while a shortcut field has focus; text entry remains available.
pub unsafe fn record(msg: &MSG) -> bool {
    let dialog = FindWindowW(wide("Qingjie.Settings").as_ptr(), null());
    let focus = GetFocus();
    if dialog == 0 || GetParent(focus) != dialog || !(10..=13).contains(&GetDlgCtrlID(focus)) {
        return false;
    }
    if msg.message == WM_KEYDOWN || msg.message == WM_SYSKEYDOWN {
        let key = msg.wParam as u32;
        let modifiers = if GetKeyState(VK_CONTROL as i32) < 0 {
            MOD_CONTROL
        } else {
            0
        } | if GetKeyState(VK_MENU as i32) < 0 {
            MOD_ALT
        } else {
            0
        } | if GetKeyState(VK_SHIFT as i32) < 0 {
            MOD_SHIFT
        } else {
            0
        };
        if (modifiers & (MOD_CONTROL | MOD_ALT) != 0 && (key as u8).is_ascii_alphanumeric())
            || (VK_F1 as u32..=VK_F24 as u32).contains(&key)
            || key == VK_SNAPSHOT as u32
        {
            SetWindowTextW(focus, wide(&Hotkey { modifiers, key }.label()).as_ptr());
            return true;
        }
    }
    false
}
pub unsafe fn record_registered(key: Hotkey) -> bool {
    let dialog = FindWindowW(wide("Qingjie.Settings").as_ptr(), null());
    if dialog == 0 {
        return false;
    }
    let focus = GetFocus();
    if GetForegroundWindow() == dialog
        && GetParent(focus) == dialog
        && IsWindowVisible(focus) != 0
        && (10..=13).contains(&GetDlgCtrlID(focus))
    {
        SetWindowTextW(focus, wide(&key.label()).as_ptr());
        return true;
    }
    false
}
#[cfg(test)]
mod persistence_tests {
    use super::*;
    #[test]
    fn atomic_file_replacement_and_restart_decode() {
        let dir = std::env::current_dir()
            .unwrap()
            .join("target/settings-tests");
        let path = dir.join(format!("{}.ini", std::process::id()));
        let original = Config::default();
        original.save_at(&path).unwrap();
        let mut changed = original.clone();
        changed.keys.swap(0, 1);
        changed.save_at(&path).unwrap();
        assert_eq!(
            Config::decode(&std::fs::read_to_string(&path).unwrap()).unwrap(),
            changed
        );
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn registration_swap_and_conflict_preserves_old_bindings() {
        unsafe {
            // Message-only windows and uncommon test chords avoid the app's normal bindings.
            let class = wide("STATIC");
            let owner = CreateWindowExW(
                0,
                class.as_ptr(),
                wide("").as_ptr(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                0,
                GetModuleHandleW(null()),
                std::ptr::null_mut(),
            );
            let blocker = CreateWindowExW(
                0,
                class.as_ptr(),
                wide("").as_ptr(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                0,
                GetModuleHandleW(null()),
                std::ptr::null_mut(),
            );
            assert_ne!(owner, 0);
            assert_ne!(blocker, 0);
            let cfg = Config::from_labels(
                [
                    "Ctrl+Alt+Shift+F20",
                    "Ctrl+Alt+Shift+F21",
                    "Ctrl+Alt+Shift+F22",
                    "Ctrl+Alt+Shift+F23",
                ]
                .map(str::to_string),
            )
            .unwrap();
            let mut registry = Registry::default();
            registry.apply(owner, &cfg, false).unwrap();
            let mut swapped = cfg.clone();
            swapped.keys.swap(0, 1);
            registry.apply(owner, &swapped, false).unwrap();
            assert_eq!(registry.slots[0].1, swapped.keys[0]);
            let blocked = Hotkey::parse("Ctrl+Alt+Shift+F24").unwrap();
            assert_ne!(
                RegisterHotKey(blocker, 999, blocked.modifiers, blocked.key),
                0
            );
            let old = registry.slots.clone();
            let mut candidate = swapped;
            candidate.keys[2] = blocked;
            assert!(registry.apply(owner, &candidate, false).is_err());
            assert_eq!(registry.slots, old);
            registry.clear(owner);
            UnregisterHotKey(blocker, 999);
            DestroyWindow(owner);
            DestroyWindow(blocker);
        }
    }
}

unsafe fn paint_settings(hwnd: HWND, dc: HDC, page: usize) {
    let ink = ui::rgb(31, 35, 41);
    let muted = ui::rgb(100, 106, 115);
    let secondary = ui::rgb(101, 109, 120);
    let blue = ui::rgb(51, 112, 255);
    fill(
        dc,
        Region {
            x: 0,
            y: 0,
            w: 704,
            h: 480,
        },
        ui::rgb(255, 255, 255),
    );
    fill(
        dc,
        Region {
            x: 0,
            y: 0,
            w: 140,
            h: 480,
        },
        ui::rgb(247, 248, 250),
    );
    fill(
        dc,
        Region {
            x: 139,
            y: 0,
            w: 1,
            h: 480,
        },
        ui::rgb(234, 236, 240),
    );
    ui::rounded(
        dc,
        Region {
            x: 18,
            y: 22,
            w: 28,
            h: 28,
        },
        5,
        blue,
        blue,
    );
    ui::icon(dc, 0, 20, 24, ui::rgb(255, 255, 255));
    ui::text_in(
        dc,
        Region {
            x: 56,
            y: 22,
            w: 64,
            h: 28,
        },
        "轻截",
        17,
        ink,
        false,
        false,
    );
    ui::label(dc, 18, 64, "偏好设置", 12, secondary, false);
    ui::label(
        dc,
        164,
        25,
        ["全局快捷键", "编辑快捷键", "使用帮助"][page.min(2)],
        18,
        ink,
        true,
    );
    ui::label(
        dc,
        164,
        57,
        [
            "在其他应用中，也能快速开始截图。",
            "固定按键，仅在截图编辑时生效。",
            "选择区域、添加标注，然后复制或保存。",
        ][page.min(2)],
        12,
        muted,
        false,
    );
    if page == 0 {
        ui::label(dc, 164, 96, "功能", 12, secondary, false);
        ui::label(dc, 432, 96, "快捷键 · 点击后按组合键", 12, secondary, false);
        let descriptions = [
            "自由选择屏幕区域",
            "截取全部显示器",
            "重复截取上一次选区",
            "等待 3 秒后选择区域",
        ];
        for (i, name) in NAMES.iter().enumerate() {
            let y = 120 + i as i32 * 52;
            ui::label(dc, 164, y + 2, name, 13, ink, false);
            ui::label(dc, 164, y + 23, descriptions[i], 12, secondary, false);
            let field = GetDlgItem(hwnd, 10 + i as i32);
            ui::input_border(
                dc,
                Region {
                    x: 432,
                    y: y + 2,
                    w: 248,
                    h: 32,
                },
                GetFocus() == field,
                ui::is_hot(field),
            );
            if i < 3 {
                fill(
                    dc,
                    Region {
                        x: 164,
                        y: y + 45,
                        w: 516,
                        h: 1,
                    },
                    ui::rgb(242, 243, 245),
                );
            }
        }
        ui::rounded(
            dc,
            Region {
                x: 164,
                y: 342,
                w: 516,
                h: 38,
            },
            4,
            ui::rgb(247, 248, 250),
            ui::rgb(247, 248, 250),
        );
        ui::label(
            dc,
            176,
            354,
            "支持 Ctrl / Alt / Shift 组合；冲突时保留原设置。",
            12,
            muted,
            false,
        );
    } else if page == 1 {
        ui::label(dc, 164, 96, "标注工具", 12, secondary, false);
        for (i, (name, key)) in [
            ("选区", "V"),
            ("画笔", "P"),
            ("矩形", "R"),
            ("椭圆", "O"),
            ("箭头", "A"),
            ("文字", "T"),
            ("马赛克", "M"),
            ("遮挡", "B"),
        ]
        .iter()
        .enumerate()
        {
            let x = 164 + (i % 4) as i32 * 132;
            let y = 120 + (i / 4) as i32 * 38;
            ui::text_in(
                dc,
                Region { x, y, w: 76, h: 24 },
                name,
                13,
                ink,
                false,
                false,
            );
            ui::keycap(dc, x + 112, y, key);
        }
        fill(
            dc,
            Region {
                x: 164,
                y: 202,
                w: 516,
                h: 1,
            },
            ui::BORDER,
        );
        ui::label(dc, 164, 220, "常用操作", 12, secondary, false);
        for (i, (name, key)) in [
            ("复制截图", "Enter / Ctrl+C"),
            ("保存 PNG", "Ctrl+S"),
            ("撤销", "Ctrl+Z"),
            ("重做", "Ctrl+Y"),
            ("置顶贴图", "Ctrl+T"),
            ("自定义颜色", "C"),
        ]
        .iter()
        .enumerate()
        {
            let x = 164 + (i % 2) as i32 * 272;
            let y = 244 + (i / 2) as i32 * 38;
            ui::text_in(
                dc,
                Region {
                    x,
                    y,
                    w: 128,
                    h: 24,
                },
                name,
                13,
                ink,
                false,
                false,
            );
            ui::keycap(dc, x + 244, y, key);
        }
        fill(
            dc,
            Region {
                x: 164,
                y: 358,
                w: 516,
                h: 1,
            },
            ui::BORDER,
        );
        ui::label(
            dc,
            164,
            374,
            "1–8 预设颜色；+ / − 调整线宽；Esc 取消截图。",
            12,
            muted,
            false,
        );
        ui::label(
            dc,
            164,
            395,
            "方向键移动选区，Shift 加速，Ctrl 调整尺寸。",
            12,
            muted,
            false,
        );
    }
    if page == 2 {
        let rows = [
            (0, "开始截图", "点击托盘图标，或使用设置中的全局快捷键。"),
            (2, "选择区域", "拖动框选；选区工具可移动选区或调整边缘。"),
            (
                1,
                "添加标注",
                "使用画笔、形状、箭头或文字；C 可自定义颜色。",
            ),
            (12, "复制与保存", "Enter 复制到剪贴板；Ctrl+S 保存为 PNG。"),
            (
                11,
                "置顶贴图",
                "Ctrl+T 贴在桌面；拖动移动，右键或 Esc 关闭。",
            ),
        ];
        for (i, (icon, title, body)) in rows.iter().enumerate() {
            let y = 98 + i as i32 * 53;
            ui::rounded(
                dc,
                Region {
                    x: 164,
                    y: y + 2,
                    w: 32,
                    h: 32,
                },
                4,
                ui::SURFACE,
                ui::SURFACE,
            );
            ui::icon(dc, *icon, 168, y + 6, ui::BLUE);
            ui::label(dc, 208, y, title, 13, ui::INK, false);
            ui::label(dc, 208, y + 22, body, 12, ui::MUTED, false);
        }
        fill(
            dc,
            Region {
                x: 164,
                y: 374,
                w: 516,
                h: 1,
            },
            ui::BORDER,
        );
        ui::label(
            dc,
            164,
            389,
            "文字输入：Enter 提交，Esc 放弃。图片仅在本地处理。",
            12,
            ui::MUTED,
            false,
        );
    }
    fill(
        dc,
        Region {
            x: 140,
            y: 424,
            w: 564,
            h: 1,
        },
        ui::rgb(234, 236, 240),
    );
}
#[cfg(test)]
mod settings_visual {
    use super::*;
    #[test]
    fn settings_system_close_and_alt_f4_destroy_window() {
        unsafe {
            for page in 0..3 {
                for (message, wp, lp) in [
                    (WM_CLOSE, 0, 0),
                    (WM_SYSCOMMAND, SC_CLOSE as usize, 0),
                    (WM_SYSKEYDOWN, VK_F4 as usize, 1isize << 29),
                ] {
                    show_page(0, &Config::default(), page);
                    let hwnd = FindWindowW(wide("Qingjie.Settings").as_ptr(), null());
                    assert_ne!(hwnd, 0);
                    SendMessageW(hwnd, message, wp, lp);
                    // Alt+F4 posts SC_CLOSE through native default processing.
                    let mut msg: MSG = zeroed();
                    while PeekMessageW(&mut msg, hwnd, 0, 0, PM_REMOVE) != 0 {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                    let still_open = IsWindow(hwnd) != 0;
                    if still_open {
                        DestroyWindow(hwnd);
                    }
                    assert!(!still_open, "设置页 {page} 未响应关闭消息 {message:#x}");
                }
            }
        }
    }
    #[test]
    fn native_settings_render() {
        unsafe {
            show(0, &Config::default());
            let hwnd = FindWindowW(wide("Qingjie.Settings").as_ptr(), null());
            assert_ne!(hwnd, 0);
            assert_ne!(
                GetClassLongPtrW(hwnd, GCLP_HICON),
                0,
                "设置窗口必须有应用图标"
            );
            assert_ne!(
                LoadIconW(GetModuleHandleW(null()), resource_id(1)),
                0,
                "可执行文件必须嵌入应用图标"
            );
            assert_ne!(SendMessageW(hwnd, WM_GETICON, ICON_BIG as usize, 0), 0);
            assert_ne!(SendMessageW(hwnd, WM_GETICON, ICON_SMALL as usize, 0), 0);
            let primary = GetDlgItem(hwnd, 1);
            SendMessageW(primary, WM_MOUSEMOVE, 0, 8 | (8 << 16));
            assert!(ui::is_hot(primary));
            SendMessageW(
                primary,
                windows_sys::Win32::UI::Controls::WM_MOUSELEAVE,
                0,
                0,
            );
            assert!(!ui::is_hot(primary));
            UpdateWindow(hwnd);
            let mut rect: RECT = zeroed();
            GetClientRect(hwnd, &mut rect);
            assert_eq!((rect.right, rect.bottom), (704, 480));
            let b = Bitmap::new(rect.right, rect.bottom).unwrap();
            assert_ne!(
                windows_sys::Win32::Storage::Xps::PrintWindow(hwnd, b.dc, 1),
                0
            );
            super::super::preview::write_preview(&b, "docs/settings-preview.png");
            SendMessageW(hwnd, WM_LBUTTONDOWN, 0, 20 | (140 << 16));
            UpdateWindow(hwnd);
            let state = window::get::<Dialog>(hwnd).unwrap();
            assert_eq!(state.value.borrow().page, 1);
            assert_eq!(IsWindowVisible(state.value.borrow().edits[0]), 0);
            assert_ne!(
                windows_sys::Win32::Storage::Xps::PrintWindow(hwnd, b.dc, 1),
                0
            );
            super::super::preview::write_preview(&b, "docs/editor-shortcuts-preview.png");
            SetFocus(GetDlgItem(hwnd, 2));
            assert!(
                !record_registered(Config::default().keys[0]),
                "不在录制输入框时不得吞掉截图快捷键"
            );
            SendMessageW(hwnd, WM_APP + 10, 2, 0);
            UpdateWindow(hwnd);
            assert_eq!(state.value.borrow().page, 2);
            assert_eq!(IsWindowVisible(GetDlgItem(hwnd, 1)), 0);
            assert_ne!(
                windows_sys::Win32::Storage::Xps::PrintWindow(hwnd, b.dc, 1),
                0
            );
            super::super::preview::write_preview(&b, "docs/help-preview.png");
            DestroyWindow(hwnd);
        }
    }
}
