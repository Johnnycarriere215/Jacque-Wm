//! Theme tokens for the panel.
//!
//! Fully resolves at compile time; no inheritance or computed values.
//! All colors expressed as `0xAARRGGBB` constants so they can be
//! passed straight into `ID2D1SolidColorBrush` setters.

use std::sync::Arc;

/// A 32-bit color with format `0xAARRGGBB` for Direct2D convenience.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color(pub u32);

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self(0xFF00_0000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
    }
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self(((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
    }
    pub fn r(&self) -> u8 {
        ((self.0 >> 16) & 0xFF) as u8
    }
    pub fn g(&self) -> u8 {
        ((self.0 >> 8) & 0xFF) as u8
    }
    pub fn b(&self) -> u8 {
        (self.0 & 0xFF) as u8
    }
    pub fn a(&self) -> u8 {
        ((self.0 >> 24) & 0xFF) as u8
    }
}

/// Panel-level theme token set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemePalette {
    pub background: Color,
    pub pill_active_bg: Color,
    pub pill_active_text: Color,
    pub pill_inactive_bg: Color,
    pub pill_inactive_text: Color,
    pub center_text: Color,
    pub metric_text: Color,
    pub divider: Color,
    pub shadow: Color,
}

impl ThemePalette {
    /// Omarchy-inspired dark theme matching the spec's `#111111`
    /// background, white active pill, ~45% muted text inactive pill.
    pub fn omarchy_dark() -> Self {
        Self {
            background: Color::rgba(0x11, 0x11, 0x11, 230),
            pill_active_bg: Color::rgb(0xFF, 0xFF, 0xFF),
            pill_active_text: Color::rgb(0x11, 0x11, 0x11),
            pill_inactive_bg: Color::rgba(0, 0, 0, 0),
            pill_inactive_text: Color::rgba(0xFF, 0xFF, 0xFF, 115),
            center_text: Color::rgb(0xEE, 0xEE, 0xEE),
            metric_text: Color::rgba(0xEE, 0xEE, 0xEE, 200),
            divider: Color::rgba(0xFF, 0xFF, 0xFF, 28),
            shadow: Color::rgba(0, 0, 0, 100),
        }
    }
}

impl Default for ThemePalette {
    fn default() -> Self {
        Self::omarchy_dark()
    }
}

/// Hierarchical theme — not used at the moment, kept for forward
/// compatibility when light themes and named tokens are introduced.
#[derive(Debug, Clone)]
pub struct Theme {
    pub palette: ThemePalette,
    pub name: Arc<str>,
    pub corner_radius_px: i32,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            palette: ThemePalette::omarchy_dark(),
            name: Arc::from("omarchy-dark"),
            corner_radius_px: 6,
        }
    }
}

/// What part of the panel a value applies to. Re-exported for
/// callers that want to address the LEFT/CENTER/RIGHT sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelSection {
    Left,
    Center,
    Right,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_components_decode() {
        let c = Color::rgba(0x12, 0x34, 0x56, 0xAB);
        assert_eq!(c.r(), 0x12);
        assert_eq!(c.g(), 0x34);
        assert_eq!(c.b(), 0x56);
        assert_eq!(c.a(), 0xAB);
    }

    #[test]
    fn omarchy_dark_is_dark_neutral() {
        let p = ThemePalette::omarchy_dark();
        assert!(p.background.r() < 32 && p.background.g() < 32 && p.background.b() < 32);
    }
}
