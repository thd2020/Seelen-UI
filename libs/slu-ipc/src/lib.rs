pub mod app;
pub mod commands;
pub mod common;
pub mod error;
pub mod messages;
pub mod notification_center;
pub mod security;
pub mod service;
pub mod taskbar_guard;

// Re-export main types for convenience
pub use app::AppIpc;
pub use common::IPC;
pub use service::ServiceIpc;
