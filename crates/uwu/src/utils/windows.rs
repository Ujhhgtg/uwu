use std::sync::Arc;

use wgpu::rwh::{HasWindowHandle, RawWindowHandle};
use windows::{
    Win32::{
        Foundation::{COLORREF, HWND},
        System::Com::{
            CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
        },
        UI::WindowsAndMessaging::{
            GWL_EXSTYLE, GetWindowLongPtrW, LWA_ALPHA, SetLayeredWindowAttributes,
            SetWindowLongPtrW, WS_EX_COMPOSITED, WS_EX_LAYERED,
        },
    },
    core::{GUID, HRESULT, IUnknown, IUnknown_Vtbl, Result, interface},
};
use winit::window::Window;

#[interface("37c994e7-432b-4834-a2f7-dce1f13b834b")]
unsafe trait ITipInvocation: IUnknown {
    fn Toggle(&self, hwnd: HWND) -> HRESULT;
}

const CLSID_UIHOST_NO_LAUNCH: GUID = GUID {
    data1: 0x4ce576fa,
    data2: 0x83dc,
    data3: 0x4f88,
    data4: [0x95, 0x1c, 0x9d, 0x07, 0x82, 0xb4, 0xe3, 0x76],
};

struct ComApartment;

impl ComApartment {
    fn init_sta() -> Result<Self> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

pub fn toggle_touch_keyboard(hwnd: Option<HWND>) -> Result<()> {
    let _com = ComApartment::init_sta()?;

    let tip: ITipInvocation =
        unsafe { CoCreateInstance(&CLSID_UIHOST_NO_LAUNCH, None, CLSCTX_ALL)? };

    unsafe {
        tip.Toggle(hwnd.unwrap_or(HWND(std::ptr::null_mut())))
            .ok()?;
    }

    Ok(())
}

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
