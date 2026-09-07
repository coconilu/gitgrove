// Keep the native caption, controls and drag behavior; only customize its colors.
use windows_sys::Win32::{
    Foundation::HWND,
    Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR},
};

const fn colorref(red: u8, green: u8, blue: u8) -> u32 {
    red as u32 | ((green as u32) << 8) | ((blue as u32) << 16)
}

// Match styles.css --bg and --text. COLORREF stores bytes as 0x00BBGGRR.
const CAPTION_COLOR: u32 = colorref(0x0d, 0x11, 0x17);
const TEXT_COLOR: u32 = colorref(0xe6, 0xed, 0xf3);

pub fn apply(window: &tauri::WebviewWindow) {
    if let Ok(hwnd) = window.hwnd() {
        // These attributes require Windows 11 build 22000+. Failure is cosmetic:
        // retain the configured native Dark theme and continue app startup.
        let _ = set_caption_colors(hwnd.0 as HWND);
    }
}

fn set_caption_colors(hwnd: HWND) -> [i32; 2] {
    [
        (DWMWA_CAPTION_COLOR, CAPTION_COLOR),
        (DWMWA_TEXT_COLOR, TEXT_COLOR),
    ]
    .map(|(attribute, value)| {
        // DWM reads a live four-byte COLORREF synchronously. No ownership of
        // the window or pointer is transferred to this call.
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                attribute as u32,
                (&value as *const u32).cast(),
                std::mem::size_of::<u32>() as u32,
            )
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caption_colors_use_native_bgr_order_and_four_bytes() {
        assert_eq!(CAPTION_COLOR, 0x0017_110d);
        assert_eq!(TEXT_COLOR, 0x00f3_ede6);
        assert_eq!(std::mem::size_of_val(&CAPTION_COLOR), 4);
    }

    #[test]
    fn invalid_window_returns_nonfatal_failures_for_both_attributes() {
        // Exercise the real DWM failure path without creating/touching a window.
        let results = set_caption_colors(std::ptr::null_mut());
        assert!(results.into_iter().all(|hresult| hresult < 0));
    }
}
