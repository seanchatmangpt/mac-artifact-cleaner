//! mac-artifact-cleaner CLI binary entrypoint.

fn main() -> anyhow::Result<()> {
    mac_artifact_cleaner::nouns::handle_cli()
}
