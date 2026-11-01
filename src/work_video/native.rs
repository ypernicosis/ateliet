//! Owns the Objective-C controller on the egui / AppKit main thread.
use serde::Deserialize;

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    pub position: f64,
    pub playing: bool,
    pub visible: bool,
    pub pinned: bool,
    pub title: String,
    pub id: String,
    pub error: String,
    pub presentation: String,
}

#[cfg(target_os = "macos")]
mod platform {
    use super::Snapshot;
    use std::ffi::{c_char, c_void, CStr, CString};
    use std::marker::PhantomData;
    use std::rc::Rc;

    extern "C" {
        fn ateliet_video_create(html: *const c_char) -> *mut c_void;
        fn ateliet_video_open(handle: *mut c_void, json: *const c_char);
        fn ateliet_video_toggle(handle: *mut c_void);
        fn ateliet_video_pause(handle: *mut c_void);
        fn ateliet_video_play(handle: *mut c_void);
        fn ateliet_video_place(handle: *mut c_void, view: *mut c_void, sw: f64, sh: f64,
            x: f64, y: f64, w: f64, h: f64, mode: i32);
        fn ateliet_video_has_event(handle: *mut c_void) -> bool;
        fn ateliet_video_snapshot(handle: *mut c_void) -> *mut c_char;
        fn ateliet_video_free_string(value: *mut c_char);
        fn ateliet_video_destroy(handle: *mut c_void);
    }

    pub struct Backend {
        handle: std::ptr::NonNull<c_void>,
        // AppKit objects are !Send and !Sync, including their destructor.
        _main_thread: PhantomData<Rc<()>>,
    }

    impl Backend {
        pub fn new() -> Result<Self, String> {
            let html = CString::new(include_str!("../../native/work_video.html")).map_err(|e| e.to_string())?;
            // SAFETY: constructed only from App::update on AppKit's main thread;
            // create returns one retained controller owned by this wrapper.
            let handle = std::ptr::NonNull::new(unsafe { ateliet_video_create(html.as_ptr()) }).ok_or("Não foi possível criar o player.")?;
            Ok(Self { handle, _main_thread: PhantomData })
        }
        pub fn open(&mut self, json: &str) -> Result<(), String> {
            let json = CString::new(json).map_err(|e| e.to_string())?;
            // SAFETY: live controller; native code copies the UTF-8 string.
            unsafe { ateliet_video_open(self.handle.as_ptr(), json.as_ptr()) };
            Ok(())
        }
        pub fn has_event(&self) -> bool { unsafe { ateliet_video_has_event(self.handle.as_ptr()) } }
        pub fn snapshot(&self) -> Snapshot {
            // SAFETY: returned buffer is either null or a malloc-owned UTF-8
            // JSON string; always released by its matching allocator.
            unsafe {
                let value = ateliet_video_snapshot(self.handle.as_ptr());
                if value.is_null() { return Snapshot::default(); }
                let result = serde_json::from_slice(CStr::from_ptr(value).to_bytes()).unwrap_or_default();
                ateliet_video_free_string(value);
                result
            }
        }
        pub fn toggle(&mut self) { unsafe { ateliet_video_toggle(self.handle.as_ptr()) }; }
        pub fn pause(&mut self) { unsafe { ateliet_video_pause(self.handle.as_ptr()) }; }
        pub fn play(&mut self) { unsafe { ateliet_video_play(self.handle.as_ptr()) }; }
        pub fn place(&mut self, frame: &eframe::Frame, screen: eframe::egui::Rect, rect: Option<eframe::egui::Rect>, mode: i32) {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            let Ok(window) = frame.window_handle() else { self.pause(); return };
            let RawWindowHandle::AppKit(handle) = window.as_raw() else { self.pause(); return };
            let r = rect.unwrap_or(eframe::egui::Rect::from_min_size(screen.min, eframe::egui::Vec2::ZERO));
            // SAFETY: eframe lends the live AppKit view on the main thread for
            // this call. Native code keeps a weak reference, never owns the host.
            unsafe { ateliet_video_place(self.handle.as_ptr(), handle.ns_view.as_ptr(),
                screen.width() as f64, screen.height() as f64,
                (r.min.x - screen.min.x) as f64, (r.min.y - screen.min.y) as f64,
                r.width() as f64, r.height() as f64, mode) };
        }
    }
    impl Drop for Backend {
        fn drop(&mut self) {
            // SAFETY: consumes exactly the retained reference from create.
            unsafe { ateliet_video_destroy(self.handle.as_ptr()) };
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::Snapshot;
    pub struct Backend;
    impl Backend {
        pub fn new() -> Result<Self, String> { Err("O player integrado está disponível no macOS. Use Abrir fora neste sistema.".into()) }
        pub fn open(&mut self, _: &str) -> Result<(), String> { Ok(()) }
        pub fn has_event(&self) -> bool { false }
        pub fn snapshot(&self) -> Snapshot { Snapshot::default() }
        pub fn toggle(&mut self) {}
        pub fn pause(&mut self) {}
        pub fn play(&mut self) {}
        pub fn place(&mut self, _: &eframe::Frame, _: eframe::egui::Rect, _: Option<eframe::egui::Rect>, _: i32) {}
    }
}
pub use platform::Backend;
