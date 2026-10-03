/// WARD Server — Portal & Compositor module
///
/// Provides interfaces for:
/// - Native Mutter session (unattended background operation on GNOME Wayland)
/// - XDG Desktop Portal (ScreenCast & RemoteDesktop fallback)

pub mod screencast;
pub mod remote_desktop;
pub mod session;
pub mod mutter;

use std::sync::Arc;
use anyhow::Result;
use self::mutter::MutterRemoteDesktop;
use self::remote_desktop::{ButtonState, RemoteDesktopPortal};

/// Unified input sink supporting both Mutter direct and XDG Desktop Portal backends.
#[derive(Clone)]
pub enum InputSink {
    Mutter(Arc<MutterRemoteDesktop>),
    Portal(Arc<tokio::sync::Mutex<RemoteDesktopPortal>>),
}

impl InputSink {
    pub async fn notify_pointer_motion(&self, dx: f64, dy: f64) -> Result<()> {
        match self {
            InputSink::Mutter(m) => m.notify_pointer_motion(dx, dy).await,
            InputSink::Portal(p) => p.lock().await.notify_pointer_motion(dx, dy).await,
        }
    }

    pub async fn notify_pointer_button(&self, button: i32, state: ButtonState) -> Result<()> {
        match self {
            InputSink::Mutter(m) => m.notify_pointer_button(button, state).await,
            InputSink::Portal(p) => p.lock().await.notify_pointer_button(button, state).await,
        }
    }

    pub async fn notify_pointer_axis(&self, dx: f64, dy: f64) -> Result<()> {
        match self {
            InputSink::Mutter(m) => m.notify_pointer_axis(dx, dy).await,
            InputSink::Portal(p) => p.lock().await.notify_pointer_axis(dx, dy).await,
        }
    }

    pub async fn notify_pointer_axis_discrete(&self, axis: u32, steps: i32) -> Result<()> {
        match self {
            InputSink::Mutter(m) => m.notify_pointer_axis_discrete(axis, steps).await,
            InputSink::Portal(p) => p.lock().await.notify_pointer_axis_discrete(axis, steps).await,
        }
    }

    pub async fn notify_keyboard_keycode(&self, keycode: i32, state: ButtonState) -> Result<()> {
        match self {
            InputSink::Mutter(m) => m.notify_keyboard_keycode(keycode, state).await,
            InputSink::Portal(p) => p.lock().await.notify_keyboard_keycode(keycode, state).await,
        }
    }

    pub async fn notify_keyboard_keysym(&self, keysym: i32, state: ButtonState) -> Result<()> {
        match self {
            InputSink::Mutter(m) => m.notify_keyboard_keysym(keysym, state).await,
            InputSink::Portal(p) => p.lock().await.notify_keyboard_keysym(keysym, state).await,
        }
    }
}
