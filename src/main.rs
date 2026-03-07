#![windows_subsystem = "windows"]
mod app;
mod tools;

fn main() -> anyhow::Result<()> {
    app::run()
}
