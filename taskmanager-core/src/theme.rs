//! The light and dark looks: the [`ThemeChoice`] setting (Match System, Light or Dark), the
//! agg-gui `Visuals` each look installs, and the [`ChannelPalette`]: the fixed colour of each
//! measured channel (CPU green, memory magenta, GPU blue, …), tuned per look so it reads on
//! that look's panels. TaskManager ships agg-gui's Light and Dark palettes only (PLAN §1,
//! §5.16); the channel colours are ours.
//!
//! `AppState` (`state/theme.rs`) holds the choice and the OS appearance the shell reports,
//! resolves them to light or dark and installs the matching `Visuals`. The instruments
//! (from M2) take their colours from [`ChannelPalette::for_look`].

use agg_gui::theme::Visuals;
use agg_gui::Color;

/// Which look the app uses. Match System by default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ThemeChoice {
    /// Follow the OS appearance; light where the shell cannot tell.
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeChoice {
    /// Every choice, in menu order.
    pub const ALL: [ThemeChoice; 3] = [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark];

    /// The settings key, also the value `TASKMANAGER_THEME` takes.
    pub fn key(self) -> &'static str {
        match self {
            ThemeChoice::System => "system",
            ThemeChoice::Light => "light",
            ThemeChoice::Dark => "dark",
        }
    }

    /// The choice stored as `key` (`None` for anything else). Case and surrounding blanks
    /// are ignored, so `TASKMANAGER_THEME=Dark` works.
    pub fn from_key(key: &str) -> Option<Self> {
        let key = key.trim();
        Self::ALL
            .into_iter()
            .find(|t| t.key().eq_ignore_ascii_case(key))
    }

    /// The label in the Settings page's App theme choice.
    pub fn label(self) -> &'static str {
        match self {
            ThemeChoice::System => "Match System",
            ThemeChoice::Light => "Light",
            ThemeChoice::Dark => "Dark",
        }
    }

    /// Whether this choice shows the dark look, given the OS appearance (`Some(true)` =
    /// dark; `None` when the shell cannot tell, which counts as light).
    pub fn is_dark(self, system_dark: Option<bool>) -> bool {
        match self {
            ThemeChoice::System => system_dark.unwrap_or(false),
            ThemeChoice::Light => false,
            ThemeChoice::Dark => true,
        }
    }
}

/// agg-gui's palette for the light or dark look.
pub fn visuals(dark: bool) -> Visuals {
    if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    }
}

/// A measured quantity with a fixed colour across every page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Channel {
    Cpu,
    /// CPU time spent in the kernel.
    Kernel,
    Memory,
    Gpu,
    /// Neural engine / NPU.
    Npu,
    Disk,
    Network,
    /// Power draw.
    Energy,
    /// Temperatures.
    Thermals,
}

impl Channel {
    /// Every channel.
    pub const ALL: [Channel; 9] = [
        Channel::Cpu,
        Channel::Kernel,
        Channel::Memory,
        Channel::Gpu,
        Channel::Npu,
        Channel::Disk,
        Channel::Network,
        Channel::Energy,
        Channel::Thermals,
    ];
}

/// The channel colours for one look.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChannelPalette {
    pub cpu: Color,
    pub kernel: Color,
    pub memory: Color,
    pub gpu: Color,
    pub npu: Color,
    pub disk: Color,
    pub network: Color,
    pub energy: Color,
    pub thermals: Color,
}

/// An opaque colour from 8-bit channels.
fn rgb8(r: u8, g: u8, b: u8) -> Color {
    Color::rgb(
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
    )
}

impl ChannelPalette {
    /// Colours for the light look: saturated enough to read on white and light grey.
    pub fn light() -> Self {
        Self {
            cpu: rgb8(0x3F, 0xC9, 0x4A),
            kernel: rgb8(0xE5, 0x3E, 0x35),
            memory: rgb8(0xD2, 0x3B, 0xE6),
            gpu: rgb8(0x2D, 0x8C, 0xF0),
            npu: rgb8(0xF0, 0x5A, 0x28),
            disk: rgb8(0x1F, 0xA8, 0x8C),
            network: rgb8(0x2A, 0x9F, 0xD6),
            energy: rgb8(0xE6, 0xB4, 0x1E),
            thermals: rgb8(0xF0, 0x8C, 0x32),
        }
    }

    /// Colours for the dark look: the same hues, a little lighter so they hold their
    /// contrast on dark panels.
    pub fn dark() -> Self {
        Self {
            cpu: rgb8(0x5A, 0xDC, 0x64),
            kernel: rgb8(0xFF, 0x5F, 0x55),
            memory: rgb8(0xE0, 0x6A, 0xF2),
            gpu: rgb8(0x4F, 0xA6, 0xFF),
            npu: rgb8(0xFF, 0x7A, 0x45),
            disk: rgb8(0x3C, 0xCF, 0xAE),
            network: rgb8(0x4C, 0xBC, 0xF0),
            energy: rgb8(0xFF, 0xD0, 0x3C),
            thermals: rgb8(0xFF, 0xA0, 0x4C),
        }
    }

    /// The palette for the light or dark look.
    pub fn for_look(dark: bool) -> Self {
        if dark {
            Self::dark()
        } else {
            Self::light()
        }
    }

    /// The colour of `channel`.
    pub fn color(&self, channel: Channel) -> Color {
        match channel {
            Channel::Cpu => self.cpu,
            Channel::Kernel => self.kernel,
            Channel::Memory => self.memory,
            Channel::Gpu => self.gpu,
            Channel::Npu => self.npu,
            Channel::Disk => self.disk,
            Channel::Network => self.network,
            Channel::Energy => self.energy,
            Channel::Thermals => self.thermals,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip_and_unknown_keys_are_none() {
        for choice in ThemeChoice::ALL {
            assert_eq!(ThemeChoice::from_key(choice.key()), Some(choice));
        }
        assert_eq!(ThemeChoice::from_key(" Dark "), Some(ThemeChoice::Dark));
        assert_eq!(ThemeChoice::from_key("phosphor"), None);
        assert_eq!(ThemeChoice::default(), ThemeChoice::System);
    }

    #[test]
    fn system_follows_the_os_and_falls_back_to_light() {
        assert!(ThemeChoice::System.is_dark(Some(true)));
        assert!(!ThemeChoice::System.is_dark(Some(false)));
        assert!(!ThemeChoice::System.is_dark(None));
        assert!(ThemeChoice::Dark.is_dark(Some(false)));
        assert!(!ThemeChoice::Light.is_dark(Some(true)));
    }

    #[test]
    fn each_look_installs_its_own_visuals() {
        assert!(visuals(true).is_dark());
        assert!(!visuals(false).is_dark());
    }

    fn assert_complete(palette: &ChannelPalette, name: &str) {
        for (i, a) in Channel::ALL.iter().enumerate() {
            let ca = palette.color(*a);
            assert!(ca.a >= 1.0, "{name} {a:?} is not opaque");
            for b in &Channel::ALL[i + 1..] {
                assert_ne!(
                    ca,
                    palette.color(*b),
                    "{name}: {a:?} and {b:?} share a colour"
                );
            }
        }
    }

    #[test]
    fn both_palettes_give_every_channel_its_own_opaque_colour() {
        assert_complete(&ChannelPalette::light(), "light");
        assert_complete(&ChannelPalette::dark(), "dark");
        assert_eq!(ChannelPalette::for_look(false), ChannelPalette::light());
        assert_eq!(ChannelPalette::for_look(true), ChannelPalette::dark());
    }

    #[test]
    fn dark_palette_is_tuned_lighter_than_light() {
        let luma = |c: Color| 0.299 * c.r + 0.587 * c.g + 0.114 * c.b;
        let (light, dark) = (ChannelPalette::light(), ChannelPalette::dark());
        for channel in Channel::ALL {
            assert!(
                luma(dark.color(channel)) > luma(light.color(channel)),
                "{channel:?} is not lighter on the dark look"
            );
        }
    }
}
