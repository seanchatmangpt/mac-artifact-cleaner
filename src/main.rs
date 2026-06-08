//! osx-clnr CLI binary entrypoint.

fn main() -> anyhow::Result<()> {
    osx_clnr::nouns::handle_cli()
}
