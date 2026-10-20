//! Native pen tablet input (Windows Ink / WM_POINTER).
//!
//! The main window is subclassed so every pen packet is read with
//! `GetPointerPenInfoHistory`: coalesced high-rate samples with their own
//! pressure, tilt, eraser and barrel button state, at sub-pixel (himetric)
//! precision. Messages are then passed on unchanged, so the UI keeps working
//! with the pen as a pointer.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PenPhase {
    Down,
    Move,
    Up,
    Hover,
    Leave,
}

#[derive(Clone, Copy, Debug)]
pub struct PenSample {
    /// Client-area position in physical pixels.
    pub x: f64,
    pub y: f64,
    pub pressure: f32,
    pub tilt_x: i32,
    pub tilt_y: i32,
    pub phase: PenPhase,
    pub eraser: bool,
    pub barrel: bool,
    pub id: u32,
}

#[derive(Default)]
struct Shared {
    queue: Vec<PenSample>,
    pen_ids: HashSet<u32>,
    in_range: bool,
    last_pressure: Option<f32>,
}

static SHARED: OnceLock<Mutex<Shared>> = OnceLock::new();
static CTX: OnceLock<egui::Context> = OnceLock::new();

fn shared() -> &'static Mutex<Shared> {
    SHARED.get_or_init(Default::default)
}

/// Takes all pen samples received since the last call.
pub fn drain() -> Vec<PenSample> {
    std::mem::take(&mut shared().lock().unwrap().queue)
}

/// Is this egui touch id actually the pen?
pub fn is_pen_id(id: u64) -> bool {
    shared().lock().unwrap().pen_ids.contains(&(id as u32))
}

/// Pressure of the pen while it touches the surface (for the settings test area).
pub fn peek_last_pressure() -> Option<f32> {
    shared().lock().unwrap().last_pressure
}

pub fn pen_in_range() -> bool {
    shared().lock().unwrap().in_range
}

#[cfg(windows)]
pub fn install(hwnd: isize, ctx: egui::Context) {
    use windows_sys::Win32::UI::Shell::SetWindowSubclass;
    use windows_sys::Win32::UI::WindowsAndMessaging::SetPropW;
    let _ = CTX.set(ctx);
    let hwnd = hwnd as windows_sys::Win32::Foundation::HWND;
    unsafe {
        // Turn off press-and-hold right click, tap feedback and flicks: they
        // delay and distort ink.
        let name: Vec<u16> = "MicrosoftTabletPenServiceProperty\0".encode_utf16().collect();
        let flags: usize = 0x0000_0001 | 0x0000_0008 | 0x0000_0010 | 0x0001_0000;
        SetPropW(hwnd, name.as_ptr(), flags as _);
        SetWindowSubclass(hwnd, Some(subclass_proc), 0x4e4e, 0);
    }
}

#[cfg(not(windows))]
pub fn install(_hwnd: isize, _ctx: egui::Context) {}

#[cfg(windows)]
unsafe extern "system" fn subclass_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
    _id: usize,
    _data: usize,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::Shell::DefSubclassProc;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    if matches!(msg, WM_POINTERDOWN | WM_POINTERUPDATE | WM_POINTERUP | WM_POINTERLEAVE) {
        unsafe { handle_pointer(hwnd, msg, wparam) };
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

#[cfg(windows)]
unsafe fn handle_pointer(hwnd: windows_sys::Win32::Foundation::HWND, msg: u32, wparam: usize) {
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
    use windows_sys::Win32::UI::Input::Pointer::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    let pointer_id = (wparam & 0xFFFF) as u32;
    let mut ptype: POINTER_INPUT_TYPE = 0;
    if unsafe { GetPointerType(pointer_id, &mut ptype) } == 0 || ptype != PT_PEN {
        return;
    }
    let mut sh = shared().lock().unwrap();
    sh.pen_ids.insert(pointer_id);
    if msg == WM_POINTERLEAVE {
        sh.in_range = false;
        sh.last_pressure = None;
        sh.queue.push(PenSample {
            x: 0.0,
            y: 0.0,
            pressure: 0.0,
            tilt_x: 0,
            tilt_y: 0,
            phase: PenPhase::Leave,
            eraser: false,
            barrel: false,
            id: pointer_id,
        });
        drop(sh);
        if let Some(c) = CTX.get() {
            c.request_repaint();
        }
        return;
    }
    let mut count: u32 = 0;
    if unsafe { GetPointerPenInfoHistory(pointer_id, &mut count, std::ptr::null_mut()) } == 0 || count == 0 {
        return;
    }
    let mut infos: Vec<POINTER_PEN_INFO> = vec![Default::default(); count as usize];
    if unsafe { GetPointerPenInfoHistory(pointer_id, &mut count, infos.as_mut_ptr()) } == 0 {
        return;
    }
    infos.truncate(count as usize);
    // History is newest-first.
    for info in infos.iter().rev() {
        let pi = &info.pointerInfo;
        let mut dev: RECT = unsafe { std::mem::zeroed() };
        let mut disp: RECT = unsafe { std::mem::zeroed() };
        let (sx, sy) = if unsafe { GetPointerDeviceRects(pi.sourceDevice, &mut dev, &mut disp) } != 0
            && dev.right > dev.left
            && dev.bottom > dev.top
        {
            let kx = (disp.right - disp.left) as f64 / (dev.right - dev.left) as f64;
            let ky = (disp.bottom - disp.top) as f64 / (dev.bottom - dev.top) as f64;
            (
                disp.left as f64 + pi.ptHimetricLocation.x as f64 * kx,
                disp.top as f64 + pi.ptHimetricLocation.y as f64 * ky,
            )
        } else {
            (pi.ptPixelLocation.x as f64, pi.ptPixelLocation.y as f64)
        };
        let mut pt = POINT { x: sx.floor() as i32, y: sy.floor() as i32 };
        unsafe { ScreenToClient(hwnd, &mut pt) };
        let x = pt.x as f64 + sx.fract();
        let y = pt.y as f64 + sy.fract();
        let flags = pi.pointerFlags;
        let phase = if flags & POINTER_FLAG_DOWN != 0 {
            PenPhase::Down
        } else if flags & POINTER_FLAG_UP != 0 || flags & POINTER_FLAG_CANCELED != 0 {
            PenPhase::Up
        } else if flags & POINTER_FLAG_INCONTACT != 0 {
            PenPhase::Move
        } else {
            PenPhase::Hover
        };
        let pressure = if info.penMask & PEN_MASK_PRESSURE != 0 {
            (info.pressure as f32 / 1024.0).clamp(0.0, 1.0)
        } else if phase == PenPhase::Hover {
            0.0
        } else {
            0.6
        };
        sh.in_range = phase != PenPhase::Up || flags & POINTER_FLAG_INRANGE != 0;
        sh.last_pressure = matches!(phase, PenPhase::Down | PenPhase::Move).then_some(pressure);
        sh.queue.push(PenSample {
            x,
            y,
            pressure,
            tilt_x: if info.penMask & PEN_MASK_TILT_X != 0 { info.tiltX } else { 0 },
            tilt_y: if info.penMask & PEN_MASK_TILT_Y != 0 { info.tiltY } else { 0 },
            phase,
            eraser: info.penFlags & (PEN_FLAG_ERASER | PEN_FLAG_INVERTED) != 0,
            barrel: info.penFlags & PEN_FLAG_BARREL != 0,
            id: pointer_id,
        });
    }
    // Keep memory bounded if the UI is not draining (e.g. minimized).
    if sh.queue.len() > 20000 {
        let n = sh.queue.len() - 20000;
        sh.queue.drain(..n);
    }
    drop(sh);
    if let Some(c) = CTX.get() {
        c.request_repaint();
    }
}
