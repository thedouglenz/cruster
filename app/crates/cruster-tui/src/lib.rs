//! Cruster terminal UI.

pub mod action;
pub mod action_shipped;
pub mod actions;
pub mod app;
pub mod command;
pub mod kubectl;
pub mod overlay;
pub mod overlays;
pub mod safety;
pub mod view;
pub mod views;

pub use action::{Action, ActionRegistry};
pub use app::App;
pub use view::ResourceView;
