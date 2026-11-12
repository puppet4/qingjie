//! Window-owned state stays alive until the outermost callback returns.
//! Reentrant callbacks never acquire a second mutable borrow. Destruction is
//! delivered after the current callback; borrowed Win32 message pointers are
//! never queued. Native default processing remains available during modal UI.
use super::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

pub struct State<T> {
    pub value: RefCell<T>,
    destroy_pending: Cell<bool>,
    repaint_pending: Cell<bool>,
}
impl<T> State<T> {
    pub fn new(value: T) -> Rc<Self> {
        Rc::new(Self {
            value: RefCell::new(value),
            destroy_pending: Cell::new(false),
            repaint_pending: Cell::new(false),
        })
    }
    pub fn param(state: &Rc<Self>) -> *const std::ffi::c_void {
        Rc::as_ptr(state).cast()
    }
}
pub unsafe fn get<T>(hwnd: HWND) -> Option<Rc<State<T>>> {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State<T>;
    if ptr.is_null() {
        return None;
    }
    Rc::increment_strong_count(ptr);
    Some(Rc::from_raw(ptr))
}
pub unsafe fn dispatch<T>(
    hwnd: HWND,
    msg: u32,
    wp: usize,
    lp: isize,
    handle: unsafe fn(HWND, u32, usize, isize, &mut T) -> Option<isize>,
) -> isize {
    if msg == WM_NCCREATE {
        let cs = &*(lp as *const CREATESTRUCTW);
        let ptr = cs.lpCreateParams as *const State<T>;
        if ptr.is_null() {
            return 0;
        }
        Rc::increment_strong_count(ptr);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr as isize);
        return 1;
    }
    let Some(state) = get::<T>(hwnd) else {
        return DefWindowProcW(hwnd, msg, wp, lp);
    };
    if msg == WM_NCDESTROY {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        drop(Rc::from_raw(Rc::as_ptr(&state)));
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    let Ok(mut value) = state.value.try_borrow_mut() else {
        match msg {
            WM_DESTROY => {
                state.destroy_pending.set(true);
                return 0;
            }
            WM_PAINT => {
                ValidateRect(hwnd, null());
                state.repaint_pending.set(true);
                return 0;
            }
            // EN_CHANGE from programmatic synchronization is intentionally ignored.
            WM_COMMAND | WM_HOTKEY | WM_TIMER | WM_CLOSE | TRAY | WM_APP_SETTINGS => return 0,
            _ => return DefWindowProcW(hwnd, msg, wp, lp),
        }
    };
    let result = handle(hwnd, msg, wp, lp, &mut value);
    if state.destroy_pending.replace(false) {
        handle(hwnd, WM_DESTROY, 0, 0, &mut value);
    }
    drop(value);
    if state.repaint_pending.replace(false) && IsWindow(hwnd) != 0 {
        InvalidateRect(hwnd, null(), 0);
    }
    // Default processing can synchronously send WM_CLOSE (SC_CLOSE / Alt+F4),
    // focus and other messages. Release the state borrow before entering it.
    result.unwrap_or_else(|| DefWindowProcW(hwnd, msg, wp, lp))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Probe {
        drops: Rc<Cell<u32>>,
        destroys: u32,
        outer_completed: bool,
    }
    impl Drop for Probe {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
        }
    }
    unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: usize, lp: isize) -> isize {
        dispatch(hwnd, msg, wp, lp, handle)
    }
    unsafe fn handle(hwnd: HWND, msg: u32, _wp: usize, _lp: isize, p: &mut Probe) -> Option<isize> {
        Some(match msg {
            WM_COMMAND => {
                // The nested command is ignored while borrowed; destruction is deferred.
                SendMessageW(hwnd, WM_COMMAND, 0, 0);
                DestroyWindow(hwnd);
                assert_eq!(p.drops.get(), 0);
                p.outer_completed = true;
                0
            }
            WM_DESTROY => {
                p.destroys += 1;
                0
            }
            _ => return None,
        })
    }
    #[test]
    fn nested_callback_and_destroy_keep_state_alive() {
        unsafe {
            let name = wide("Qingjie.Test.Lifetime");
            let mut wc: WNDCLASSW = zeroed();
            wc.lpfnWndProc = Some(proc);
            wc.lpszClassName = name.as_ptr();
            wc.hInstance = GetModuleHandleW(null());
            RegisterClassW(&wc);
            let drops = Rc::new(Cell::new(0));
            let state = State::new(Probe {
                drops: drops.clone(),
                destroys: 0,
                outer_completed: false,
            });
            let hwnd = CreateWindowExW(
                0,
                name.as_ptr(),
                name.as_ptr(),
                WS_POPUP,
                0,
                0,
                10,
                10,
                0,
                0,
                wc.hInstance,
                State::param(&state),
            );
            assert_ne!(hwnd, 0);
            SendMessageW(hwnd, WM_COMMAND, 0, 0);
            assert!(state.value.borrow().outer_completed);
            assert_eq!(state.value.borrow().destroys, 1);
            assert_eq!(Rc::strong_count(&state), 1);
            drop(state);
            assert_eq!(drops.get(), 1);
        }
    }
}
