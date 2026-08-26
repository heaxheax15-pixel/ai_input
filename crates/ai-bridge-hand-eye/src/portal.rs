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

        Ok(format!("remote desktop access granted to {}", request.app_id))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portal_rejects_unlisted_applications() {
        let allowlist = Allowlist::from_toml(r#"
            [[apps]]
            app_id = "org.mozilla.firefox"
            allowed = true
        "#).unwrap();

        let client = PortalClient::new(allowlist);
        let request = PortalRequest {
            app_id: "org.gnome.Terminal".to_string(),
            action: "screenshare".to_string(),
        };

        assert!(matches!(client.call_screen_cast(&request), Err(PortalError::NotAuthorized)));
    }

    #[test]
    fn portal_allows_listed_applications() {
        let allowlist = Allowlist::from_toml(r#"
            [[apps]]
            app_id = "org.mozilla.firefox"
            allowed = true
        "#).unwrap();

        let client = PortalClient::new(allowlist);
        let request = PortalRequest {
            app_id: "org.mozilla.firefox".to_string(),
            action: "screenshare".to_string(),
        };

        assert!(client.call_screen_cast(&request).is_ok());
    }
}
