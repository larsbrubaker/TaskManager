//! Start-up choices read from the environment: `TASKMANAGER_PAGE=<id>` opens a page
//! (`summary`, `startup-apps`, …; see `Page::id`) and `TASKMANAGER_THEME=light|dark|system`
//! picks the look. Without `TASKMANAGER_THEME` the app follows the OS (Match System),
//! except for `TASKMANAGER_SCREENSHOT` runs, which default to Light so captures are the
//! same on every machine.

use taskmanager_core::{Page, ThemeChoice};

/// What the app opens with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Startup {
    pub page: Page,
    pub theme: ThemeChoice,
}

impl Startup {
    /// Read the process environment.
    pub fn from_env() -> Result<Self, String> {
        let var = |name: &str| std::env::var(name).ok();
        Self::parse(
            var("TASKMANAGER_PAGE").as_deref(),
            var("TASKMANAGER_THEME").as_deref(),
            std::env::var_os("TASKMANAGER_SCREENSHOT").is_some(),
        )
    }

    /// The choices for the given variable values (`None`: unset). An empty value counts as
    /// unset; an unknown one is an error naming the accepted values.
    pub fn parse(
        page: Option<&str>,
        theme: Option<&str>,
        screenshot: bool,
    ) -> Result<Self, String> {
        let page = match page.map(str::trim).filter(|p| !p.is_empty()) {
            None => Page::default(),
            Some(id) => Page::from_id(id).ok_or_else(|| {
                format!(
                    "unknown TASKMANAGER_PAGE {id:?}; expected one of: {}",
                    Page::id_list()
                )
            })?,
        };
        let theme = match theme.map(str::trim).filter(|t| !t.is_empty()) {
            None if screenshot => ThemeChoice::Light,
            None => ThemeChoice::System,
            Some(key) => ThemeChoice::from_key(key).ok_or_else(|| {
                format!("unknown TASKMANAGER_THEME {key:?}; expected light, dark or system")
            })?,
        };
        Ok(Self { page, theme })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_summary_and_match_system() {
        let s = Startup::parse(None, None, false).expect("defaults");
        assert_eq!(s.page, Page::Summary);
        assert_eq!(s.theme, ThemeChoice::System);
        let s = Startup::parse(Some(""), Some(" "), false).expect("blank is unset");
        assert_eq!(s.theme, ThemeChoice::System);
    }

    #[test]
    fn screenshots_default_to_light_unless_a_theme_is_given() {
        assert_eq!(
            Startup::parse(None, None, true).expect("ok").theme,
            ThemeChoice::Light
        );
        assert_eq!(
            Startup::parse(None, Some("dark"), true).expect("ok").theme,
            ThemeChoice::Dark
        );
    }

    #[test]
    fn page_and_theme_are_read_and_unknown_values_rejected() {
        let s = Startup::parse(Some("startup-apps"), Some("system"), true).expect("ok");
        assert_eq!(s.page, Page::StartupApps);
        assert_eq!(s.theme, ThemeChoice::System);
        let err = Startup::parse(Some("pro"), None, false).expect_err("unknown page");
        assert!(
            err.contains("summary") && err.contains("benchmarks"),
            "{err}"
        );
        assert!(Startup::parse(None, Some("amber"), false).is_err());
    }
}
