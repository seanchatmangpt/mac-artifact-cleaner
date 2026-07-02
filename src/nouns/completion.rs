//! Shell completion generation noun.

use std::io;

use clap::{CommandFactory, Subcommand};
use clap_complete::{generate, Shell};

use crate::nouns::Cli;

#[derive(Subcommand, Debug)]
pub enum CompletionAction {
    /// Generate shell completion script
    Generate {
        /// Target shell (bash, zsh, fish, powershell, elvish)
        #[arg(value_enum)]
        shell: Shell,
    },
}

pub fn handle(action: CompletionAction) -> anyhow::Result<()> {
    match action {
        CompletionAction::Generate { shell } => {
            let mut cmd = Cli::command();
            let name = cmd.get_name().to_string();
            generate(shell, &mut cmd, name, &mut io::stdout());
        }
    }
    Ok(())
}
