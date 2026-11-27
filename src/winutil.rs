//! Windows 平台工具：资源管理器定位、默认程序打开、回收站删除。

use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};

/// 去掉 `canonicalize()` 产生的 `\\?\` 长路径前缀，便于 Explorer 和 Shell API 识别。
pub fn clean_path(path: &Path) -> PathBuf {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let s = abs.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        abs
    }
}

#[allow(dead_code)]
fn to_wide_nul(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 在 Windows 资源管理器中打开并选中指定文件或目录。
pub fn reveal_in_explorer(path: &Path) {
    let clean = clean_path(path);
    let _ = std::process::Command::new("explorer.exe")
        .raw_arg(format!("/select,\"{}\"", clean.display()))
        .spawn();
}

/// 使用系统默认程序打开文件、目录或 URL。
#[allow(dead_code)]
pub fn open_default(target: &str) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let clean = if Path::new(target).exists() {
        clean_path(Path::new(target)).to_string_lossy().into_owned()
    } else {
        target.to_string()
    };

    let op = to_wide_nul("open");
    let file = to_wide_nul(&clean);
    let ret = unsafe { ShellExecuteW(std::ptr::null_mut(), op.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL) };
    if (ret as isize) <= 32 {
        Err(format!("无法打开：{clean}"))
    } else {
        Ok(())
    }
}

/// 将文件或目录移动到 Windows 回收站（需要调用方先做二次确认）。
#[allow(dead_code)]
pub fn move_to_trash(path: &Path) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::{FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE, SHFILEOPSTRUCTW, SHFileOperationW};

    let clean = clean_path(path);
    let mut wide: Vec<u16> = clean.as_os_str().encode_wide().collect();
    wide.push(0);
    wide.push(0); // SHFileOperationW 要求双 NUL 结尾

    let mut op = SHFILEOPSTRUCTW {
        hwnd: std::ptr::null_mut(),
        wFunc: FO_DELETE,
        pFrom: wide.as_ptr(),
        pTo: std::ptr::null(),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT) as u16,
        fAnyOperationsAborted: 0,
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: std::ptr::null(),
    };
    let rc = unsafe { SHFileOperationW(&mut op) };
    if rc == 0 && op.fAnyOperationsAborted == 0 {
        Ok(())
    } else {
        Err(format!("移入回收站失败（错误码 {rc}）：{}", clean.display()))
    }
}

/// 在 Windows 11 上通过 DWM 属性恢复无边框窗口圆角与系统阴影；在 Windows 10 或不支持环境返回 `false`。
pub fn try_enable_rounded_corners(wh: &impl raw_window_handle::HasWindowHandle) -> bool {
    use raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};

    let Ok(handle) = wh.window_handle() else {
        return false;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return false;
    };
    let hwnd = win32.hwnd.get() as HWND;
    let pref = DWMWCP_ROUND;
    let hr = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            (&raw const pref).cast(),
            core::mem::size_of_val(&pref) as u32,
        )
    };
    hr == 0
}
