//! IME (text-input-v3) state + events.
//!
//! Primary consumer: buffr (CJK / dead-key composition in web forms).
//! Gated behind the `text-input` feature.

use wayland_protocols::wp::text_input::zv3::client::zwp_text_input_v3::{
    ContentHint as WlContentHint, ContentPurpose as WlContentPurpose, ZwpTextInputV3,
};

use crate::geometry::Rect;

/// Per-surface IME control surface.
///
/// Obtained via `Toplevel::ime()` / `LayerSurface::ime()`. In
/// text-input-v3 the protocol object is per-seat (only one
/// focused surface at a time), so all `Ime` instances on the same
/// `EventLoop` share the underlying proxy — calls on the
/// non-focused surface's `Ime` are still safe (the compositor
/// ignores them until focus comes back).
pub struct Ime {
    pub(crate) wp: ZwpTextInputV3,
}

impl Ime {
    /// Activate IME for this surface. After `enable`, the compositor
    /// will start sending [`ImeEvent::Preedit`] / [`ImeEvent::Commit`]
    /// events when the user composes characters.
    ///
    /// Typically called when the user focuses a text input field.
    /// Calling on an unfocused surface is a no-op until focus.
    pub fn enable(&self) {
        self.wp.enable();
        self.wp.commit();
    }

    /// Deactivate IME for this surface. Called when leaving a text
    /// field so the IME's candidate window dismisses.
    pub fn disable(&self) {
        self.wp.disable();
        self.wp.commit();
    }

    /// Set the on-screen rectangle the focused text cursor occupies,
    /// in surface-local logical pixels. The IME positions its
    /// candidate popup relative to this rect.
    pub fn set_cursor_rect(&self, rect: Rect) {
        self.wp.set_cursor_rectangle(
            rect.position.x,
            rect.position.y,
            rect.size.width as i32,
            rect.size.height as i32,
        );
        self.wp.commit();
    }

    /// Tell the IME what kind of text the user is entering. Affects
    /// candidate ranking + on-screen keyboard layout on touch devices.
    pub fn set_purpose(&self, purpose: ContentPurpose) {
        // ContentHint is the second arg; pass a sensible default of
        // None when consumer calls set_purpose alone. Consumers
        // wanting both should call set_hint after set_purpose; only
        // the last `set_content_type` request before `commit` wins.
        self.wp
            .set_content_type(WlContentHint::None, purpose.to_protocol());
        self.wp.commit();
    }

    /// Hint flags that affect IME behaviour orthogonally to purpose
    /// (e.g. `MULTILINE` for chat boxes, `SENSITIVE` for passwords).
    pub fn set_hint(&self, hint: ContentHint) {
        self.wp
            .set_content_type(hint.to_protocol(), WlContentPurpose::Normal);
        self.wp.commit();
    }

    /// Set both purpose + hint in a single committed request.
    pub fn set_content_type(&self, purpose: ContentPurpose, hint: ContentHint) {
        self.wp
            .set_content_type(hint.to_protocol(), purpose.to_protocol());
        self.wp.commit();
    }

    /// Inform the IME of the text surrounding the cursor (used for
    /// context-aware composition — e.g. word completion needs to see
    /// the word being typed).
    pub fn set_surrounding_text(&self, text: &str, cursor: u32, anchor: u32) {
        self.wp
            .set_surrounding_text(text.to_owned(), cursor as i32, anchor as i32);
        self.wp.commit();
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

impl ContentPurpose {
    fn to_protocol(self) -> WlContentPurpose {
        match self {
            ContentPurpose::Normal => WlContentPurpose::Normal,
            ContentPurpose::Alpha => WlContentPurpose::Alpha,
            ContentPurpose::Digits => WlContentPurpose::Digits,
            ContentPurpose::Number => WlContentPurpose::Number,
            ContentPurpose::Phone => WlContentPurpose::Phone,
            ContentPurpose::Url => WlContentPurpose::Url,
            ContentPurpose::Email => WlContentPurpose::Email,
            ContentPurpose::Name => WlContentPurpose::Name,
            ContentPurpose::Password => WlContentPurpose::Password,
            ContentPurpose::Pin => WlContentPurpose::Pin,
            ContentPurpose::Date => WlContentPurpose::Date,
            ContentPurpose::Time => WlContentPurpose::Time,
            ContentPurpose::Datetime => WlContentPurpose::Datetime,
            ContentPurpose::Terminal => WlContentPurpose::Terminal,
        }
    }
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

impl ContentHint {
    fn to_protocol(self) -> WlContentHint {
        let mut out = WlContentHint::None;
        if self.contains(ContentHint::COMPLETION) {
            out |= WlContentHint::Completion;
        }
        if self.contains(ContentHint::SPELLCHECK) {
            out |= WlContentHint::Spellcheck;
        }
        if self.contains(ContentHint::AUTO_CAPITAL) {
            out |= WlContentHint::AutoCapitalization;
        }
        if self.contains(ContentHint::LOWERCASE) {
            out |= WlContentHint::Lowercase;
        }
        if self.contains(ContentHint::UPPERCASE) {
            out |= WlContentHint::Uppercase;
        }
        if self.contains(ContentHint::TITLECASE) {
            out |= WlContentHint::Titlecase;
        }
        if self.contains(ContentHint::HIDDEN_TEXT) {
            out |= WlContentHint::HiddenText;
        }
        if self.contains(ContentHint::SENSITIVE_DATA) {
            out |= WlContentHint::SensitiveData;
        }
        if self.contains(ContentHint::LATIN) {
            out |= WlContentHint::Latin;
        }
        if self.contains(ContentHint::MULTILINE) {
            out |= WlContentHint::Multiline;
        }
        out
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
