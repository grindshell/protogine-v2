//! The engine side of `pg.system`: the platform's name, the clipboard, power, and opening URLs.
//! Each platform's code is in a module of its own. It has no Lua dependency.

#[cfg(not(target_arch = "wasm32"))]
use macroquad::miniquad::window::{clipboard_get, clipboard_set};

#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(not(any(
    windows,
    target_os = "macos",
    target_os = "linux",
    target_arch = "wasm32"
)))]
use other as platform;
#[cfg(target_arch = "wasm32")]
use web as platform;
#[cfg(windows)]
use windows as platform;

/// The platform's name, as Love2D's `getOS` gives it.
pub fn os() -> &'static str {
    if cfg!(target_arch = "wasm32") {
        "Web"
    } else if cfg!(windows) {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "OS X"
    } else if cfg!(target_os = "ios") {
        "iOS"
    } else if cfg!(target_os = "android") {
        "Android"
    } else if cfg!(unix) {
        // Love2D calls the BSDs Linux too.
        "Linux"
    } else {
        "Unknown"
    }
}

/// The number of logical processors.
#[cfg(not(target_arch = "wasm32"))]
pub fn processor_count() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
}

/// The number of logical processors, as the browser reports it.
#[cfg(target_arch = "wasm32")]
pub fn processor_count() -> usize {
    (web::processor_count() as usize).max(1)
}

/// The clipboard's text, or `""` if it holds none.
#[cfg(not(target_arch = "wasm32"))]
pub fn clipboard_text() -> String {
    clipboard_get().unwrap_or_default()
}

/// Browsers only let a page read the clipboard when the player pastes, so this is the text the
/// player last pasted into the page, or the text last set.
#[cfg(target_arch = "wasm32")]
pub fn clipboard_text() -> String {
    web::clipboard_text()
}

/// Puts text on the clipboard. Like Love2D, it stops at a NUL character.
pub fn set_clipboard_text(text: &str) {
    let text = text.split('\0').next().unwrap_or_default();
    #[cfg(not(target_arch = "wasm32"))]
    clipboard_set(text);
    #[cfg(target_arch = "wasm32")]
    web::set_clipboard_text(text);
}

/// Opens an http, https or mailto URL in the player's browser or mail app, and returns whether
/// it did. Other schemes aren't opened, since some, like file URLs, can run programs.
pub fn open_url(url: &str) -> bool {
    openable(url) && platform::open_url(url)
}

fn openable(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once(':') else {
        return false;
    };
    if url.chars().any(char::is_control) {
        return false;
    }
    match scheme.to_ascii_lowercase().as_str() {
        "http" | "https" => rest.starts_with("//") && rest.len() > 2,
        "mailto" => true,
        _ => false,
    }
}

/// Vibrates the device. Only browsers that support it can (mostly on Android).
pub fn vibrate(seconds: f64) {
    #[cfg(target_arch = "wasm32")]
    web::vibrate(seconds * 1000.0);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = seconds;
}

/// The battery's state, from the platform. Every platform's reading follows SDL's, which
/// Love2D uses.
pub fn power_info() -> PowerInfo {
    platform::power_info()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerState {
    Unknown,
    /// Running on battery.
    Battery,
    NoBattery,
    Charging,
    /// Plugged in and fully charged.
    Charged,
}

impl PowerState {
    /// Love2D's name for the state.
    pub fn name(self) -> &'static str {
        match self {
            PowerState::Unknown => "unknown",
            PowerState::Battery => "battery",
            PowerState::NoBattery => "nobattery",
            PowerState::Charging => "charging",
            PowerState::Charged => "charged",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PowerInfo {
    pub state: PowerState,
    /// The battery's charge, from 0 to 100.
    pub percent: Option<u8>,
    /// How long the battery will last.
    pub seconds: Option<u32>,
}

impl PowerInfo {
    const UNKNOWN: PowerInfo = PowerInfo::only(PowerState::Unknown);

    const fn only(state: PowerState) -> PowerInfo {
        PowerInfo {
            state,
            percent: None,
            seconds: None,
        }
    }
}

/// Of several batteries, the one with the most time left, or failing that, the most charge.
#[cfg(any(target_os = "linux", target_os = "macos", test))]
fn best_battery(batteries: impl IntoIterator<Item = PowerInfo>) -> Option<PowerInfo> {
    batteries.into_iter().reduce(|best, battery| {
        let better = if battery.seconds.is_none() && best.seconds.is_none() {
            battery.percent > best.percent
        } else {
            battery.seconds > best.seconds
        };
        if better { battery } else { best }
    })
}

/// Power from the browser's Battery Status API. Browsers report a computer with no battery as
/// full and charging, so, as in SDL, that reads as no battery.
#[cfg(any(target_arch = "wasm32", test))]
fn battery_status(
    charging: bool,
    level: f64,
    charging_time: f64,
    discharging_time: f64,
) -> PowerInfo {
    if charging && level == 1.0 && charging_time == 0.0 {
        return PowerInfo::only(PowerState::NoBattery);
    }
    let state = if !charging {
        PowerState::Battery
    } else if charging_time == 0.0 {
        PowerState::Charged
    } else {
        PowerState::Charging
    };
    PowerInfo {
        state,
        percent: Some((level * 100.0).round().clamp(0.0, 100.0) as u8),
        // Infinite while charging.
        seconds: discharging_time
            .is_finite()
            .then_some(discharging_time.max(0.0) as u32),
    }
}

/// Opens `url` with `program`, the desktop's URL opener on Linux and macOS.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn spawn_opener(program: &str, url: &str) -> bool {
    match std::process::Command::new(program).arg(url).spawn() {
        Ok(mut child) => {
            // Reap it without waiting on it: xdg-open can run as long as the browser does.
            std::thread::spawn(move || child.wait());
            true
        }
        Err(_) => false,
    }
}

#[cfg(windows)]
mod windows {
    use std::ptr;

    use winapi::um::{
        combaseapi::{CoInitializeEx, CoUninitialize},
        objbase::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE},
        shellapi::ShellExecuteW,
        winbase::{GetSystemPowerStatus, SYSTEM_POWER_STATUS},
        winuser::SW_SHOWNORMAL,
    };

    use super::{PowerInfo, PowerState};

    pub fn open_url(url: &str) -> bool {
        let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
        let (operation, url) = (wide("open"), wide(url));
        // SAFETY: both strings are NUL-terminated and outlive the call.
        unsafe {
            // ShellExecute's documentation asks for COM, which some URL handlers use.
            let com = CoInitializeEx(
                ptr::null_mut(),
                COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE,
            );
            let result = ShellExecuteW(
                ptr::null_mut(),
                operation.as_ptr(),
                url.as_ptr(),
                ptr::null(),
                ptr::null(),
                SW_SHOWNORMAL,
            );
            if com >= 0 {
                CoUninitialize();
            }
            // Values above 32 mean success.
            result as isize > 32
        }
    }

    pub fn power_info() -> PowerInfo {
        // SAFETY: the struct is plain data, which the call fills in.
        let mut status: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
        if unsafe { GetSystemPowerStatus(&mut status) } == 0 || status.BatteryFlag == 0xFF {
            return PowerInfo::UNKNOWN;
        }
        if status.BatteryFlag & 0x80 != 0 {
            return PowerInfo::only(PowerState::NoBattery);
        }
        let state = if status.BatteryFlag & 0x08 != 0 {
            PowerState::Charging
        } else if status.ACLineStatus == 1 {
            PowerState::Charged
        } else {
            PowerState::Battery
        };
        PowerInfo {
            state,
            percent: (status.BatteryLifePercent != 0xFF)
                .then(|| status.BatteryLifePercent.min(100)),
            seconds: (status.BatteryLifeTime != u32::MAX).then_some(status.BatteryLifeTime),
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use core_foundation_sys::{
        array::{CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef},
        base::{
            CFEqual, CFGetTypeID, CFIndex, CFRelease, CFTypeID, CFTypeRef, kCFAllocatorDefault,
        },
        dictionary::{CFDictionaryGetValue, CFDictionaryRef},
        number::{
            CFBooleanGetTypeID, CFBooleanGetValue, CFNumberGetTypeID, CFNumberGetValue,
            kCFNumberSInt32Type,
        },
        string::{CFStringCreateWithBytes, CFStringGetTypeID, kCFStringEncodingUTF8},
    };

    use super::{PowerInfo, PowerState, best_battery};

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOPSCopyPowerSourcesInfo() -> CFTypeRef;
        fn IOPSCopyPowerSourcesList(blob: CFTypeRef) -> CFArrayRef;
        fn IOPSGetPowerSourceDescription(blob: CFTypeRef, source: CFTypeRef) -> CFDictionaryRef;
    }

    pub fn open_url(url: &str) -> bool {
        super::spawn_opener("open", url)
    }

    /// A Core Foundation object from a Create or Copy function, released when dropped.
    struct Owned(CFTypeRef);

    impl Owned {
        fn string(s: &str) -> Owned {
            // SAFETY: the bytes are UTF-8 and outlive the call, which copies them.
            let string = unsafe {
                CFStringCreateWithBytes(
                    kCFAllocatorDefault,
                    s.as_ptr(),
                    s.len() as CFIndex,
                    kCFStringEncodingUTF8,
                    0,
                )
            };
            Owned(string.cast())
        }
    }

    impl Drop for Owned {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: this object owns the reference.
                unsafe { CFRelease(self.0) };
            }
        }
    }

    /// A power source's description: a dictionary with the keys in IOKit's IOPSKeys.h.
    struct Description(CFDictionaryRef);

    impl Description {
        /// The value at `key`, if it has the type `type_id`.
        fn value(&self, key: &str, type_id: CFTypeID) -> Option<CFTypeRef> {
            let key = Owned::string(key);
            if key.0.is_null() {
                return None;
            }
            // SAFETY: the dictionary and key are live, and the value is only borrowed.
            let value = unsafe { CFDictionaryGetValue(self.0, key.0) };
            (!value.is_null() && unsafe { CFGetTypeID(value) } == type_id).then_some(value)
        }

        fn boolean(&self, key: &str) -> Option<bool> {
            // SAFETY: `value` checks that it's a CFBoolean.
            let value = self.value(key, unsafe { CFBooleanGetTypeID() })?;
            Some(unsafe { CFBooleanGetValue(value.cast()) })
        }

        fn integer(&self, key: &str) -> Option<i32> {
            // SAFETY: `value` checks that it's a CFNumber, and `n` fits the type asked for.
            let value = self.value(key, unsafe { CFNumberGetTypeID() })?;
            let mut n = 0i32;
            let converted =
                unsafe { CFNumberGetValue(value.cast(), kCFNumberSInt32Type, (&raw mut n).cast()) };
            converted.then_some(n)
        }

        fn string_is(&self, key: &str, expected: &str) -> bool {
            let expected = Owned::string(expected);
            // SAFETY: `value` checks that it's a CFString, and `expected` is live.
            let value = self.value(key, unsafe { CFStringGetTypeID() });
            value.is_some_and(|v| !expected.0.is_null() && unsafe { CFEqual(v, expected.0) } != 0)
        }
    }

    pub fn power_info() -> PowerInfo {
        // SAFETY: the Copy functions' results are owned and released by `Owned`; the others are
        // borrowed from the list, which outlives them.
        let blob = Owned(unsafe { IOPSCopyPowerSourcesInfo() });
        if blob.0.is_null() {
            return PowerInfo::UNKNOWN;
        }
        let list = Owned(unsafe { IOPSCopyPowerSourcesList(blob.0) }.cast());
        if list.0.is_null() {
            return PowerInfo::UNKNOWN;
        }
        let mut on_ac = false;
        let mut batteries = Vec::new();
        for i in 0..unsafe { CFArrayGetCount(list.0.cast()) } {
            let source = unsafe { CFArrayGetValueAtIndex(list.0.cast(), i) };
            let description = unsafe { IOPSGetPowerSourceDescription(blob.0, source) };
            if description.is_null() {
                continue;
            }
            let source = Description(description);
            if source.boolean("Is Present") == Some(false) {
                continue;
            }
            let ac = source.string_is("Power Source State", "AC Power");
            if !ac && !source.string_is("Power Source State", "Battery Power") {
                continue;
            }
            on_ac |= ac;
            let Some(max) = source.integer("Max Capacity").filter(|&max| max > 0) else {
                continue;
            };
            let charging = source.boolean("Is Charging") == Some(true);
            let percent = source
                .integer("Current Capacity")
                .filter(|&current| current >= 0)
                .map(|current| (f64::from(current) / f64::from(max) * 100.0).min(100.0) as u8);
            // In minutes, and 0 while plugged in.
            let seconds = match source.integer("Time to Empty") {
                Some(minutes) if minutes > 0 || (minutes == 0 && !ac) => Some(minutes as u32 * 60),
                _ => None,
            };
            let state = if charging {
                PowerState::Charging
            } else {
                PowerState::Battery
            };
            batteries.push(PowerInfo {
                state,
                percent,
                seconds,
            });
        }
        match best_battery(batteries) {
            None => PowerInfo::only(PowerState::NoBattery),
            Some(best) if best.state == PowerState::Charging => best,
            Some(best) if on_ac => PowerInfo {
                state: PowerState::Charged,
                ..best
            },
            Some(best) => best,
        }
    }
}

#[cfg(any(target_os = "linux", all(test, not(target_arch = "wasm32"))))]
mod linux {
    use std::{fs, path::Path};

    use super::{PowerInfo, PowerState, best_battery};

    #[cfg(target_os = "linux")]
    pub fn open_url(url: &str) -> bool {
        super::spawn_opener("xdg-open", url)
    }

    #[cfg(target_os = "linux")]
    pub fn power_info() -> PowerInfo {
        power_supplies(Path::new("/sys/class/power_supply"))
    }

    /// Reads the batteries in the kernel's power supply directory.
    pub fn power_supplies(root: &Path) -> PowerInfo {
        let Ok(entries) = fs::read_dir(root) else {
            return PowerInfo::UNKNOWN;
        };
        let batteries = entries.flatten().filter_map(|entry| battery(&entry.path()));
        best_battery(batteries).unwrap_or(PowerInfo::only(PowerState::NoBattery))
    }

    fn battery(dir: &Path) -> Option<PowerInfo> {
        let read = |name: &str| {
            let text = fs::read_to_string(dir.join(name)).ok()?;
            Some(text.trim().to_ascii_lowercase())
        };
        let number = |name: &str| read(name)?.parse::<i64>().ok();
        if read("type")? != "battery" {
            return None;
        }
        // A battery with the "device" scope powers a peripheral, such as a game controller.
        if read("scope").as_deref() == Some("device") {
            return None;
        }
        let state = if read("present").as_deref() == Some("0") {
            PowerState::NoBattery
        } else {
            match read("status").as_deref() {
                Some("charging") => PowerState::Charging,
                Some("discharging") => PowerState::Battery,
                Some("full" | "not charging") => PowerState::Charged,
                _ => PowerState::Unknown,
            }
        };
        let percent = number("capacity")
            .filter(|&percent| percent >= 0)
            .map(|percent| percent.min(100) as u8);
        let seconds = match number("time_to_empty_now") {
            Some(seconds) => (seconds > 0).then_some(seconds as u32),
            // Microwatt-hours over microwatts.
            None if state == PowerState::Battery => {
                match (number("energy_now"), number("power_now")) {
                    (Some(energy), Some(power)) if energy >= 0 && power > 0 => {
                        Some((energy * 3600 / power) as u32)
                    }
                    _ => None,
                }
            }
            None => None,
        };
        Some(PowerInfo {
            state,
            percent,
            seconds,
        })
    }
}

#[cfg(not(any(
    windows,
    target_os = "macos",
    target_os = "linux",
    target_arch = "wasm32"
)))]
mod other {
    use super::PowerInfo;

    pub fn open_url(_url: &str) -> bool {
        false
    }

    pub fn power_info() -> PowerInfo {
        PowerInfo::UNKNOWN
    }
}

/// `window.protogineSystem`, in `web/index.html`.
#[cfg(target_arch = "wasm32")]
mod web {
    use wasm_bindgen::prelude::*;

    use super::PowerInfo;

    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = protogineSystem, js_name = processorCount)]
        pub fn processor_count() -> f64;
        #[wasm_bindgen(js_namespace = protogineSystem, js_name = getClipboardText)]
        pub fn clipboard_text() -> String;
        #[wasm_bindgen(js_namespace = protogineSystem, js_name = setClipboardText)]
        pub fn set_clipboard_text(text: &str);
        #[wasm_bindgen(js_namespace = protogineSystem, js_name = openURL)]
        pub fn open_url(url: &str) -> bool;
        #[wasm_bindgen(js_namespace = protogineSystem)]
        pub fn vibrate(milliseconds: f64);
        /// `[charging, level, chargingTime, dischargingTime]`, with `charging` as 0 or 1.
        #[wasm_bindgen(js_namespace = protogineSystem)]
        fn battery() -> Option<Vec<f64>>;
    }

    pub fn power_info() -> PowerInfo {
        match battery().as_deref() {
            Some(&[charging, level, charging_time, discharging_time]) => {
                super::battery_status(charging != 0.0, level, charging_time, discharging_time)
            }
            _ => PowerInfo::UNKNOWN,
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::{fs, path::PathBuf};

    use super::*;

    #[test]
    fn opens_only_web_and_mail_urls() {
        for url in [
            "https://love2d.org/wiki/love.system",
            "HTTP://example.com",
            "mailto:someone@example.com",
        ] {
            assert!(openable(url), "{url}");
        }
        for url in [
            "",
            "love2d.org",
            "https:",
            "https://",
            "file:///C:/Windows/System32/calc.exe",
            "C:\\Windows\\notepad.exe",
            "javascript:alert(1)",
            "steam://run/1",
            "https://example.com/\nsecond line",
            "https://example.com/\0",
        ] {
            assert!(!openable(url), "{url:?}");
        }
    }

    #[test]
    fn this_machine_has_sensible_power_info() {
        let info = power_info();
        assert!(info.percent.is_none_or(|p| p <= 100), "{info:?}");
        if info.state == PowerState::NoBattery || info.state == PowerState::Unknown {
            assert_eq!(info.percent, None, "{info:?}");
        }
    }

    fn battery(state: PowerState, percent: Option<u8>, seconds: Option<u32>) -> PowerInfo {
        PowerInfo {
            state,
            percent,
            seconds,
        }
    }

    #[test]
    fn picks_the_battery_that_lasts_longest() {
        let short = battery(PowerState::Battery, Some(90), Some(600));
        let long = battery(PowerState::Battery, Some(40), Some(3600));
        let unknown = battery(PowerState::Battery, Some(95), None);
        assert_eq!(best_battery([short, long, unknown]), Some(long));
        // Without times, the most charged one.
        let low = battery(PowerState::Battery, Some(10), None);
        assert_eq!(best_battery([low, unknown]), Some(unknown));
        assert_eq!(best_battery([]), None);
    }

    #[test]
    fn reads_the_browser_battery_status() {
        let info = battery_status(false, 0.5, f64::INFINITY, 5400.0);
        assert_eq!(info, battery(PowerState::Battery, Some(50), Some(5400)));
        let info = battery_status(true, 0.8, 1200.0, f64::INFINITY);
        assert_eq!(info, battery(PowerState::Charging, Some(80), None));
        let info = battery_status(true, 0.99, 0.0, f64::INFINITY);
        assert_eq!(info, battery(PowerState::Charged, Some(99), None));
        // How browsers describe a computer with no battery.
        let info = battery_status(true, 1.0, 0.0, f64::INFINITY);
        assert_eq!(info, PowerInfo::only(PowerState::NoBattery));
    }

    /// A fake /sys/class/power_supply, deleted afterwards.
    struct PowerSupplies(PathBuf);

    impl PowerSupplies {
        fn new(name: &str, supplies: &[(&str, &[(&str, &str)])]) -> Self {
            let root = std::env::temp_dir().join(format!(
                "protogine-power-test-{}-{name}",
                std::process::id()
            ));
            fs::remove_dir_all(&root).ok();
            for (supply, files) in supplies {
                let dir = root.join(supply);
                fs::create_dir_all(&dir).unwrap();
                for (file, contents) in *files {
                    fs::write(dir.join(file), format!("{contents}\n")).unwrap();
                }
            }
            fs::create_dir_all(&root).unwrap();
            PowerSupplies(root)
        }

        fn read(&self) -> PowerInfo {
            linux::power_supplies(&self.0)
        }
    }

    impl Drop for PowerSupplies {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).ok();
        }
    }

    #[test]
    fn reads_linux_batteries() {
        let laptop = PowerSupplies::new(
            "laptop",
            &[
                ("AC", &[("type", "Mains"), ("online", "0")]),
                (
                    "BAT0",
                    &[
                        ("type", "Battery"),
                        ("present", "1"),
                        ("status", "Discharging"),
                        ("capacity", "57"),
                        ("energy_now", "30000000"),
                        ("power_now", "10000000"),
                    ],
                ),
                (
                    "hid-controller-battery",
                    &[
                        ("type", "Battery"),
                        ("scope", "Device"),
                        ("status", "Discharging"),
                        ("capacity", "100"),
                        ("time_to_empty_now", "99999"),
                    ],
                ),
            ],
        );
        assert_eq!(
            laptop.read(),
            battery(PowerState::Battery, Some(57), Some(3 * 3600))
        );

        let charging = PowerSupplies::new(
            "charging",
            &[(
                "BAT1",
                &[
                    ("type", "Battery"),
                    ("status", "Charging"),
                    ("capacity", "140"),
                ],
            )],
        );
        assert_eq!(
            charging.read(),
            battery(PowerState::Charging, Some(100), None)
        );

        let full = PowerSupplies::new(
            "full",
            &[("BAT0", &[("type", "Battery"), ("status", "Not charging")])],
        );
        assert_eq!(full.read(), PowerInfo::only(PowerState::Charged));

        let desktop = PowerSupplies::new("desktop", &[("AC", &[("type", "Mains")])]);
        assert_eq!(desktop.read(), PowerInfo::only(PowerState::NoBattery));

        let missing = PathBuf::from("/nonexistent/power_supply");
        assert_eq!(linux::power_supplies(&missing), PowerInfo::UNKNOWN);
    }
}
