//! The app's destinations (PLAN §2): the fifteen pages listed in the sidebar, in sidebar
//! order and split into the main group and the "MORE" group, plus Settings, reached from the
//! sidebar's footer. Each page has a title (the title row), a sidebar label, a Font Awesome
//! icon and a stable id used by `TASKMANAGER_PAGE`, settings and the tests.
//!
//! The Processor page is titled after the CPU: "Apple Silicon" on Apple-silicon Macs,
//! "Processor" elsewhere. The platform reports which through [`ProcessorBrand`] so the core
//! stays free of `cfg`.

use crate::fonts::fa;

/// What kind of CPU the machine has, as far as page naming goes. The platform reports it;
/// generic until it does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ProcessorBrand {
    #[default]
    Generic,
    /// An Apple M-series chip.
    AppleSilicon,
}

/// Which part of the sidebar a page sits in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PageGroup {
    /// The first block of rows.
    Main,
    /// The rows under the "MORE" divider.
    More,
    /// The footer (Settings).
    Footer,
}

/// One destination.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Page {
    #[default]
    Summary,
    Performance,
    Processes,
    SystemInfo,
    StartupApps,
    Users,
    Services,
    PowerFreq,
    Processor,
    Recorder,
    Connections,
    InstalledApps,
    Drivers,
    DiskSpace,
    Benchmarks,
    Settings,
}

/// The label of the sidebar divider between [`PageGroup::Main`] and [`PageGroup::More`].
pub const MORE_LABEL: &str = "MORE";

impl Page {
    /// The sidebar rows, top to bottom.
    pub const SIDEBAR: [Page; 15] = [
        Page::Summary,
        Page::Performance,
        Page::Processes,
        Page::SystemInfo,
        Page::StartupApps,
        Page::Users,
        Page::Services,
        Page::PowerFreq,
        Page::Processor,
        Page::Recorder,
        Page::Connections,
        Page::InstalledApps,
        Page::Drivers,
        Page::DiskSpace,
        Page::Benchmarks,
    ];

    /// Every page: the sidebar rows, then Settings.
    pub const ALL: [Page; 16] = [
        Page::Summary,
        Page::Performance,
        Page::Processes,
        Page::SystemInfo,
        Page::StartupApps,
        Page::Users,
        Page::Services,
        Page::PowerFreq,
        Page::Processor,
        Page::Recorder,
        Page::Connections,
        Page::InstalledApps,
        Page::Drivers,
        Page::DiskSpace,
        Page::Benchmarks,
        Page::Settings,
    ];

    /// The stable id: `TASKMANAGER_PAGE`, settings, widget names.
    pub fn id(self) -> &'static str {
        match self {
            Page::Summary => "summary",
            Page::Performance => "performance",
            Page::Processes => "processes",
            Page::SystemInfo => "system-info",
            Page::StartupApps => "startup-apps",
            Page::Users => "users",
            Page::Services => "services",
            Page::PowerFreq => "power-freq",
            Page::Processor => "processor",
            Page::Recorder => "recorder",
            Page::Connections => "connections",
            Page::InstalledApps => "installed-apps",
            Page::Drivers => "drivers",
            Page::DiskSpace => "disk-space",
            Page::Benchmarks => "benchmarks",
            Page::Settings => "settings",
        }
    }

    /// The page with id `id` (`None` for anything else). Case and surrounding blanks are
    /// ignored, so `TASKMANAGER_PAGE=Startup-Apps` works.
    pub fn from_id(id: &str) -> Option<Page> {
        let id = id.trim();
        Self::ALL
            .into_iter()
            .find(|p| p.id().eq_ignore_ascii_case(id))
    }

    /// Every id, comma separated (for error messages).
    pub fn id_list() -> String {
        Self::ALL.map(Page::id).join(", ")
    }

    /// The title shown in the title row.
    pub fn title(self, brand: ProcessorBrand) -> &'static str {
        match self {
            Page::Summary => "Summary",
            Page::Performance => "Performance",
            Page::Processes => "Processes",
            Page::SystemInfo => "System Info",
            Page::StartupApps => "Startup apps",
            Page::Users => "Users",
            Page::Services => "Services",
            Page::PowerFreq => "Power & Freq",
            Page::Processor => match brand {
                ProcessorBrand::Generic => "Processor",
                ProcessorBrand::AppleSilicon => "Apple Silicon",
            },
            Page::Recorder => "Flight Recorder",
            Page::Connections => "Connections",
            Page::InstalledApps => "Installed Apps",
            Page::Drivers => "Drivers",
            Page::DiskSpace => "Disk Space",
            Page::Benchmarks => "Benchmarks",
            Page::Settings => "Settings",
        }
    }

    /// The label of the page's sidebar row (or footer button).
    pub fn sidebar_label(self, brand: ProcessorBrand) -> &'static str {
        // The titles are short enough for the sidebar as they are.
        self.title(brand)
    }

    /// The page's Font Awesome icon.
    pub fn icon(self) -> char {
        match self {
            Page::Summary => fa::TACHOMETER,
            Page::Performance => fa::HEARTBEAT,
            Page::Processes => fa::LIST_ALT,
            Page::SystemInfo => fa::INFO_CIRCLE,
            Page::StartupApps => fa::POWER_OFF,
            Page::Users => fa::USERS,
            Page::Services => fa::COGS,
            Page::PowerFreq => fa::LINE_CHART,
            Page::Processor => fa::MICROCHIP,
            Page::Recorder => fa::PAPER_PLANE,
            Page::Connections => fa::GLOBE,
            Page::InstalledApps => fa::TH_LARGE,
            Page::Drivers => fa::CUBE,
            Page::DiskSpace => fa::PIE_CHART,
            Page::Benchmarks => fa::CLOCK_O,
            Page::Settings => fa::COG,
        }
    }

    /// Where the page sits in the sidebar.
    pub fn group(self) -> PageGroup {
        match self {
            Page::Summary
            | Page::Performance
            | Page::Processes
            | Page::SystemInfo
            | Page::StartupApps
            | Page::Users
            | Page::Services => PageGroup::Main,
            Page::Settings => PageGroup::Footer,
            _ => PageGroup::More,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidebar_is_every_page_but_settings_in_order() {
        assert_eq!(&Page::ALL[..15], &Page::SIDEBAR[..]);
        assert_eq!(Page::ALL[15], Page::Settings);
        assert_eq!(Page::default(), Page::Summary);
    }

    #[test]
    fn groups_split_at_the_more_divider() {
        let groups: Vec<PageGroup> = Page::SIDEBAR.iter().map(|p| p.group()).collect();
        assert!(groups[..7].iter().all(|g| *g == PageGroup::Main));
        assert!(groups[7..].iter().all(|g| *g == PageGroup::More));
        assert_eq!(Page::Settings.group(), PageGroup::Footer);
    }

    #[test]
    fn processor_page_is_named_after_the_chip() {
        assert_eq!(Page::Processor.title(ProcessorBrand::Generic), "Processor");
        assert_eq!(
            Page::Processor.sidebar_label(ProcessorBrand::AppleSilicon),
            "Apple Silicon"
        );
        assert_eq!(
            Page::Recorder.sidebar_label(ProcessorBrand::Generic),
            "Flight Recorder"
        );
    }
}
