pub mod app;
pub mod ask_input;
pub mod audio_player;
pub mod card;
pub mod components;
pub mod format;
pub mod hotkeys;
pub mod layout;
pub mod markdown;
pub mod modal;
pub mod notifications;
pub mod nudge;
pub mod search;
pub mod selector;
pub mod services;
pub mod state;
pub mod terminal;
pub mod text_editor;
pub mod theme;
pub mod todo;
pub mod tool_calls;
pub mod views;

pub use app::{App, HomeTab, InputMode, StatsSubtab, UndoAction, View};
// AnimationClock and NotificationManager are now private services accessed via App methods
// State types are accessed via app.tabs or crate::ui::state::{...}
// Hotkey registry - used for centralized hotkey resolution and help generation
pub use audio_player::{AudioPlaybackState, AudioPlayer};
#[allow(unused_imports)]
pub use hotkeys::{
    get_binding, get_bindings_for_context, resolve_hotkey, resolver as hotkey_resolver,
    HotkeyBinding, HotkeyContext, HotkeyId, HotkeyResolver,
};
pub use modal::ModalState;
pub use terminal::{init as init_terminal, restore as restore_terminal, Tui};
