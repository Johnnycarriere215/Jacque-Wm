//! Key codes, modifier bitflags, and the [`Hotkey`] definition.
//!
//! These types are intentionally OS-agnostic. The platform keyboard
//! hook translates raw Win32 virtual key codes into [`KeyCode`] values
//! before forwarding them to the [`crate::core::hotkeys::HotkeyManager`].

use bitflags::bitflags;
use serde::{Deserialize, Serialize};

bitflags! {
    /// Win32-style modifier bitflags. We use Win32 names so the
    /// platform layer can map directly to `GetAsyncKeyState`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct Modifiers: u8 {
        /// Either Control key.
        const CTRL  = 0b0000_0001;
        /// Either Shift key.
        const SHIFT = 0b0000_0010;
        /// Either Alt key.
        const ALT   = 0b0000_0100;
        /// Windows / super key.
        const SUPER = 0b0000_1000;
        /// Caps Lock (rarely used; included for completeness).
        const CAPS  = 0b0001_0000;
    }
}

/// OS-agnostic key code.
///
/// Only the keys actually bound by JacqueWM are enumerated; future
/// expansions go here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum KeyCode {
    /// Digit key on the main row, 1..=9 (matches US layout).
    Digit(u8),
    /// The letter A..Z (ascii range).
    Letter(char),
    /// Custom VK code for any other key.
    Virtual(u16),
}

impl KeyCode {
    /// Convert a US-main-row digit into its [`KeyCode`] representation.
    pub fn digit(n: u8) -> Self {
        Self::Digit(n)
    }
}

/// Logical hotkey definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hotkey {
    /// Primary key.
    pub key: KeyCode,
    /// Modifier mask — must include at least one modifier to avoid
    /// intercepting ordinary typing.
    pub modifiers: Modifiers,
}

impl Hotkey {
    /// Construct a hotkey. Returns `None` if `modifiers` is empty.
    pub fn new(key: KeyCode, modifiers: Modifiers) -> Option<Self> {
        if modifiers.is_empty() {
            None
        } else {
            Some(Self { key, modifiers })
        }
    }

    /// Returns `true` if this hotkey definition matches the given
    /// platform event.
    pub fn matches(&self, press: &HotkeyPress) -> bool {
        self.key == press.key_code && self.modifiers == press.modifiers
    }
}

/// Platform-neutral hotkey-press event.
///
/// The platform layer produces these by translating raw Win32 LL hook
/// signals. They are the *only* representation of input the dispatcher
/// understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HotkeyPress {
    /// Virtual key code (matches `KeyCode::Virtual`).
    pub key_code: KeyCode,
    /// Active modifiers at the moment of the event.
    pub modifiers: Modifiers,
    /// `true` if this is a held-down auto-repeat event, not an initial
    /// press. The dispatcher can use this to throttle hotkeys.
    pub auto_repeat: bool,
}

impl HotkeyPress {
    /// Construct a press event.
    pub fn new(key_code: KeyCode, modifiers: Modifiers, auto_repeat: bool) -> Self {
        Self {
            key_code,
            modifiers,
            auto_repeat,
        }
    }
}
