use std::sync::atomic::{AtomicBool, Ordering};

use image::{Rgba, RgbaImage};

/// A rectangle in screen pixels. Origin is the top-left of the captured image's coordinate space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Where a captured image sits on screen, and how many image pixels one screen pixel covers.
#[derive(Debug, Clone, Copy)]
pub struct Placement {
    pub origin_x: i32,
    pub origin_y: i32,
    pub screen_w: u32,
    pub screen_h: u32,
    pub scale_x: f32,
    pub scale_y: f32,
}

impl Placement {
    pub fn new(
        origin_x: i32,
        origin_y: i32,
        screen_w: u32,
        screen_h: u32,
        image_w: u32,
        image_h: u32,
    ) -> Self {
        let scale_x = if screen_w == 0 {
            1.0
        } else {
            image_w as f32 / screen_w as f32
        };
        let scale_y = if screen_h == 0 {
            1.0
        } else {
            image_h as f32 / screen_h as f32
        };
        Self {
            origin_x,
            origin_y,
            screen_w: screen_w.max(1),
            screen_h: screen_h.max(1),
            scale_x,
            scale_y,
        }
    }

    pub fn region(self) -> ScreenRect {
        ScreenRect {
            x: self.origin_x,
            y: self.origin_y,
            width: self.screen_w,
            height: self.screen_h,
        }
    }

    /// Image-pixel rectangle for a screen rectangle, padded so field edges are covered.
    pub fn map_rect(
        self,
        field: ScreenRect,
        image_w: u32,
        image_h: u32,
    ) -> Option<(u32, u32, u32, u32)> {
        const PAD: i32 = 4;
        let x0 = ((field.x - self.origin_x) as f32 * self.scale_x).floor() as i32 - PAD;
        let y0 = ((field.y - self.origin_y) as f32 * self.scale_y).floor() as i32 - PAD;
        let x1 = ((field.x + field.width as i32 - self.origin_x) as f32 * self.scale_x).ceil()
            as i32
            + PAD;
        let y1 = ((field.y + field.height as i32 - self.origin_y) as f32 * self.scale_y).ceil()
            as i32
            + PAD;
        let left = x0.max(0) as u32;
        let top = y0.max(0) as u32;
        let right = (x1.max(0) as u32).min(image_w);
        let bottom = (y1.max(0) as u32).min(image_h);
        if right <= left || bottom <= top {
            None
        } else {
            Some((left, top, right - left, bottom - top))
        }
    }
}

/// Password controls that intersect `regions`. Lookup failure leaves the image unchanged.
pub fn fields_in(regions: &[ScreenRect]) -> Vec<ScreenRect> {
    if regions.is_empty() {
        return Vec::new();
    }
    match platform_fields(regions) {
        Ok(fields) => fields,
        Err(err) => {
            static LOGGED: AtomicBool = AtomicBool::new(false);
            if !LOGGED.swap(true, Ordering::Relaxed) {
                eprintln!("clAIre password redaction: {err}");
            }
            Vec::new()
        }
    }
}

pub fn cover(image: &mut RgbaImage, place: Placement, fields: &[ScreenRect]) {
    let width = image.width();
    let height = image.height();
    for field in fields {
        if let Some((x, y, w, h)) = place.map_rect(*field, width, height) {
            pixelate(image, x, y, w, h);
        }
    }
}

fn pixelate(image: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32) {
    const BLOCK: u32 = 14;
    let mut yy = y;
    while yy < y + h {
        let bh = BLOCK.min(y + h - yy);
        let mut xx = x;
        while xx < x + w {
            let bw = BLOCK.min(x + w - xx);
            let color = sample_block(image, xx, yy, bw, bh);
            for dy in 0..bh {
                for dx in 0..bw {
                    image.put_pixel(xx + dx, yy + dy, color);
                }
            }
            xx += BLOCK;
        }
        yy += BLOCK;
    }
}

fn sample_block(image: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> Rgba<u8> {
    let mut sum = [0u32; 3];
    let mut count = 0u32;
    let step_x = (w / 3).max(1);
    let step_y = (h / 3).max(1);
    let mut yy = y;
    while yy < y + h {
        let mut xx = x;
        while xx < x + w {
            let pixel = image.get_pixel(xx, yy).0;
            sum[0] += pixel[0] as u32;
            sum[1] += pixel[1] as u32;
            sum[2] += pixel[2] as u32;
            count += 1;
            xx += step_x;
        }
        yy += step_y;
    }
    if count == 0 {
        return *image.get_pixel(x, y);
    }
    Rgba([
        (sum[0] / count) as u8,
        (sum[1] / count) as u8,
        (sum[2] / count) as u8,
        255,
    ])
}

#[cfg(target_os = "linux")]
fn platform_fields(regions: &[ScreenRect]) -> Result<Vec<ScreenRect>, String> {
    let raw: Vec<(i32, i32, u32, u32)> = regions
        .iter()
        .map(|rect| (rect.x, rect.y, rect.width, rect.height))
        .collect();
    Ok(crate::linux_windows::password_field_rects(&raw)?
        .into_iter()
        .map(|(x, y, width, height)| ScreenRect {
            x,
            y,
            width,
            height,
        })
        .collect())
}

#[cfg(target_os = "macos")]
fn platform_fields(regions: &[ScreenRect]) -> Result<Vec<ScreenRect>, String> {
    macos::password_fields(regions)
}

#[cfg(target_os = "windows")]
fn platform_fields(regions: &[ScreenRect]) -> Result<Vec<ScreenRect>, String> {
    windows::password_fields(regions)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn platform_fields(_regions: &[ScreenRect]) -> Result<Vec<ScreenRect>, String> {
    Ok(Vec::new())
}

#[cfg(target_os = "macos")]
mod macos {
    use std::collections::HashSet;
    use std::ffi::c_void;

    use core_foundation::array::CFArrayRef;
    use core_foundation::base::{CFTypeRef, TCFType};
    use core_foundation::dictionary::CFDictionaryRef;
    use core_foundation::number::CFNumberRef;
    use core_foundation::string::{CFString, CFStringRef};

    use super::ScreenRect;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGSize {
        width: f64,
        height: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGRect {
        origin: CGPoint,
        size: CGSize,
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
        fn AXUIElementCreateApplication(pid: i32) -> CFTypeRef;
        fn AXUIElementCopyAttributeValue(
            element: CFTypeRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> i32;
        fn AXValueGetValue(value: CFTypeRef, value_type: u32, out: *mut c_void) -> u8;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: CFTypeRef);
        fn CFArrayGetCount(array: CFArrayRef) -> isize;
        fn CFArrayGetValueAtIndex(array: CFArrayRef, index: isize) -> *const c_void;
        fn CFArrayGetTypeID() -> usize;
        fn CFDictionaryGetTypeID() -> usize;
        fn CFNumberGetTypeID() -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFGetTypeID(cf: CFTypeRef) -> usize;
        fn CFDictionaryGetValue(dict: CFDictionaryRef, key: CFTypeRef) -> *const c_void;
        fn CFNumberGetValue(number: CFNumberRef, number_type: i32, value: *mut c_void) -> u8;
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        static kCGWindowOwnerPID: CFStringRef;
        static kCGWindowBounds: CFStringRef;
        static kCGWindowLayer: CFStringRef;
        fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: u32) -> CFArrayRef;
        fn CGRectMakeWithDictionaryRepresentation(dict: CFDictionaryRef, rect: *mut CGRect) -> u8;
        fn CGMainDisplayID() -> u32;
        fn CGDisplayBounds(display: u32) -> CGRect;
    }

    const K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY: u32 = 1;
    const K_AX_VALUE_CG_POINT: u32 = 1;
    const K_AX_VALUE_CG_SIZE: u32 = 2;
    const K_CF_NUMBER_SINT32: i32 = 3;

    pub fn password_fields(regions: &[ScreenRect]) -> Result<Vec<ScreenRect>, String> {
        if unsafe { AXIsProcessTrusted() } == 0 {
            return Err("Accessibility permission is off".into());
        }
        let primary_h = unsafe { CGDisplayBounds(CGMainDisplayID()).size.height.round() as i32 };
        let windows = on_screen_windows(regions);
        let mut pids = HashSet::new();
        for window in &windows {
            if window.pid > 0 && window.pid != std::process::id() as i32 {
                pids.insert(window.pid);
            }
        }
        let mut out = Vec::new();
        for pid in pids {
            let app = unsafe { AXUIElementCreateApplication(pid) };
            if app.is_null() {
                continue;
            }
            if let Some(ax_windows) = copy_attr(app, "AXWindows") {
                let count = array_len(ax_windows);
                for index in 0..count {
                    let window = array_item(ax_windows, index);
                    if window.is_null() {
                        continue;
                    }
                    let Some(ax_rect) = element_rect(window) else {
                        continue;
                    };
                    let Some(cg_rect) = windows
                        .iter()
                        .find(|item| {
                            item.pid == pid
                                && (overlaps(item.rect, ax_rect, 24)
                                    || flipped_overlaps(item.rect, ax_rect, primary_h))
                        })
                        .map(|item| item.rect)
                    else {
                        continue;
                    };
                    let axis = if overlaps(cg_rect, ax_rect, 24) {
                        YAxis::TopLeft
                    } else {
                        YAxis::BottomLeft
                    };
                    let mut budget = 400u32;
                    walk(window, 0, &mut budget, axis, primary_h, &mut out);
                }
                unsafe { CFRelease(ax_windows) };
            }
            unsafe { CFRelease(app) };
        }
        out.retain(|rect| {
            rect.width > 0
                && rect.height > 0
                && rect.height < 400
                && regions.iter().any(|region| overlaps(*region, *rect, 8))
        });
        out.sort_by_key(|rect| (rect.x, rect.y, rect.width, rect.height));
        out.dedup_by_key(|rect| (rect.x, rect.y, rect.width, rect.height));
        Ok(out)
    }

    struct OnScreen {
        pid: i32,
        rect: ScreenRect,
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum YAxis {
        TopLeft,
        BottomLeft,
    }

    fn on_screen_windows(regions: &[ScreenRect]) -> Vec<OnScreen> {
        let list = unsafe { CGWindowListCopyWindowInfo(K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY, 0) };
        if list.is_null() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let count = array_len(list);
        for index in 0..count {
            let item = array_item(list, index);
            if item.is_null() || unsafe { CFGetTypeID(item) } != unsafe { CFDictionaryGetTypeID() }
            {
                continue;
            }
            let dict = item as CFDictionaryRef;
            let layer = dict_i32(dict, unsafe { kCGWindowLayer }).unwrap_or(0);
            if layer != 0 {
                continue;
            }
            let Some(pid) = dict_i32(dict, unsafe { kCGWindowOwnerPID }) else {
                continue;
            };
            let bounds = unsafe { CFDictionaryGetValue(dict, kCGWindowBounds as CFTypeRef) };
            if bounds.is_null()
                || unsafe { CFGetTypeID(bounds as CFTypeRef) } != unsafe { CFDictionaryGetTypeID() }
            {
                continue;
            }
            let mut cg = CGRect {
                origin: CGPoint { x: 0.0, y: 0.0 },
                size: CGSize {
                    width: 0.0,
                    height: 0.0,
                },
            };
            if unsafe { CGRectMakeWithDictionaryRepresentation(bounds as CFDictionaryRef, &mut cg) }
                == 0
            {
                continue;
            }
            let rect = ScreenRect {
                x: cg.origin.x.round() as i32,
                y: cg.origin.y.round() as i32,
                width: cg.size.width.round().max(0.0) as u32,
                height: cg.size.height.round().max(0.0) as u32,
            };
            if rect.width < 32 || rect.height < 32 {
                continue;
            }
            if regions.iter().any(|region| overlaps(*region, rect, 16)) {
                out.push(OnScreen { pid, rect });
            }
        }
        unsafe { CFRelease(list) };
        out
    }

    fn walk(
        element: CFTypeRef,
        depth: u32,
        budget: &mut u32,
        axis: YAxis,
        primary_h: i32,
        out: &mut Vec<ScreenRect>,
    ) {
        if *budget == 0 || depth > 14 || element.is_null() {
            return;
        }
        *budget -= 1;
        if let Some(subrole) = copy_string(element, "AXSubrole") {
            if subrole == "AXSecureTextField" {
                if let Some(rect) = element_rect(element) {
                    out.push(normalize(rect, axis, primary_h));
                }
                return;
            }
        }
        let Some(children) = copy_attr(element, "AXChildren") else {
            return;
        };
        let count = array_len(children).min(200);
        for index in 0..count {
            walk(
                array_item(children, index),
                depth + 1,
                budget,
                axis,
                primary_h,
                out,
            );
        }
        unsafe { CFRelease(children) };
    }

    fn normalize(rect: ScreenRect, axis: YAxis, primary_h: i32) -> ScreenRect {
        match axis {
            YAxis::TopLeft => rect,
            YAxis::BottomLeft => ScreenRect {
                y: primary_h - rect.y - rect.height as i32,
                ..rect
            },
        }
    }

    fn element_rect(element: CFTypeRef) -> Option<ScreenRect> {
        let position = copy_attr(element, "AXPosition")?;
        let size = copy_attr(element, "AXSize")?;
        let mut point = CGPoint { x: 0.0, y: 0.0 };
        let mut cg_size = CGSize {
            width: 0.0,
            height: 0.0,
        };
        let point_ok = unsafe {
            AXValueGetValue(
                position,
                K_AX_VALUE_CG_POINT,
                &mut point as *mut _ as *mut c_void,
            )
        };
        let size_ok = unsafe {
            AXValueGetValue(
                size,
                K_AX_VALUE_CG_SIZE,
                &mut cg_size as *mut _ as *mut c_void,
            )
        };
        unsafe {
            CFRelease(position);
            CFRelease(size);
        }
        if point_ok == 0 || size_ok == 0 {
            return None;
        }
        Some(ScreenRect {
            x: point.x.round() as i32,
            y: point.y.round() as i32,
            width: cg_size.width.round().max(0.0) as u32,
            height: cg_size.height.round().max(0.0) as u32,
        })
    }

    fn copy_string(element: CFTypeRef, name: &str) -> Option<String> {
        let value = copy_attr(element, name)?;
        if unsafe { CFGetTypeID(value) } != unsafe { CFStringGetTypeID() } {
            unsafe { CFRelease(value) };
            return None;
        }
        let text = unsafe { CFString::wrap_under_get_rule(value as CFStringRef).to_string() };
        unsafe { CFRelease(value) };
        Some(text)
    }

    fn copy_attr(element: CFTypeRef, name: &str) -> Option<CFTypeRef> {
        let attr = CFString::new(name);
        let mut value: CFTypeRef = std::ptr::null();
        let err = unsafe {
            AXUIElementCopyAttributeValue(element, attr.as_concrete_TypeRef(), &mut value)
        };
        if err != 0 || value.is_null() {
            None
        } else {
            Some(value)
        }
    }

    fn array_len(array: CFTypeRef) -> isize {
        if array.is_null() || unsafe { CFGetTypeID(array) } != unsafe { CFArrayGetTypeID() } {
            return 0;
        }
        unsafe { CFArrayGetCount(array as CFArrayRef) }
    }

    fn array_item(array: CFTypeRef, index: isize) -> CFTypeRef {
        unsafe { CFArrayGetValueAtIndex(array as CFArrayRef, index) as CFTypeRef }
    }

    fn dict_i32(dict: CFDictionaryRef, key: CFStringRef) -> Option<i32> {
        let value = unsafe { CFDictionaryGetValue(dict, key as CFTypeRef) };
        if value.is_null()
            || unsafe { CFGetTypeID(value as CFTypeRef) } != unsafe { CFNumberGetTypeID() }
        {
            return None;
        }
        let mut number = 0i32;
        let ok = unsafe {
            CFNumberGetValue(
                value as CFNumberRef,
                K_CF_NUMBER_SINT32,
                &mut number as *mut _ as *mut c_void,
            )
        };
        if ok == 0 {
            None
        } else {
            Some(number)
        }
    }

    fn overlaps(a: ScreenRect, b: ScreenRect, pad: i32) -> bool {
        a.x - pad < b.x + b.width as i32
            && a.x + a.width as i32 + pad > b.x
            && a.y - pad < b.y + b.height as i32
            && a.y + a.height as i32 + pad > b.y
    }

    fn flipped_overlaps(cg: ScreenRect, ax: ScreenRect, primary_h: i32) -> bool {
        overlaps(cg, normalize(ax, YAxis::BottomLeft, primary_h), 24)
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use std::sync::Mutex;

    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::System::Variant::VARIANT;
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, TreeScope_Descendants, UIA_IsPasswordPropertyId,
    };
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowRect, IsWindowVisible};

    use super::ScreenRect;

    struct Listed {
        hwnd: HWND,
        rect: ScreenRect,
    }

    pub fn password_fields(regions: &[ScreenRect]) -> Result<Vec<ScreenRect>, String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                    .map_err(|err| err.to_string())?;
            let condition = automation
                .CreatePropertyCondition(UIA_IsPasswordPropertyId, &VARIANT::from(true))
                .map_err(|err| err.to_string())?;
            let listed = top_level_windows();
            let mut out = Vec::new();
            for window in listed {
                if !regions
                    .iter()
                    .any(|region| overlaps(*region, window.rect, 16))
                {
                    continue;
                }
                let Ok(element) = automation.ElementFromHandle(window.hwnd) else {
                    continue;
                };
                let Ok(found) = element.FindAll(TreeScope_Descendants, &condition) else {
                    continue;
                };
                let count = found.Length().unwrap_or(0);
                for index in 0..count {
                    let Ok(item) = found.GetElement(index) else {
                        continue;
                    };
                    let Ok(rect) = item.CurrentBoundingRectangle() else {
                        continue;
                    };
                    let width = (rect.right - rect.left).max(0) as u32;
                    let height = (rect.bottom - rect.top).max(0) as u32;
                    if width == 0 || height == 0 || height >= 400 {
                        continue;
                    }
                    let field = ScreenRect {
                        x: rect.left,
                        y: rect.top,
                        width,
                        height,
                    };
                    if regions.iter().any(|region| overlaps(*region, field, 8)) {
                        out.push(field);
                    }
                }
            }
            out.sort_by_key(|rect| (rect.x, rect.y, rect.width, rect.height));
            out.dedup_by_key(|rect| (rect.x, rect.y, rect.width, rect.height));
            Ok(out)
        }
    }

    fn top_level_windows() -> Vec<Listed> {
        let found = Mutex::new(Vec::new());
        unsafe {
            let ptr = &found as *const Mutex<Vec<Listed>> as isize;
            let _ = EnumWindows(Some(enum_windows), LPARAM(ptr));
        }
        found.into_inner().unwrap_or_default()
    }

    unsafe extern "system" fn enum_windows(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let found = &*(lparam.0 as *const Mutex<Vec<Listed>>);
        if IsWindowVisible(hwnd).as_bool() {
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_ok() {
                let width = (rect.right - rect.left).max(0) as u32;
                let height = (rect.bottom - rect.top).max(0) as u32;
                if width >= 32 && height >= 32 {
                    if let Ok(mut guard) = found.lock() {
                        guard.push(Listed {
                            hwnd,
                            rect: ScreenRect {
                                x: rect.left,
                                y: rect.top,
                                width,
                                height,
                            },
                        });
                    }
                }
            }
        }
        true.into()
    }

    fn overlaps(a: ScreenRect, b: ScreenRect, pad: i32) -> bool {
        a.x - pad < b.x + b.width as i32
            && a.x + a.width as i32 + pad > b.x
            && a.y - pad < b.y + b.height as i32
            && a.y + a.height as i32 + pad > b.y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_rect_scales_from_the_screen_origin() {
        let place = Placement::new(100, 50, 200, 100, 400, 200);
        let mapped = place
            .map_rect(
                ScreenRect {
                    x: 110,
                    y: 60,
                    width: 20,
                    height: 10,
                },
                400,
                200,
            )
            .expect("inside");
        assert_eq!(mapped, (16, 16, 48, 28));
    }

    #[test]
    fn cover_pixelates_the_field_and_leaves_the_corner() {
        let mut image = RgbaImage::new(80, 40);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            *pixel = Rgba([x as u8, y as u8, 0, 255]);
        }
        let before = image.get_pixel(2, 2).0;
        let place = Placement::new(0, 0, 80, 40, 80, 40);
        cover(
            &mut image,
            place,
            &[ScreenRect {
                x: 30,
                y: 10,
                width: 20,
                height: 16,
            }],
        );
        assert_eq!(image.get_pixel(2, 2).0, before);
        let covered = image.get_pixel(36, 18).0;
        assert_ne!(covered, [36, 18, 0, 255]);
    }
}
