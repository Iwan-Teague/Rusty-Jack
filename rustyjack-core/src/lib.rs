pub mod cli;
pub mod operations;
pub mod system;
pub mod autopilot;

pub use cli::{Cli, Commands, OutputFormat};
pub use operations::{HandlerResult, dispatch_command};
pub use system::resolve_root;
