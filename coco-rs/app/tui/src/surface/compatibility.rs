//! Terminal compatibility decisions for native scrollback.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum TerminalCompatibility {
    #[default]
    NativeScrollback,
    ZellijNativeScrollbackDisabled,
}

impl TerminalCompatibility {
    pub(crate) fn detect() -> Self {
        Self::detect_with(|name| {
            std::env::var_os(name).and_then(|value| {
                let text = value.to_string_lossy();
                (!text.is_empty()).then(|| text.into_owned())
            })
        })
    }

    pub(crate) fn detect_with<F>(get_env: F) -> Self
    where
        F: Fn(&str) -> Option<String>,
    {
        if ["ZELLIJ", "ZELLIJ_SESSION_NAME", "ZELLIJ_VERSION"]
            .into_iter()
            .any(|name| get_env(name).is_some_and(|value| !value.is_empty()))
        {
            Self::ZellijNativeScrollbackDisabled
        } else {
            Self::NativeScrollback
        }
    }

    pub(crate) fn native_scrollback_enabled(self) -> bool {
        matches!(self, Self::NativeScrollback)
    }

    pub(crate) fn status_message(self) -> Option<&'static str> {
        match self {
            Self::NativeScrollback => None,
            Self::ZellijNativeScrollbackDisabled => Some("native scrollback disabled in Zellij"),
        }
    }
}

#[cfg(test)]
#[path = "compatibility.test.rs"]
mod tests;
