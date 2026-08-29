use crate::allowlist::Allowlist;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortalError {
    NotAuthorized,
    UnsupportedAction,
    MissingAppId,
    Dbus(String),
}

impl std::fmt::Display for PortalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAuthorized => write!(f, "application is not allowed by the portal allowlist"),
            Self::UnsupportedAction => write!(f, "portal action is unsupported"),
            Self::MissingAppId => write!(f, "application identifier is required"),
            Self::Dbus(message) => write!(f, "dbus error: {message}"),
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

        proxy
            .select_sources(
                &session,
                SelectSourcesOptions::default()
                    .set_cursor_mode(CursorMode::Embedded)
                    .set_sources(SourceType::Monitor | SourceType::Window)
                    .set_multiple(true)
                    .set_persist_mode(PersistMode::DoNot),
            )
            .await
            .map_err(|e| PortalError::Dbus(e.to_string()))?;

        let response = proxy
            .start(&session, None, Default::default())
            .await
            .map_err(|e| PortalError::Dbus(e.to_string()))?
            .response()
            .map_err(|e| PortalError::Dbus(e.to_string()))?;

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
