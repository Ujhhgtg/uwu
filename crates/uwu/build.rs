fn main() {
    // --- 1. executable icon (windows only) ---
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("./assets/images/app_icon/icon.ico");
        res.compile().unwrap();
    }
}
