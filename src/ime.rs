//! IME (text-input-v3) state + events.
//!
//! Lands in #15 (Phase 6). Gated behind the `text-input` feature.
//! Primary consumer: buffr (CJK / dead-key composition in web forms).

use crate::geometry::Rect;

/// Per-surface IME control surface.
///
/// Obtained via `Toplevel::ime()` / `Subsurface::ime()` (added in #15).
/// IME state has its own lifecycle (enable / disable / preedit /
/// commit / cursor-rect updates) that doesn't slot cleanly into a
/// single event stream — the imperative API here lets consumers
/// drive activation in response to focus changes and form-field
/// transitions.
pub struct Ime {
    pub(crate) _private: (),
}

impl Ime {
    /// Activate IME for this surface. After `enable`, the compositor
    /// will start sending [`ImeEvent::Preedit`] / [`ImeEvent::Commit`]
    /// events when the user composes characters.
    ///
    /// Typically called when the user focuses a text input field.
    pub fn enable(&self) {
        unimplemented!("#15: zwp_text_input_v3.enable + commit")
    }

    /// Deactivate IME for this surface. Called when leaving a text
    /// field so the IME's candidate window dismisses.
    pub fn disable(&self) {
        unimplemented!("#15: zwp_text_input_v3.disable + commit")
    }

    /// Set the on-screen rectangle the focused text cursor occupies,
    /// in surface-local logical pixels. The IME positions its
    /// candidate popup relative to this rect.
    pub fn set_cursor_rect(&self, _rect: Rect) {
        unimplemented!("#15: zwp_text_input_v3.set_cursor_rectangle")
    }

    /// Tell the IME what kind of text the user is entering. Affects
    /// candidate ranking + on-screen keyboard layout on touch devices.
    pub fn set_purpose(&self, _purpose: ContentPurpose) {
        unimplemented!("#15: zwp_text_input_v3.set_content_type")
    }

    /// Hint flags that affect IME behaviour orthogonally to purpose
    /// (e.g. `MULTILINE` for chat boxes, `SENSITIVE` for passwords).
    pub fn set_hint(&self, _hint: ContentHint) {
        unimplemented!("#15: zwp_text_input_v3.set_content_type")
    }

    /// Inform the IME of the text surrounding the cursor (used for
    /// context-aware composition — e.g. word completion needs to see
    /// the word being typed).
    pub fn set_surrounding_text(&self, _text: &str, _cursor: u32, _anchor: u32) {
        unimplemented!("#15: zwp_text_input_v3.set_surrounding_text")
    }
}

/// Semantic purpose of a text input. Maps 1:1 to
/// `zwp_text_input_v3.content_purpose` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ContentPurpose {
    /// Default — generic text.
    Normal,
    /// Single-character input (numeric keypads etc.).
    Alpha,
    /// Numeric.
    Digits,
    /// Numeric with sign + decimal.
    Number,
    /// Phone number.
    Phone,
    /// URL.
    Url,
    /// Email address.
    Email,
    /// Person's name.
    Name,
    /// Password (typically hides on-screen keyboard suggestions).
    Password,
    /// PIN — numeric password.
    Pin,
    /// Date.
    Date,
    /// Time.
    Time,
    /// Date + time.
    Datetime,
    /// Terminal / shell command line.
    Terminal,
}

bitflags::bitflags! {
    /// Hint flags. Maps to `zwp_text_input_v3.content_hint`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct ContentHint: u32 {
        /// Suggest completions.
        const COMPLETION    = 1 << 0;
        /// Auto-correct.
        const SPELLCHECK    = 1 << 1;
        /// Auto-capitalize.
        const AUTO_CAPITAL  = 1 << 2;
        /// Lowercase only.
        const LOWERCASE     = 1 << 3;
        /// Uppercase only.
        const UPPERCASE     = 1 << 4;
        /// Title-case (first letter of each word).
        const TITLECASE     = 1 << 5;
        /// Hide visible feedback (passwords).
        const HIDDEN_TEXT   = 1 << 6;
        /// Sensitive (don't expose to clipboard managers / dictation).
        const SENSITIVE_DATA = 1 << 7;
        /// Latin script only.
        const LATIN         = 1 << 8;
        /// Allow multiple lines (Enter inserts newline rather than submit).
        const MULTILINE     = 1 << 9;
    }
}

/// IME event dispatched as part of [`crate::WindowEvent::Ime`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum ImeEvent {
    /// Composition string visible to the user but not yet committed.
    /// Replaces any previous preedit. `cursor` is the byte offset of
    /// the caret inside `text` (used to position the composition
    /// cursor in the text field).
    Preedit {
        /// The current composition string.
        text: String,
        /// Caret byte offset inside `text`, or `None` to hide caret.
        cursor: Option<u32>,
    },

    /// Final committed string. Consumer should insert this into the
    /// text field at the cursor.
    Commit(String),

    /// IME wants the consumer to delete `before_bytes` UTF-8 bytes
    /// before the cursor + `after_bytes` after, then commit.
    /// Used by IMEs that rewrite already-typed text (e.g. converting
    /// hiragana to kanji retroactively).
    DeleteSurroundingText {
        /// Bytes to delete before the cursor.
        before_bytes: u32,
        /// Bytes to delete after the cursor.
        after_bytes: u32,
    },
}
