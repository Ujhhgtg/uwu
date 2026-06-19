use std::sync::Arc;
use wgpu::rwh::{HasWindowHandle, RawWindowHandle};
use windows::Win32::{
    Foundation::{COLORREF, HWND},
    UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, LWA_ALPHA, SetLayeredWindowAttributes, SetWindowLongPtrW,
        WS_EX_COMPOSITED, WS_EX_LAYERED,
    },
};
use winit::window::Window;

pub fn winit_window_to_hwnd(window: &Arc<Window>) -> Option<HWND> {
    let handle = window.window_handle();
    if let Ok(handle) = handle
        && let RawWindowHandle::Win32(raw) = handle.as_raw()
    {
        Some(windows::Win32::Foundation::HWND(raw.hwnd.get() as _))
    } else {
        None
    }
}

pub unsafe fn enable_premultiplied_alpha(hwnd: HWND) -> windows::core::Result<()> {
    let ex_style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;

    let new_ex_style = ex_style | WS_EX_LAYERED.0 | WS_EX_COMPOSITED.0;

    unsafe { SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_ex_style as isize) };

    unsafe {
        let _ = SetLayeredWindowAttributes(
            hwnd,
            COLORREF(0), // colorkey (unused)
            255,         // global alpha
            LWA_ALPHA,
        );
    };

    Ok(())
}
