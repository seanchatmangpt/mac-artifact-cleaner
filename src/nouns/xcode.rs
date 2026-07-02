//! Xcode simulator and DerivedData noun.

use crate::integration::progress::human_bytes as format_bytes;
use clap::Subcommand;

#[derive(Subcommand, Debug)]
pub enum XcodeAction {
    /// Scan CoreSimulator runtimes for size and availability
    Simulators,
    /// Show DerivedData size
    DerivedData,
}

pub fn handle(action: XcodeAction) -> anyhow::Result<()> {
    match action {
        XcodeAction::Simulators => handle_simulators(),
        XcodeAction::DerivedData => handle_derived_data(),
    }
}

fn handle_simulators() -> anyhow::Result<()> {
    use crate::integration::xcode::list_simulator_runtimes;

    let result = list_simulator_runtimes()?;

    if result.runtimes.is_empty() {
        println!("No simulator runtimes found.");
        return Ok(());
    }

    println!("Simulator Runtimes:");
    println!("{:-<60}", "");
    for rt in &result.runtimes {
        let avail = if rt.is_available {
            "available"
        } else {
            "UNAVAILABLE - candidate for deletion"
        };
        let size = format_bytes(rt.size_bytes);
        println!("  {} ({}) — {} [{}]", rt.name, rt.version, size, avail);
        if let Some(p) = &rt.path {
            println!("    Path: {}", p.display());
        }
    }
    println!("{:-<60}", "");
    println!("Total:       {}", format_bytes(result.total_bytes));
    println!("Unavailable: {}", format_bytes(result.unavailable_bytes));

    Ok(())
}

fn handle_derived_data() -> anyhow::Result<()> {
    use crate::integration::xcode::du_path;

    let derived_data = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~"))
        .join("Library/Developer/Xcode/DerivedData");

    if !derived_data.exists() {
        println!(
            "DerivedData directory not found: {}",
            derived_data.display()
        );
        return Ok(());
    }

    let size = du_path(&derived_data);
    println!("DerivedData: {}", derived_data.display());
    println!("Size:        {}", format_bytes(size));

    Ok(())
}
