//! Web browsers by desktop file id. Kairos Shield is a Chrome extension, so only Chromium
//! browsers can run it; site rules never reach any other browser.

/// In the order the starter groups prefer one.
pub const CHROMIUM: &[&str] = &[
    "google-chrome.desktop",
    "brave-browser.desktop",
    "microsoft-edge.desktop",
    "vivaldi-stable.desktop",
    "chromium.desktop",
    "chromium-browser.desktop",
    "google-chrome-beta.desktop",
    "google-chrome-unstable.desktop",
    "microsoft-edge-beta.desktop",
    "opera.desktop",
    "com.google.Chrome.desktop",
    "com.brave.Browser.desktop",
    "org.chromium.Chromium.desktop",
    "com.microsoft.Edge.desktop",
    "com.vivaldi.Vivaldi.desktop",
];

pub const OTHER: &[&str] = &[
    "firefox.desktop",
    "firefox-esr.desktop",
    "org.mozilla.firefox.desktop",
    "firefox_firefox.desktop",
    "librewolf.desktop",
    "io.gitlab.librewolf-community.desktop",
    "waterfox.desktop",
    "net.waterfox.waterfox.desktop",
    "mullvad-browser.desktop",
    "torbrowser.desktop",
    "org.torproject.torbrowser-launcher.desktop",
    "zen.desktop",
    "app.zen_browser.zen.desktop",
    "org.gnome.Epiphany.desktop",
    "org.kde.falkon.desktop",
    "falkon.desktop",
    "midori.desktop",
];

pub fn is_chromium(app: &str) -> bool {
    CHROMIUM.contains(&app)
}

pub fn is_browser(app: &str) -> bool {
    is_chromium(app) || OTHER.contains(&app)
}

/// The first installed Chromium browser, to pin in the starter groups.
pub fn preferred_chromium(installed: &dyn Fn(&str) -> bool) -> Option<&'static str> {
    CHROMIUM.iter().copied().find(|id| installed(id))
}
