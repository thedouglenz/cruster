//! Cruster terminal UI.

pub mod action;
pub mod action_shipped;
pub mod actions;
pub mod app;
pub mod command;
pub mod history;
pub mod kubectl;
pub mod overlay;
pub mod overlays;
pub mod safety;
pub mod theme;
pub mod view;
pub mod views;
pub mod workflows;

pub use action::{Action, ActionRegistry};
pub use app::App;
pub use view::ResourceView;
