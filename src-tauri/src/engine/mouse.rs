use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::rng::SmallRng;
use super::sleep_interruptible;

#[derive(Clone, Copy, Debug)]
pub enum MouseEventSpec {
    LeftDown,
    LeftUp,
    RightDown,
    RightUp,
    MiddleDown,
    MiddleUp,
}

#[cfg(target_os = "windows")]
mod platform {
    use super::MouseEventSpec;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_MOUSE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
        MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
        MOUSEINPUT,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SetCursorPos, SM_CXSCREEN, SM_CYSCREEN,
    };

    pub fn current_cursor_position() -> Option<(i32, i32)> {
        use windows_sys::Win32::Foundation::POINT;
        use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

        let mut point = POINT { x: 0, y: 0 };
        let ok = unsafe { GetCursorPos(&mut point) };
        if ok == 0 {
            None
        } else {
            Some((point.x, point.y))
        }
    }

    pub fn current_screen_size() -> Option<(i32, i32)> {
        let width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
        let height = unsafe { GetSystemMetrics(SM_CYSCREEN) };
        if width <= 0 || height <= 0 {
            return None;
        }

        use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
        let dpi = unsafe { GetDpiForSystem() };
        let scale = dpi as f64 / 96.0;

        Some((
            (width as f64 / scale) as i32,
            (height as f64 / scale) as i32,
        ))
    }

    pub fn move_mouse(x: i32, y: i32) {
        unsafe { SetCursorPos(x, y) };
    }

    fn mouse_event_flag(event: MouseEventSpec) -> u32 {
        match event {
            MouseEventSpec::LeftDown => MOUSEEVENTF_LEFTDOWN,
            MouseEventSpec::LeftUp => MOUSEEVENTF_LEFTUP,
            MouseEventSpec::RightDown => MOUSEEVENTF_RIGHTDOWN,
            MouseEventSpec::RightUp => MOUSEEVENTF_RIGHTUP,
            MouseEventSpec::MiddleDown => MOUSEEVENTF_MIDDLEDOWN,
            MouseEventSpec::MiddleUp => MOUSEEVENTF_MIDDLEUP,
        }
    }

    fn make_input(flags: u32, time: u32) -> INPUT {
        INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: 0,
                    dwFlags: flags,
                    time,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    pub fn send_mouse_event(event: MouseEventSpec, _click_state: i64) {
        let input = make_input(mouse_event_flag(event), 0);
        unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
    }

    pub fn send_batch(down: MouseEventSpec, up: MouseEventSpec, n: usize, _hold_ms: u32) {
        let mut inputs: Vec<INPUT> = Vec::with_capacity(n * 2);
        for _ in 0..n {
            inputs.push(make_input(mouse_event_flag(down), 0));
            inputs.push(make_input(mouse_event_flag(up), 0));
        }
        unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        };
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::MouseEventSpec;
    use core_graphics::display::{CGDisplay, CGPoint};
    use core_graphics::event::{
        CGEvent, CGEventTapLocation, CGEventType, CGMouseButton, EventField,
    };
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    fn event_source() -> Option<CGEventSource> {
        CGEventSource::new(CGEventSourceStateID::CombinedSessionState).ok()
    }

    fn current_location() -> Option<CGPoint> {
        let source = event_source()?;
        let event = CGEvent::new(source).ok()?;
        Some(event.location())
    }

    pub fn current_cursor_position() -> Option<(i32, i32)> {
        let point = current_location()?;
        Some((point.x.round() as i32, point.y.round() as i32))
    }

    pub fn current_screen_size() -> Option<(i32, i32)> {
        let bounds = CGDisplay::main().bounds();
        Some((
            bounds.size.width.round() as i32,
            bounds.size.height.round() as i32,
        ))
    }

    pub fn move_mouse(x: i32, y: i32) {
        let _ = CGDisplay::warp_mouse_cursor_position(CGPoint::new(x as f64, y as f64));
    }

    fn mouse_event_parts(event: MouseEventSpec) -> (CGEventType, CGMouseButton, Option<i64>) {
        match event {
            MouseEventSpec::LeftDown => (CGEventType::LeftMouseDown, CGMouseButton::Left, None),
            MouseEventSpec::LeftUp => (CGEventType::LeftMouseUp, CGMouseButton::Left, None),
            MouseEventSpec::RightDown => (CGEventType::RightMouseDown, CGMouseButton::Right, None),
            MouseEventSpec::RightUp => (CGEventType::RightMouseUp, CGMouseButton::Right, None),
            MouseEventSpec::MiddleDown => {
                (CGEventType::OtherMouseDown, CGMouseButton::Center, Some(2))
            }
            MouseEventSpec::MiddleUp => (CGEventType::OtherMouseUp, CGMouseButton::Center, Some(2)),
        }
    }

    pub fn send_mouse_event(event: MouseEventSpec, click_state: i64) {
        let source = match event_source() {
            Some(source) => source,
            None => return,
        };
        let location = match current_location() {
            Some(location) => location,
            None => return,
        };
        let (event_type, button, button_number) = mouse_event_parts(event);
        let mouse_event = match CGEvent::new_mouse_event(source, event_type, location, button) {
            Ok(event) => event,
            Err(_) => return,
        };

        mouse_event.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, click_state);
        if let Some(button_number) = button_number {
            mouse_event
                .set_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER, button_number);
        }

        mouse_event.post(CGEventTapLocation::HID);
    }

    pub fn send_batch(down: MouseEventSpec, up: MouseEventSpec, n: usize, _hold_ms: u32) {
        for _ in 0..n {
            send_mouse_event(down, 1);
            send_mouse_event(up, 1);
        }
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod platform {
    use super::MouseEventSpec;

    pub fn current_cursor_position() -> Option<(i32, i32)> {
        None
    }

    pub fn current_screen_size() -> Option<(i32, i32)> {
        None
    }

    pub fn move_mouse(_x: i32, _y: i32) {}

    pub fn send_mouse_event(_event: MouseEventSpec, _click_state: i64) {}

    pub fn send_batch(_down: MouseEventSpec, _up: MouseEventSpec, _n: usize, _hold_ms: u32) {}
}

pub fn current_cursor_position() -> Option<(i32, i32)> {
    platform::current_cursor_position()
}

pub fn current_screen_size() -> Option<(i32, i32)> {
    platform::current_screen_size()
}

#[inline]
pub fn get_cursor_pos() -> (i32, i32) {
    current_cursor_position().unwrap_or((0, 0))
}

#[inline]
pub fn move_mouse(x: i32, y: i32) {
    platform::move_mouse(x, y);
}

#[inline]
pub fn send_mouse_event(event: MouseEventSpec, click_state: i64) {
    platform::send_mouse_event(event, click_state);
}

pub fn send_batch(down: MouseEventSpec, up: MouseEventSpec, n: usize, hold_ms: u32) {
    platform::send_batch(down, up, n, hold_ms);
}

pub fn send_clicks(
    down: MouseEventSpec,
    up: MouseEventSpec,
    count: usize,
    hold_ms: u32,
    use_double_click_gap: bool,
    double_click_delay_ms: u32,
    running: &Arc<AtomicBool>,
) {
    if count == 0 {
        return;
    }

    if !use_double_click_gap && count > 1 && hold_ms == 0 {
        send_batch(down, up, count, hold_ms);
        return;
    }

    for index in 0..count {
        if !running.load(Ordering::SeqCst) {
            return;
        }

        let click_state = if use_double_click_gap {
            (index + 1) as i64
        } else {
            1
        };

        send_mouse_event(down, click_state);
        if hold_ms > 0 {
            sleep_interruptible(Duration::from_millis(hold_ms as u64), running);
        }
        send_mouse_event(up, click_state);

        if index + 1 < count && use_double_click_gap && double_click_delay_ms > 0 {
            sleep_interruptible(Duration::from_millis(double_click_delay_ms as u64), running);
        }
    }
}

#[inline]
pub fn get_button_flags(button: i32) -> (MouseEventSpec, MouseEventSpec) {
    match button {
        2 => (MouseEventSpec::RightDown, MouseEventSpec::RightUp),
        3 => (MouseEventSpec::MiddleDown, MouseEventSpec::MiddleUp),
        _ => (MouseEventSpec::LeftDown, MouseEventSpec::LeftUp),
    }
}

#[inline]
pub fn ease_in_out_quad(t: f64) -> f64 {
    if t < 0.5 {
        2.0 * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
    }
}

#[inline]
pub fn cubic_bezier(t: f64, p0: f64, p1: f64, p2: f64, p3: f64) -> f64 {
    let u = 1.0 - t;
    u * u * u * p0 + 3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t * p3
}

pub fn smooth_move(
    start_x: i32,
    start_y: i32,
    end_x: i32,
    end_y: i32,
    duration_ms: u64,
    rng: &mut SmallRng,
) {
    if duration_ms < 5 {
        move_mouse(end_x, end_y);
        return;
    }

    let (sx, sy) = (start_x as f64, start_y as f64);
    let (ex, ey) = (end_x as f64, end_y as f64);
    let (dx, dy) = (ex - sx, ey - sy);
    let distance = (dx * dx + dy * dy).sqrt();
    if distance < 1.0 {
        return;
    }

    let (perp_x, perp_y) = (-dy / distance, dx / distance);
    let sign = |b: bool| if b { 1.0f64 } else { -1.0 };
    let o1 = (rng.next_f64() * 0.3 + 0.15) * distance * sign(rng.next_f64() >= 0.5);
    let o2 = (rng.next_f64() * 0.3 + 0.15) * distance * sign(rng.next_f64() >= 0.5);
    let cp1x = sx + dx * 0.33 + perp_x * o1;
    let cp1y = sy + dy * 0.33 + perp_y * o1;
    let cp2x = sx + dx * 0.66 + perp_x * o2;
    let cp2y = sy + dy * 0.66 + perp_y * o2;

    let steps = (duration_ms as usize).clamp(10, 200);
    let step_dur = Duration::from_millis(duration_ms / steps as u64);

    for i in 0..=steps {
        let t = ease_in_out_quad(i as f64 / steps as f64);
        move_mouse(
            cubic_bezier(t, sx, cp1x, cp2x, ex) as i32,
            cubic_bezier(t, sy, cp1y, cp2y, ey) as i32,
        );
        if i < steps {
            std::thread::sleep(step_dur);
        }
    }
}
