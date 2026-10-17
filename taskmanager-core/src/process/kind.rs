//! Pure heuristics that decide a process's [`ProcessKind`](super::ProcessKind) from what the
//! OS tells us about it (PLAN §5.3: the Applications / Background groups).
//!
//! The native platform gathers the facts (an executable path, whether the process owns a
//! visible window, which programs desktop entries launch) and asks these functions, so the
//! rules are tested on every OS. They are the M1 heuristics; M5 refines them.
//!
//! - macOS: an app is the main executable of a top-level `.app` bundle, i.e. the path is
//!   `…/Name.app/Contents/MacOS/<exe>` with no `.app` component further up and nothing
//!   below `MacOS/`. Helper apps nested in a bundle (`…/Frameworks/…/Helpers/X Helper.app/
//!   Contents/MacOS/X Helper`), XPC services and login items inside an app are Background
//!   children of the app.
//! - Windows: owning a visible, unowned top-level window (the native side enumerates them).
//! - Linux: launched by a visible `.desktop` entry (see [`super::desktop`]).

/// Whether `exe` is the main executable of a top-level macOS application bundle.
pub fn is_macos_bundle_main_executable(exe: &str) -> bool {
    const MARKER: &str = ".app/Contents/MacOS/";
    let Some(at) = exe.find(MARKER) else {
        return false;
    };
    let bundle_path = &exe[..at];
    let inside = &exe[at + MARKER.len()..];
    // No enclosing bundle (a helper app nested in another app's Contents) …
    let nested = bundle_path.contains(".app/");
    // … and the executable sits directly in MacOS/.
    !nested && !inside.is_empty() && !inside.contains('/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_level_bundle_executables_are_apps() {
        assert!(is_macos_bundle_main_executable(
            "/Applications/Safari.app/Contents/MacOS/Safari"
        ));
        assert!(is_macos_bundle_main_executable(
            "/Users/me/Applications/My Tool.app/Contents/MacOS/My Tool"
        ));
        assert!(is_macos_bundle_main_executable(
            "/System/Library/CoreServices/Finder.app/Contents/MacOS/Finder"
        ));
    }

    #[test]
    fn helpers_services_and_plain_binaries_are_background() {
        assert!(!is_macos_bundle_main_executable(
            "/Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Framework.framework/\
             Versions/1/Helpers/Google Chrome Helper (Renderer).app/Contents/MacOS/Google Chrome Helper (Renderer)"
        ));
        assert!(!is_macos_bundle_main_executable(
            "/Applications/Foo.app/Contents/XPCServices/Bar.xpc/Contents/MacOS/Bar"
        ));
        assert!(!is_macos_bundle_main_executable(
            "/Applications/Foo.app/Contents/MacOS/tools/helper"
        ));
        assert!(!is_macos_bundle_main_executable(
            "/Applications/Foo.app/Contents/MacOS/"
        ));
        assert!(!is_macos_bundle_main_executable("/usr/sbin/syslogd"));
        assert!(!is_macos_bundle_main_executable(""));
    }
}
