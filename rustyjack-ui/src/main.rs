mod app;
mod config;
mod core;
mod display;
mod input;
mod menu;
mod stats;

use anyhow::Result;

use app::App;

fn main() -> Result<()> {
    App::new()?.run()
}
