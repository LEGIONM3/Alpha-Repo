//! # alpha-orb-ui
//!
//! Desktop UI for Project Alpha.
//!
//! Provides a WebView2-based chat window using `tao` for window management
//! and `wry` for web content rendering. This crate embeds the complete
//! HTML/CSS/JS frontend at compile time.
//!
//! ## Architecture
//!
//! ```text
//! alpha-orb-ui
//! ├── window   — tao window creation (800×600, resizable)
//! ├── webview  — wry WebView with embedded HTML + IPC handler
//! ├── ipc      — Rust ↔ JS protocol + ConversationBridge
//! └── assets/  — HTML, CSS, JS (compiled into binary via include_str!)
//! ```
//!
//! ## Current State (Sprint 4A Phase 5)
//!
//! - Full ConversationService integration via streaming IPC.
//! - Pipeline: User → WebView → IPC → ConversationService → ModelRouter → Stream → WebView → User.

pub mod ipc;
pub mod webview;
pub mod window;

use std::sync::Arc;

use alpha_conversation::types::ModelProvider;
use alpha_conversation::ConversationService;
use tao::event::{Event, WindowEvent};
use tao::event_loop::ControlFlow;
use tracing::{info, warn};

/// The embedded HTML content for compile-time verification in tests.
pub const INDEX_HTML: &str = include_str!("assets/index.html");

/// The embedded CSS content for compile-time verification in tests.
pub const STYLE_CSS: &str = include_str!("assets/style.css");

/// The embedded JS content for compile-time verification in tests.
pub const APP_JS: &str = include_str!("assets/app.js");

/// Custom event type for IPC communication through the tao event loop.
///
/// The wry IPC handler cannot directly call `webview.evaluate_script()`,
/// so it sends responses through the event loop proxy as custom events.
/// The main event loop then dispatches them to the WebView.
#[derive(Debug, Clone)]
pub enum IpcEvent {
    /// A JavaScript string to evaluate in the WebView.
    EvaluateScript(String),
}

/// Launch the Alpha desktop UI with a connected `ConversationService`.
///
/// This function blocks the calling thread — it runs the `tao` event loop
/// which must execute on the main thread. It does not return under normal
/// operation; the process exits when the window is closed.
///
/// The `runtime_handle` should be the handle to the existing tokio runtime
/// that the `ConversationService` and its dependencies were created on.
///
/// # Panics
///
/// Panics if window or WebView creation fails (e.g. WebView2 runtime
/// not installed on Windows).
pub fn launch<M: ModelProvider + 'static>(
    service: Arc<ConversationService<M>>,
    runtime_handle: tokio::runtime::Handle,
) {
    let (event_loop, window) = window::create_window();
    let proxy = event_loop.create_proxy();

    // Create the conversation bridge.
    let bridge = Arc::new(ipc::ConversationBridge::new(service, runtime_handle));

    // Must hold the webview reference for its lifetime.
    let webview = webview::create_webview(&window, proxy, bridge);

    info!("Alpha UI launched with ConversationService — entering event loop");

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                info!("Window close requested — exiting");
                *control_flow = ControlFlow::Exit;
            }
            Event::UserEvent(IpcEvent::EvaluateScript(js)) => {
                if let Err(e) = webview.evaluate_script(&js) {
                    warn!(error = %e, "Failed to evaluate IPC script in WebView");
                }
            }
            _ => {}
        }
    });
}

#[cfg(test)]
mod tests;
