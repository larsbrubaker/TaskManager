//! Names for the demo machine's processes: applications and their helpers, background
//! daemons, short-lived tools, and the users they run as.
//!
//! All names are generic (no product names, no real user names); the signed-in user is
//! `demo`. [`super::population`] draws from these lists with the seed, so different seeds
//! give different (but equally plausible) machines.

/// The signed-in user.
pub const DEMO_USER: &str = "demo";
/// The superuser.
pub const ROOT_USER: &str = "root";
/// Service accounts daemons run under.
pub const SERVICE_USERS: [&str; 8] = [
    "_windowserver",
    "_networkd",
    "_locationd",
    "_spotlight",
    "_coreaudiod",
    "_timed",
    "_analyticsd",
    "_mdnsresponder",
];

/// A child process an application starts.
#[derive(Clone, Copy, Debug)]
pub struct HelperSpec {
    pub name: &'static str,
    /// How many of them (a range: drawn per seed).
    pub count: (u64, u64),
    /// Relative CPU appetite.
    pub weight: f64,
    /// Memory range, megabytes (log-uniform).
    pub memory_mb: (f64, f64),
    /// Whether the helper is restarted now and then (renderers).
    pub restarts: bool,
    /// Share of the GPU it accounts for when the app is drawing.
    pub gpu_share: f64,
}

/// An application.
#[derive(Clone, Copy, Debug)]
pub struct AppSpec {
    pub name: &'static str,
    /// Bundle directory (under which the executable lives).
    pub bundle: &'static str,
    /// Always present (otherwise the seed decides).
    pub always: bool,
    pub weight: f64,
    pub memory_mb: (f64, f64),
    pub threads: (u64, u64),
    /// Receives network traffic.
    pub networked: bool,
    pub gpu_share: f64,
    pub helpers: &'static [HelperSpec],
}

const fn helper(name: &'static str, count: (u64, u64), weight: f64, mb: (f64, f64)) -> HelperSpec {
    HelperSpec {
        name,
        count,
        weight,
        memory_mb: mb,
        restarts: false,
        gpu_share: 0.0,
    }
}

const fn renderer(
    name: &'static str,
    count: (u64, u64),
    weight: f64,
    mb: (f64, f64),
) -> HelperSpec {
    HelperSpec {
        restarts: true,
        ..helper(name, count, weight, mb)
    }
}

const fn gpu_helper(name: &'static str, share: f64) -> HelperSpec {
    HelperSpec {
        gpu_share: share,
        ..helper(name, (1, 1), 0.4, (80.0, 260.0))
    }
}

/// The applications the demo can be running.
pub const APPS: [AppSpec; 12] = [
    AppSpec {
        name: "Web Browser",
        bundle: "/Applications/Web Browser.app",
        always: true,
        weight: 0.5,
        memory_mb: (900.0, 1600.0),
        threads: (38, 60),
        networked: true,
        gpu_share: 0.05,
        helpers: &[
            gpu_helper("Web Browser Helper (GPU)", 0.35),
            helper("Web Browser Helper", (2, 3), 0.1, (30.0, 90.0)),
            renderer("Web Browser Helper (Renderer)", (5, 8), 0.5, (60.0, 700.0)),
        ],
    },
    AppSpec {
        name: "Code Editor",
        bundle: "/Applications/Code Editor.app",
        always: true,
        weight: 0.6,
        memory_mb: (700.0, 2600.0),
        threads: (30, 48),
        networked: false,
        gpu_share: 0.02,
        helpers: &[
            gpu_helper("Code Editor Helper (GPU)", 0.12),
            renderer("Code Editor Helper (Renderer)", (1, 2), 0.6, (200.0, 600.0)),
            helper("Code Editor Helper (Plugin)", (2, 3), 1.2, (150.0, 900.0)),
        ],
    },
    AppSpec {
        name: "Terminal",
        bundle: "/System/Applications/Utilities/Terminal.app",
        always: true,
        weight: 0.05,
        memory_mb: (40.0, 120.0),
        threads: (8, 14),
        networked: false,
        gpu_share: 0.01,
        helpers: &[helper("zsh", (2, 3), 0.01, (2.0, 6.0))],
    },
    AppSpec {
        name: "File Manager",
        bundle: "/System/Library/CoreServices/File Manager.app",
        always: true,
        weight: 0.02,
        memory_mb: (40.0, 160.0),
        threads: (6, 12),
        networked: false,
        gpu_share: 0.01,
        helpers: &[],
    },
    AppSpec {
        name: "Chat",
        bundle: "/Applications/Chat.app",
        always: false,
        weight: 0.2,
        memory_mb: (250.0, 600.0),
        threads: (30, 45),
        networked: true,
        gpu_share: 0.02,
        helpers: &[
            gpu_helper("Chat Helper (GPU)", 0.08),
            renderer("Chat Helper (Renderer)", (1, 2), 0.3, (150.0, 450.0)),
        ],
    },
    AppSpec {
        name: "Mail",
        bundle: "/System/Applications/Mail.app",
        always: false,
        weight: 0.05,
        memory_mb: (150.0, 400.0),
        threads: (12, 22),
        networked: true,
        gpu_share: 0.01,
        helpers: &[helper("Mail Web Content", (1, 2), 0.05, (40.0, 120.0))],
    },
    AppSpec {
        name: "Music",
        bundle: "/System/Applications/Music.app",
        always: false,
        weight: 0.15,
        memory_mb: (180.0, 350.0),
        threads: (18, 30),
        networked: true,
        gpu_share: 0.02,
        helpers: &[],
    },
    AppSpec {
        name: "Notes",
        bundle: "/System/Applications/Notes.app",
        always: false,
        weight: 0.02,
        memory_mb: (90.0, 200.0),
        threads: (8, 14),
        networked: false,
        gpu_share: 0.0,
        helpers: &[],
    },
    AppSpec {
        name: "Photos",
        bundle: "/System/Applications/Photos.app",
        always: false,
        weight: 0.08,
        memory_mb: (300.0, 900.0),
        threads: (20, 34),
        networked: false,
        gpu_share: 0.06,
        helpers: &[],
    },
    AppSpec {
        name: "Calendar",
        bundle: "/System/Applications/Calendar.app",
        always: false,
        weight: 0.01,
        memory_mb: (60.0, 140.0),
        threads: (6, 10),
        networked: true,
        gpu_share: 0.0,
        helpers: &[],
    },
    AppSpec {
        name: "Settings",
        bundle: "/System/Applications/Settings.app",
        always: false,
        weight: 0.02,
        memory_mb: (60.0, 150.0),
        threads: (6, 12),
        networked: false,
        gpu_share: 0.0,
        helpers: &[helper("Settings Extension", (1, 2), 0.01, (20.0, 60.0))],
    },
    AppSpec {
        name: "Document Viewer",
        bundle: "/System/Applications/Document Viewer.app",
        always: false,
        weight: 0.01,
        memory_mb: (50.0, 180.0),
        threads: (5, 9),
        networked: false,
        gpu_share: 0.0,
        helpers: &[],
    },
];

/// Well-known daemons with fixed users and appetites: `(name, user, weight, memory MB,
/// gpu share)`. User `""` means the signed-in user.
pub const NOTABLE_DAEMONS: [(&str, &str, f64, f64, f64); 16] = [
    ("windowd", "_windowserver", 1.4, 380.0, 0.3),
    ("searchindexd", "root", 0.4, 120.0, 0.0),
    ("cloudsyncd", "", 0.15, 90.0, 0.0),
    ("photoanalysisd", "", 0.1, 260.0, 0.05),
    ("audiod", "_coreaudiod", 0.12, 30.0, 0.0),
    ("networkd", "_networkd", 0.05, 18.0, 0.0),
    ("locationd", "_locationd", 0.04, 22.0, 0.0),
    ("logd", "root", 0.08, 25.0, 0.0),
    ("configd", "root", 0.03, 12.0, 0.0),
    ("powerd", "root", 0.02, 8.0, 0.0),
    ("dock", "", 0.03, 60.0, 0.02),
    ("controlcenter", "", 0.03, 45.0, 0.01),
    ("notificationd", "", 0.02, 40.0, 0.0),
    ("spotlight", "", 0.02, 45.0, 0.0),
    ("mdnsd", "_mdnsresponder", 0.02, 9.0, 0.0),
    ("analyticsd", "_analyticsd", 0.02, 20.0, 0.0),
];

/// Daemons that run once per user (root, the signed-in user, and a service account).
pub const PER_USER_DAEMONS: [&str; 14] = [
    "prefsd",
    "distnoted",
    "trustd",
    "secd",
    "lsd",
    "usernoted",
    "xpcservice",
    "keybagd",
    "tccd",
    "pboard",
    "fontworker",
    "cfnetworkd",
    "contextstored",
    "biomed",
];

/// Subsystem words for composing the rest of the daemon names.
pub const DAEMON_PREFIXES: [&str; 64] = [
    "accessibility",
    "account",
    "activity",
    "airplay",
    "alert",
    "appstore",
    "asset",
    "audioanalytics",
    "backup",
    "battery",
    "biometric",
    "bluetooth",
    "bookmark",
    "calendar",
    "camera",
    "clipboard",
    "cloudphoto",
    "contacts",
    "continuity",
    "crashreport",
    "display",
    "diskarbitration",
    "diskimage",
    "dictation",
    "family",
    "findmy",
    "font",
    "gamecontroller",
    "handoff",
    "help",
    "homekit",
    "iconservices",
    "identity",
    "imagecapture",
    "input",
    "keyboard",
    "language",
    "mediaanalysis",
    "mediaremote",
    "message",
    "metadata",
    "migration",
    "mobileasset",
    "news",
    "nearby",
    "passkit",
    "print",
    "quicklook",
    "reminder",
    "remotemgmt",
    "screentime",
    "sharing",
    "siriknowledge",
    "softwareupdate",
    "speech",
    "spell",
    "storage",
    "symptoms",
    "thermalmonitor",
    "timesync",
    "usage",
    "usb",
    "wallpaper",
    "weather",
];

/// Endings for composed daemon names.
pub const DAEMON_SUFFIXES: [&str; 8] = [
    "d",
    "agent",
    "helper",
    "service",
    "server",
    "monitor",
    "worker",
    "extension",
];

/// Executable directories, by owner.
pub const ROOT_DIRS: [&str; 3] = [
    "/usr/libexec/",
    "/usr/sbin/",
    "/System/Library/CoreServices/",
];
pub const USER_DIRS: [&str; 3] = [
    "/System/Library/CoreServices/",
    "/usr/libexec/",
    "/System/Library/PrivateFrameworks/Support/",
];

/// A short-lived tool that starts and exits on its own.
#[derive(Clone, Copy, Debug)]
pub struct TransientSpec {
    pub name: &'static str,
    pub user: &'static str,
    /// Started from a Terminal shell (otherwise by pid 1).
    pub from_shell: bool,
    /// Lifetime range, seconds.
    pub lifetime: (f64, f64),
    /// Relative CPU appetite.
    pub weight: f64,
    pub memory_mb: f64,
}

const fn transient(
    name: &'static str,
    user: &'static str,
    from_shell: bool,
    lifetime: (f64, f64),
    weight: f64,
    memory_mb: f64,
) -> TransientSpec {
    TransientSpec {
        name,
        user,
        from_shell,
        lifetime,
        weight,
        memory_mb,
    }
}

/// The tools that come and go.
pub const TRANSIENTS: [TransientSpec; 8] = [
    transient("git", DEMO_USER, true, (0.5, 4.0), 0.6, 9.0),
    transient("mdworker", DEMO_USER, false, (6.0, 40.0), 0.3, 24.0),
    transient("xpcproxy", ROOT_USER, false, (0.2, 2.5), 0.05, 4.0),
    transient("sh", DEMO_USER, true, (0.5, 6.0), 0.05, 2.0),
    transient("screencapture", DEMO_USER, false, (1.0, 5.0), 0.2, 18.0),
    transient("security", DEMO_USER, true, (0.5, 3.0), 0.05, 5.0),
    transient("sleep", DEMO_USER, true, (2.0, 10.0), 0.0, 0.5),
    transient(
        "softwareupdate-check",
        ROOT_USER,
        false,
        (3.0, 12.0),
        0.3,
        30.0,
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn enough_unique_daemon_names() {
        let mut names = HashSet::new();
        for p in DAEMON_PREFIXES {
            for s in DAEMON_SUFFIXES {
                names.insert(format!("{p}{s}"));
            }
        }
        assert_eq!(names.len(), DAEMON_PREFIXES.len() * DAEMON_SUFFIXES.len());
        assert!(names.len() >= 500);
    }

    #[test]
    fn four_apps_are_always_running() {
        assert_eq!(APPS.iter().filter(|a| a.always).count(), 4);
        assert!(APPS.iter().all(|a| a.bundle.ends_with(".app")));
    }
}
