use windows::{
    Win32::Foundation::{HRESULT, HWND},
    core::{GUID, IUnknown, interface},
};

const DISABLE_TOUCH_SCREEN: GUID = GUID {
    data1: 0x32ce38b2,
    data2: 0x2c9a,
    data3: 0x41b1,
    data4: [0x9b, 0xc5, 0xb3, 0x78, 0x43, 0x94, 0xaa, 0x44],
};

const IID_PROPERTY_STORE: GUID = GUID {
    data1: 0x886d8eeb,
    data2: 0x8cf2,
    data3: 0x4446,
    data4: [0x8d, 0x02, 0xcd, 0xba, 0x1d, 0xbd, 0xcf, 0x99],
};

const VT_BOOL: u16 = 11;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PROPERTYKEY {
    pub fmtid: GUID,
    pub pid: u32,
}

#[repr(C, align(8))]
pub struct PROPVARIANT {
    pub vt: u16,
    pub w_reserved1: u16,
    pub w_reserved2: u16,
    pub w_reserved3: u16,
    pub bool_val: i16,
    pub padding: [u8; 14],
}

#[interface("886d8eeb-8cf2-4446-8d02-cdba1dbdcf99")]
pub unsafe trait IPropertyStore: IUnknown {
    fn GetCount(&self, cprops: *mut u32) -> HRESULT;
    fn GetAt(&self, iprop: u32, pkey: *mut PROPERTYKEY) -> HRESULT;
    fn GetValue(&self, key: *const PROPERTYKEY, pv: *mut PROPVARIANT) -> HRESULT;
    fn SetValue(&self, key: *const PROPERTYKEY, pv: *const PROPVARIANT) -> HRESULT;
    fn Commit(&self) -> HRESULT;
}

extern "system" {
    pub fn SHGetPropertyStoreForWindow(
        hwnd: HWND,
        riid: *const GUID,
        ppv: *mut *mut std::ffi::c_void,
    ) -> HRESULT;
}

#[repr(C)]
struct OSVERSIONINFOEXW {
    dwOSVersionInfoSize: u32,
    dwMajorVersion: u32,
    dwMinorVersion: u32,
    dwBuildNumber: u32,
    dwPlatformId: u32,
    szCSDVersion: [u16; 128],
    wServicePackMajor: u16,
    wServicePackMinor: u16,
    wSuiteMask: u16,
    wProductType: u8,
    wReserved: u8,
}

#[link(name = "ntdll")]
extern "system" {
    fn RtlGetVersion(lpVersionInformation: *mut OSVERSIONINFOEXW) -> i32;
}

pub fn is_windows_10_or_greater() -> bool {
    let mut info = OSVERSIONINFOEXW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOEXW>() as u32,
        dwMajorVersion: 0,
        dwMinorVersion: 0,
        dwBuildNumber: 0,
        dwPlatformId: 0,
        szCSDVersion: [0; 128],
        wServicePackMajor: 0,
        wServicePackMinor: 0,
        wSuiteMask: 0,
        wProductType: 0,
        wReserved: 0,
    };
    unsafe {
        if RtlGetVersion(&mut info) == 0 {
            info.dwMajorVersion >= 10
        } else {
            false
        }
    }
}

pub unsafe fn disable_edge_gestures(hwnd: HWND, disable: bool) -> windows::core::Result<()> {
    let mut prop_store: Option<IPropertyStore> = None;
    let hr = SHGetPropertyStoreForWindow(
        hwnd,
        &IID_PROPERTY_STORE,
        &mut prop_store as *mut Option<IPropertyStore> as *mut _,
    );
    if hr.is_ok() {
        if let Some(prop_store) = prop_store {
            let key = PROPERTYKEY {
                fmtid: DISABLE_TOUCH_SCREEN,
                pid: 2,
            };
            let var = PROPVARIANT {
                vt: VT_BOOL,
                w_reserved1: 0,
                w_reserved2: 0,
                w_reserved3: 0,
                bool_val: if disable { -1 } else { 0 }, // VARIANT_TRUE is -1, VARIANT_FALSE is 0
                padding: [0; 14],
            };
            prop_store.SetValue(&key, &var).ok()?;
        }
    }
    Ok(())
}
