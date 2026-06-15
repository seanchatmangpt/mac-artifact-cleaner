//! osx-clnr CLI binary entrypoint.

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "github" {
        if let Err(e) = clap_noun_verb::run() {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
        Ok(())
    } else {
        osx_clnr::nouns::handle_cli()
    }
}
