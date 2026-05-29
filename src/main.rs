//! pentecost CLI binary entrypoint.

fn main() -> anyhow::Result<()> {
    pentecost::nouns::handle_cli()
}
