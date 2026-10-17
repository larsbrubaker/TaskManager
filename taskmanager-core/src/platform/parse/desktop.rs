//! Parser for freedesktop `.desktop` entries, used to classify Linux processes as
//! Applications (PLAN §5.3).
//!
//! The native Linux platform reads every `*.desktop` file under `/usr/share/applications`
//! and `~/.local/share/applications`, parses them here and collects the program names
//! their `Exec=` (and `TryExec=`) lines launch. A process whose executable's file name is
//! in that set is listed under Applications; everything else is Background. This is the
//! M1 heuristic (narrower than "has a DISPLAY", which every child of a terminal has); M5
//! refines it with window ownership.
//!
//! Only the `[Desktop Entry]` group counts; actions (`[Desktop Action …]`) are other
//! launch lines of the same app. Entries that are not `Type=Application`, or are hidden
//! (`Hidden=true`, `NoDisplay=true`), are not user-facing launchers and are skipped.

use std::collections::HashSet;

/// The parts of a desktop entry the classifier needs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DesktopEntry {
    /// The program `Exec=` launches, as a file name (`firefox`, not `/usr/bin/firefox`).
    pub exec_program: Option<String>,
    /// The program `TryExec=` names, as a file name.
    pub try_exec_program: Option<String>,
    /// `Type=Application`.
    pub is_application: bool,
    /// `Hidden=true` or `NoDisplay=true`.
    pub hidden: bool,
}

impl DesktopEntry {
    /// Whether this entry is a visible application launcher.
    pub fn is_launcher(&self) -> bool {
        self.is_application && !self.hidden
    }
}

/// Parse a `.desktop` file. `None` if it has no `[Desktop Entry]` group.
pub fn parse_desktop_entry(text: &str) -> Option<DesktopEntry> {
    let mut entry = DesktopEntry::default();
    let mut in_main_group = false;
    let mut saw_main_group = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_main_group = line == "[Desktop Entry]";
            saw_main_group |= in_main_group;
            continue;
        }
        if !in_main_group {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "Type" => entry.is_application = value == "Application",
            "Exec" => entry.exec_program = exec_program(value),
            "TryExec" => entry.try_exec_program = program_file_name(value),
            "Hidden" | "NoDisplay" if value.eq_ignore_ascii_case("true") => entry.hidden = true,
            _ => {}
        }
    }
    saw_main_group.then_some(entry)
}

/// The program an `Exec=` value runs, as a file name: the first word, unquoted, skipping a
/// leading `env` and its `NAME=value` assignments (`env GDK_BACKEND=x11 /opt/app/bin/app
/// %U` → `app`). `None` for an empty line.
pub fn exec_program(exec: &str) -> Option<String> {
    let mut words = exec_words(exec).into_iter().peekable();
    if words
        .peek()
        .is_some_and(|w| program_file_name(w).as_deref() == Some("env"))
    {
        words.next();
        while words
            .peek()
            .is_some_and(|w| w.contains('=') && !w.starts_with('/'))
        {
            words.next();
        }
    }
    words.next().and_then(|w| program_file_name(&w))
}

/// The program names a set of desktop entries launch (visible launchers only).
pub fn launcher_programs<'a>(
    entries: impl IntoIterator<Item = &'a DesktopEntry>,
) -> HashSet<String> {
    let mut names = HashSet::new();
    for entry in entries.into_iter().filter(|e| e.is_launcher()) {
        names.extend(entry.exec_program.iter().cloned());
        names.extend(entry.try_exec_program.iter().cloned());
    }
    names
}

/// The last path component of `path` (`/usr/bin/gedit` → `gedit`); `None` if empty.
pub fn program_file_name(path: &str) -> Option<String> {
    let name = path.trim().rsplit('/').next().unwrap_or("");
    (!name.is_empty()).then(|| name.to_string())
}

/// Split an `Exec=` value into words, honouring double quotes and backslash escapes
/// inside them (the Desktop Entry spec's quoting rules).
fn exec_words(exec: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_quotes = false;
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => in_quotes = !in_quotes,
            '\\' if in_quotes => {
                if let Some(escaped) = chars.next() {
                    word.push(escaped);
                }
            }
            c if c.is_whitespace() && !in_quotes => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            c => word.push(c),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIREFOX: &str = "\
[Desktop Entry]
Version=1.0
Name=Firefox Web Browser
Exec=/usr/lib/firefox/firefox %u
TryExec=firefox
Terminal=false
Type=Application
Icon=firefox

[Desktop Action new-window]
Name=Open a New Window
Exec=/usr/lib/firefox/firefox-bin -new-window
";

    #[test]
    fn reads_the_main_group_only() {
        let entry = parse_desktop_entry(FIREFOX).expect("has a main group");
        assert_eq!(entry.exec_program.as_deref(), Some("firefox"));
        assert_eq!(entry.try_exec_program.as_deref(), Some("firefox"));
        assert!(entry.is_launcher());
    }

    #[test]
    fn quoted_paths_and_env_prefixes_name_the_real_program() {
        assert_eq!(
            exec_program("\"/opt/My App/bin/my app\" --flag %F").as_deref(),
            Some("my app")
        );
        assert_eq!(
            exec_program("env GDK_BACKEND=x11 BAMF=1 /opt/app/bin/app %U").as_deref(),
            Some("app")
        );
        assert_eq!(
            exec_program("/usr/bin/env FOO=1 code --new-window").as_deref(),
            Some("code")
        );
        assert_eq!(
            exec_program("\"a \\\"b\\\"\" x").as_deref(),
            Some("a \"b\"")
        );
        assert_eq!(exec_program("   "), None);
    }

    #[test]
    fn hidden_and_non_application_entries_are_not_launchers() {
        let hidden =
            parse_desktop_entry("[Desktop Entry]\nType=Application\nExec=helper\nNoDisplay=true\n")
                .expect("parses");
        assert!(!hidden.is_launcher());
        let link = parse_desktop_entry("[Desktop Entry]\nType=Link\nURL=https://example.com\n")
            .expect("parses");
        assert!(!link.is_launcher());
        assert_eq!(parse_desktop_entry("[Other]\nExec=x\n"), None);
        assert_eq!(parse_desktop_entry(""), None);
    }

    #[test]
    fn launcher_programs_collects_exec_and_try_exec() {
        let firefox = parse_desktop_entry(FIREFOX).expect("parses");
        let gedit = parse_desktop_entry(
            "[Desktop Entry]\nType=Application\nExec=gedit %U\nTryExec=gedit-wrapper\n",
        )
        .expect("parses");
        let hidden =
            parse_desktop_entry("[Desktop Entry]\nType=Application\nExec=daemon\nHidden=true\n")
                .expect("parses");
        let names = launcher_programs([&firefox, &gedit, &hidden]);
        let mut sorted: Vec<_> = names.into_iter().collect();
        sorted.sort();
        assert_eq!(sorted, vec!["firefox", "gedit", "gedit-wrapper"]);
        assert_eq!(program_file_name("/usr/bin/").as_deref(), None);
        assert_eq!(program_file_name("bash").as_deref(), Some("bash"));
    }
}
