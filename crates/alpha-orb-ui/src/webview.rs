//! WebView creation for Alpha's desktop client.
//!
//! Loads a self-contained HTML page into a `wry` WebView.
//! Registers an IPC handler that routes `window.ipc.postMessage()`
//! calls from JavaScript through the `ConversationBridge` to the
//! `ConversationService` streaming pipeline.

use std::sync::Arc;

use alpha_conversation::ModelProvider;
use tao::event_loop::EventLoopProxy;
use tao::window::Window;
use tracing::{debug, info};
use wry::{WebView, WebViewBuilder};

use crate::ipc::ConversationBridge;
use crate::IpcEvent;

/// The complete HTML content, embedded at compile time.
///
/// This embeds `assets/index.html` directly into the binary so no
/// file-system access or custom protocol is needed at runtime.
const INDEX_HTML: &str = include_str!("assets/index.html");

/// Create a WebView attached to the given window.
///
/// Loads the embedded HTML with inline CSS and JavaScript.
/// Registers an IPC handler that dispatches messages through
/// the `ConversationBridge` for streaming conversation support.
/// DevTools are enabled in debug builds.
pub fn create_webview<M: ModelProvider + 'static>(
    window: &Window,
    proxy: EventLoopProxy<IpcEvent>,
    bridge: Arc<ConversationBridge<M>>,
) -> WebView {
    let builder = WebViewBuilder::new()
        .with_html(INDEX_HTML)
        .with_devtools(cfg!(debug_assertions))
        .with_ipc_handler(move |request| {
            let raw = request.body();
            debug!(raw = %raw, "IPC message received from JS");
            bridge.handle_message(raw, &proxy);
        });

    let webview = builder
        .build(window)
        .expect("Failed to create WebView");

    info!("WebView created with ConversationBridge IPC handler");

    webview
}
