use glib::ffi;

pub fn silence_glib_logs() {
    unsafe extern "C" fn silent_writer(
        _level: ffi::GLogLevelFlags,
        _fields: *const ffi::GLogField,
        _n_fields: usize,
        _user_data: ffi::gpointer,
    ) -> ffi::GLogWriterOutput {
        ffi::G_LOG_WRITER_UNHANDLED
    }

    unsafe {
        ffi::g_log_set_writer_func(Some(silent_writer), std::ptr::null_mut(), None);
    }
}
