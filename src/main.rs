use clap::Parser;
use imposer::{ImpositionConfig, impose};
use std::fs;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "imposer",
    version,
    about = "Imposition de PDF sur planches A4/A3/SRA3"
)]
struct Cli {
    #[arg(short, long)]
    input: PathBuf,

    #[arg(short, long)]
    output: PathBuf,

    #[arg(long, value_enum, default_value = "a4")]
    sheet: imposer::config::SheetSize,

    #[arg(long, value_enum, default_value = "auto")]
    sheet_orientation: imposer::config::SheetOrientation,

    #[arg(long, default_value_t = 2.0)]
    bleed: f32,

    #[arg(long, default_value_t = 5.0)]
    safety_margin: f32,

    #[arg(long, value_enum, default_value = "portrait")]
    document_orientation: imposer::config::DocumentOrientation,

    #[arg(long)]
    no_rotate_copies: bool,
}

fn main() {
    let cli = Cli::parse();
    let input = match fs::read(&cli.input) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("erreur lecture entrée : {e}");
            std::process::exit(1);
        }
    };

    let config = ImpositionConfig {
        sheet_size: cli.sheet,
        sheet_orientation: cli.sheet_orientation,
        bleed_mm: cli.bleed,
        safety_margin_mm: cli.safety_margin,
        document_orientation: cli.document_orientation,
        rotate_copies: !cli.no_rotate_copies,
    };

    match impose(&input, &config) {
        Ok(bytes) => {
            if let Err(e) = fs::write(&cli.output, &bytes) {
                eprintln!("erreur écriture sortie : {e}");
                std::process::exit(1);
            }
            println!(
                "PDF généré : {} ({} octets)",
                cli.output.display(),
                bytes.len()
            );
        }
        Err(e) => {
            eprintln!("erreur : {e}");
            std::process::exit(1);
        }
    }
}
