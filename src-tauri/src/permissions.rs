#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessibilityPermissionPayload {
    pub supported: bool,
    pub granted: bool,
}

pub fn accessibility_permission_status() -> AccessibilityPermissionPayload {
    AccessibilityPermissionPayload {
        supported: cfg!(target_os = "macos"),
        granted: platform::accessibility_permission_granted(),
    }
}

pub fn ensure_accessibility_permission() -> Result<(), String> {
    let status = accessibility_permission_status();
    if status.supported && !status.granted {
        return Err(String::from(
            "BlurAutoClicker needs Accessibility access on macOS to move and click the mouse. Use \"Allow Access\" in the app, or enable it in System Settings > Privacy & Security > Accessibility.",
        ));
    }

    Ok(())
}

pub fn request_accessibility_permission() -> Result<AccessibilityPermissionPayload, String> {
    platform::request_accessibility_permission()?;
    Ok(accessibility_permission_status())
}

pub fn open_accessibility_settings() -> Result<(), String> {
    platform::open_accessibility_settings()
}

#[cfg(target_os = "macos")]
mod platform {
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation::string::{CFString, CFStringRef};
    use std::process::Command;

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> u8;
        fn AXIsProcessTrustedWithOptions(theDict: CFDictionaryRef) -> u8;
        static kAXTrustedCheckOptionPrompt: CFStringRef;
    }

    pub fn accessibility_permission_granted() -> bool {
        unsafe { AXIsProcessTrusted() != 0 }
    }

    pub fn request_accessibility_permission() -> Result<(), String> {
        let prompt_key = unsafe { CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt) };
        let options: CFDictionary<CFString, CFBoolean> =
            CFDictionary::from_CFType_pairs(&[(prompt_key, CFBoolean::true_value())]);

        unsafe {
            AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef());
        }

        Ok(())
    }

    pub fn open_accessibility_settings() -> Result<(), String> {
        let attempts: &[&[&str]] = &[
            &[
                "open",
                "x-apple.systempreferences:com.apple.Settings.PrivacySecurity.extension?Privacy_Accessibility",
            ],
            &[
                "open",
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
            ],
            &["open", "/System/Applications/System Settings.app"],
            &["open", "-b", "com.apple.systempreferences"],
            &["open", "-b", "com.apple.SystemSettings"],
        ];

        for attempt in attempts {
            let mut command = Command::new(attempt[0]);
            command.args(&attempt[1..]);

            match command.status() {
                Ok(status) if status.success() => return Ok(()),
                Ok(_) | Err(_) => continue,
            }
        }

        Err(String::from(
            "Failed to open macOS Accessibility settings automatically.",
        ))
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    pub fn accessibility_permission_granted() -> bool {
        true
    }

    pub fn request_accessibility_permission() -> Result<(), String> {
        Ok(())
    }

    pub fn open_accessibility_settings() -> Result<(), String> {
        Ok(())
    }
}
