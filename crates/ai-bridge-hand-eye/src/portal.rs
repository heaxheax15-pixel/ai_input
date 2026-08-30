use crate::allowlist::Allowlist;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortalError {
    NotAuthorized,
    UnsupportedAction,
    MissingAppId,
    Dbus(String),
    Clipboard(String),
}

impl std::fmt::Display for PortalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAuthorized => write!(f, "application is not allowed by the portal allowlist"),
            Self::UnsupportedAction => write!(f, "portal action is unsupported"),
            Self::MissingAppId => write!(f, "application identifier is required"),
            Self::Dbus(message) => write!(f, "dbus error: {message}"),
            Self::Clipboard(message) => write!(f, "clipboard error: {message}"),
        }
    }
}

impl std::error::Error for PortalError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalRequest {
    pub app_id: String,
    pub action: String,
}

pub struct PortalClient {
    pub allowlist: Allowlist,
}

impl PortalClient {
    pub fn new(allowlist: Allowlist) -> Self {
        Self { allowlist }
    }

    pub fn write_image_to_clipboard(image: &[u8], width: usize, height: usize) -> Result<(), PortalError> {
        let mut clipboard = arboard::Clipboard::new().map_err(|e| PortalError::Clipboard(e.to_string()))?;
        let data = arboard::ImageData {
            width,
            height,
            bytes: std::borrow::Cow::Owned(image.to_vec()),
        };
        clipboard.set_image(data).map_err(|e| PortalError::Clipboard(e.to_string()))?;
        Ok(())
    }

    pub fn authorize(&self, app_id: &str) -> Result<(), PortalError> {
        if app_id.trim().is_empty() {
            return Err(PortalError::MissingAppId);
        }

        if !self.allowlist.is_allowed(app_id) {
            return Err(PortalError::NotAuthorized);
        }

        Ok(())
    }

    pub fn call_screen_cast(&self, request: &PortalRequest) -> Result<String, PortalError> {
        self.authorize(&request.app_id)?;
        if request.action != "screenshare" {
            return Err(PortalError::UnsupportedAction);
        }

        Ok(format!("screen-cast started for {}", request.app_id))
    }

    pub fn call_remote_desktop(&self, request: &PortalRequest) -> Result<String, PortalError> {
        self.authorize(&request.app_id)?;
        if request.action != "remote-input" {
            return Err(PortalError::UnsupportedAction);
        }

        Ok(format!(
            "remote desktop access granted to {}",
            request.app_id
        ))
    }

    pub fn require_allowed_app_id(&self, app_id: &str, target: &str) -> Result<(), PortalError> {
        let _ = target;
        self.authorize(app_id)
    }
}

pub fn ensure_allowed_application(allowlist: &Allowlist, app_id: &str) -> Result<(), PortalError> {
    if app_id.trim().is_empty() {
        return Err(PortalError::MissingAppId);
    }

    if !allowlist.is_allowed(app_id) {
        return Err(PortalError::NotAuthorized);
    }

    Ok(())
}

pub fn portal_for_target(allowlist: &Allowlist, target: &str) -> Result<(), PortalError> {
    ensure_allowed_application(allowlist, target)
}

pub fn build_failure_status_text(app_id: &str, failure_reason: &str, elapsed: Duration) -> String {
    let reason = if failure_reason.trim().is_empty() {
        "SilentTimeout".to_string()
    } else {
        failure_reason.trim().to_string()
    };
    let elapsed_label = if elapsed.as_secs() >= 1 {
        format!("{}s", elapsed.as_secs())
    } else {
        "<1s".to_string()
    };

    format!(
        "App: {app_id}\nFailure: {reason}\nElapsed since last response: {elapsed_label}\nAction: role swapped to the remaining healthy branch and recovery initiated."
    )
}

/// Real Wayland portal integration via `ashpd`, enabled only by the
/// `real-portal` cargo feature so that mock environments keep their tests.
///
/// Policy carried here:
/// - **Hand (RemoteDesktop):** mouse/keyboard injection events are restricted
///   exclusively to applications present in the `allowlist.toml`.
/// - **Eye (ScreenCast):** used exclusively for image capture or self-diagnosis
///   in the ops room. Reading text via OCR is explicitly forbidden.
#[cfg(feature = "real-portal")]
pub mod real {
    use super::*;
    use ashpd::desktop::{
        remote_desktop::{DeviceType, KeyState, RemoteDesktop, SelectDevicesOptions},
        screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType},
        PersistMode,
    };
    use std::fs;
    use std::path::PathBuf;

    const PORTAL_RESTORE_TOKEN_FILENAME: &str = "portal_restore.token";

    fn portal_restore_path() -> PathBuf {
        std::env::temp_dir().join("ai_bridge_runtime_sockets").join(PORTAL_RESTORE_TOKEN_FILENAME)
    }

    fn write_restore_token(token: &str) -> Result<(), PortalError> {
        let path = portal_restore_path();
        fs::create_dir_all(path.parent().unwrap()).map_err(|e| PortalError::Dbus(e.to_string()))?;
        fs::write(&path, token).map_err(|e| PortalError::Dbus(e.to_string()))?;
        fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| PortalError::Dbus(format!("failed to set permissions on restore token: {e}")))?;
        Ok(())
    }

    fn read_restore_token() -> Option<String> {
        let path = portal_restore_path();
        fs::read_to_string(path).ok()
    }

    /// Guards that only allowlisted applications can drive the hand.
    pub fn authorize_injection(allowlist: &Allowlist, app_id: &str) -> Result<(), PortalError> {
        ensure_allowed_application(allowlist, app_id)
    }

    /// The eye portal is reserved for image capture and self-diagnosis. OCR text
    /// reading is not a permitted use of the capture stream. This returns an
    /// error whenever an OCR-style request is attempted.
    pub fn authorize_capture(
        allowlist: &Allowlist,
        app_id: &str,
        purpose: CapturePurpose,
    ) -> Result<(), PortalError> {
        ensure_allowed_application(allowlist, app_id)?;
        match purpose {
            CapturePurpose::Capture | CapturePurpose::SelfDiagnosis => Ok(()),
            CapturePurpose::OcrTextReading => Err(PortalError::UnsupportedAction),
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum CapturePurpose {
        Capture,
        SelfDiagnosis,
        OcrTextReading,
    }

    /// Hand: opens a RemoteDesktop session restricted to allowlisted apps and
    /// injects a keyboard or pointer event within that restricted session.
    #[allow(clippy::too_many_arguments)]
    pub async fn remote_desktop_inject(
        allowlist: &Allowlist,
        app_id: &str,
        event: InputEvent,
    ) -> Result<String, PortalError> {
        authorize_injection(allowlist, app_id)?;

        let proxy = RemoteDesktop::new()
            .await
            .map_err(|e| PortalError::Dbus(e.to_string()))?;
        let session = proxy
            .create_session(Default::default())
            .await
            .map_err(|e| PortalError::Dbus(e.to_string()))?;

        proxy
            .select_devices(
                &session,
                SelectDevicesOptions::default()
                    .set_devices(DeviceType::Keyboard | DeviceType::Pointer),
            )
            .await
            .map_err(|e| PortalError::Dbus(e.to_string()))?;

        match event {
            InputEvent::KeyPress(keycode) => proxy
                .notify_keyboard_keycode(&session, keycode, KeyState::Pressed, Default::default())
                .await
                .map_err(|e| PortalError::Dbus(e.to_string()))?,
            InputEvent::KeyRelease(keycode) => proxy
                .notify_keyboard_keycode(&session, keycode, KeyState::Released, Default::default())
                .await
                .map_err(|e| PortalError::Dbus(e.to_string()))?,
        }

        Ok(format!("remote desktop input injected for {app_id}"))
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum InputEvent {
        KeyPress(i32),
        KeyRelease(i32),
    }

    pub fn build_failure_status_text(app_id: &str, failure_reason: &str, elapsed: Duration) -> String {
        super::build_failure_status_text(app_id, failure_reason, elapsed)
    }

    /// TODO: real capture requires reading actual frame bytes from the PipeWire
    /// screencast stream opened by the portal session. `screen_cast_capture()`
    /// only creates/starts the session and does not currently expose pixel data.
    pub async fn capture_failure_window_image(
        _allowlist: &Allowlist,
        _app_id: &str,
        _purpose: CapturePurpose,
    ) -> Result<std::path::PathBuf, PortalError> {
        Err(PortalError::UnsupportedAction)
    }

    /// TODO: the real implementation requires a verified, display-backed capture
    /// pipeline that yields actual frame data, real image dimensions, and a text
    /// input method that is not built from hand-written keycode guesses.
    pub async fn capture_and_inject_failure_context(
        _allowlist: &Allowlist,
        _failed_app_id: &str,
        _new_maestro_app_id: &str,
        _failure_reason: &str,
        _elapsed: Duration,
    ) -> Result<(), PortalError> {
        Err(PortalError::UnsupportedAction)
    }

    /// Eye: opens a ScreenCast session used exclusively for image capture or
    /// self-diagnosis. OCR text reading is statically rejected by the caller.
    pub async fn screen_cast_capture(
        allowlist: &Allowlist,
        app_id: &str,
        purpose: CapturePurpose,
    ) -> Result<String, PortalError> {
        authorize_capture(allowlist, app_id, purpose)?;

        let proxy = Screencast::new()
            .await
            .map_err(|e| PortalError::Dbus(e.to_string()))?;
        let session = proxy
            .create_session(Default::default())
            .await
            .map_err(|e| PortalError::Dbus(e.to_string()))?;

        let restore = read_restore_token();
        let options = SelectSourcesOptions::default()
            .set_cursor_mode(CursorMode::Embedded)
            .set_sources(SourceType::Monitor | SourceType::Window)
            .set_multiple(true)
            .set_persist_mode(PersistMode::Application)
            .set_restore_token(restore.as_deref());

        proxy
            .select_sources(&session, options)
            .await
            .map_err(|e| PortalError::Dbus(e.to_string()))?;

        let response = proxy
            .start(&session, None, Default::default())
            .await
            .map_err(|e| PortalError::Dbus(e.to_string()))?
            .response()
            .map_err(|e| PortalError::Dbus(e.to_string()))?;

        if let Some(token) = response.restore_token() {
            let _ = write_restore_token(token);
        }

        let node_count = response.streams().len();
        Ok(format!(
            "screen cast capture started for {app_id} ({node_count} streams)"
        ))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn allowlist() -> Allowlist {
            Allowlist::from_toml(
                r#"
                [[apps]]
                app_id = "org.gnome.Terminal"
                allowed = true
            "#,
            )
            .unwrap()
        }

        #[test]
        fn injection_is_restricted_to_allowlisted_apps() {
            let l = allowlist();
            assert!(authorize_injection(&l, "org.gnome.Terminal").is_ok());
            assert!(matches!(
                authorize_injection(&l, "org.mozilla.firefox"),
                Err(PortalError::NotAuthorized)
            ));
        }

        #[test]
        fn capture_rejects_ocr_text_reading() {
            let l = allowlist();
            assert!(authorize_capture(&l, "org.gnome.Terminal", CapturePurpose::Capture).is_ok());
            assert!(
                authorize_capture(&l, "org.gnome.Terminal", CapturePurpose::SelfDiagnosis).is_ok()
            );
            assert!(matches!(
                authorize_capture(&l, "org.gnome.Terminal", CapturePurpose::OcrTextReading),
                Err(PortalError::UnsupportedAction)
            ));
        }

        #[test]
        fn failure_status_text_uses_app_id_reason_and_elapsed() {
            let text = super::super::build_failure_status_text(
                "org.gnome.Terminal",
                "SilentTimeout",
                std::time::Duration::from_secs(45),
            );
            assert!(text.contains("org.gnome.Terminal"));
            assert!(text.contains("SilentTimeout"));
            assert!(text.contains("45s"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portal_rejects_unlisted_applications() {
        let allowlist = Allowlist::from_toml(
            r#"
            [[apps]]
            app_id = "org.mozilla.firefox"
            allowed = true
        "#,
        )
        .unwrap();

        let client = PortalClient::new(allowlist);
        let request = PortalRequest {
            app_id: "org.gnome.Terminal".to_string(),
            action: "screenshare".to_string(),
        };

        assert!(matches!(
            client.call_screen_cast(&request),
            Err(PortalError::NotAuthorized)
        ));
    }

    #[test]
    fn portal_allows_listed_applications() {
        let allowlist = Allowlist::from_toml(
            r#"
            [[apps]]
            app_id = "org.mozilla.firefox"
            allowed = true
        "#,
        )
        .unwrap();

        let client = PortalClient::new(allowlist);
        let request = PortalRequest {
            app_id: "org.mozilla.firefox".to_string(),
            action: "screenshare".to_string(),
        };

        assert!(client.call_screen_cast(&request).is_ok());
    }
}
