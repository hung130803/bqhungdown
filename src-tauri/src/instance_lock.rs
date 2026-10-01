//! Hold an OS lock for the entire app lifetime, before restoring/cleaning jobs.
#[cfg(windows)]
pub fn acquire() -> std::io::Result<std::fs::File> {
    let root = dirs::config_dir().ok_or_else(||
        std::io::Error::new(std::io::ErrorKind::NotFound, "No config directory"))?
        .join("com.prodown.app");
    std::fs::create_dir_all(&root)?;
    acquire_at(&root.join("instance.lock"))
}

#[cfg(windows)]
fn acquire_at(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().read(true).write(true).create(true)
        .truncate(false).share_mode(0).open(path)
}

#[cfg(windows)]
pub fn explain_failure(error: &std::io::Error) {
    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(window: *mut std::ffi::c_void, text: *const u16,
                       caption: *const u16, kind: u32) -> i32;
    }
    let message = if error.raw_os_error() == Some(32) {
        "BQHungDown đang chạy. Hãy mở cửa sổ hiện có từ khay hệ thống cạnh đồng hồ."
            .to_string()
    } else {
        format!("Không thể khóa dữ liệu BQHungDown để mở ứng dụng an toàn: {error}")
    };
    let text: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    let title: Vec<u16> = "BQHungDown".encode_utf16().chain(Some(0)).collect();
    // Both strings are NUL-terminated and remain alive for this blocking call.
    unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), 0x40); }
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn exclusive_until_guard_is_dropped() {
        let path = std::env::temp_dir().join(format!("bqd-lock-test-{}.lock", std::process::id()));
        let first = super::acquire_at(&path).unwrap();
        assert_eq!(super::acquire_at(&path).unwrap_err().raw_os_error(), Some(32));
        drop(first);
        let second = super::acquire_at(&path).unwrap();
        drop(second);
        std::fs::remove_file(path).unwrap();
    }
}
