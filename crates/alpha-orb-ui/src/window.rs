//! Window creation for Alpha's desktop client.
//!
//! Creates a simple 800×600 resizable window using `tao`.
//! No transparency, no always-on-top, standard title bar.

use tao::dpi::LogicalSize;
use tao::event_loop::{EventLoop, EventLoopBuilder};
use tao::window::{Window, WindowBuilder};
use tracing::info;

use crate::IpcEvent;

/// Create the main Alpha window.
///
/// Returns the window and its associated event loop.
/// The event loop must be run on the main thread.
pub fn create_window() -> (EventLoop<IpcEvent>, Window) {
    let event_loop = EventLoopBuilder::<IpcEvent>::with_user_event().build();

    let window = WindowBuilder::new()
        .with_title("Alpha")
        .with_inner_size(LogicalSize::new(800.0_f64, 600.0_f64))
        .with_resizable(true)
        .build(&event_loop)
        .expect("Failed to create window");

    info!("Window created: 800×600, resizable");

    (event_loop, window)
}
