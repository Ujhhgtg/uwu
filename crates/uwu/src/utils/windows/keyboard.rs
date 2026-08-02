use windows::{
    Win32::{
        Foundation::HWND,
        System::Com::{
            CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
        },
    },
    core::{GUID, HRESULT, IUnknown, IUnknown_Vtbl, Result, interface},
};

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
