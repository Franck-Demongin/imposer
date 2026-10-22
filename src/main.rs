use clap::Parser;
use imposer::{ImposeOptions, SourcePage, generate, sources_from_bytes};
use std::fs;
use std::path::PathBuf;

mod tui;

#[derive(Parser, Debug)]
#[command(
    name = "imposer",
    version,
    about = "Imposition de PDF sur planches A4/A3/SRA3"
)]
struct Cli {
    #[arg(short, long)]
    input: Vec<PathBuf>,

    #[arg(short, long)]
    output: Option<PathBuf>,

    #[arg(long, value_enum, default_value = "a4")]
    sheet: imposer::config::SheetSize,

    #[arg(long, value_enum, default_value = "auto")]
    sheet_orientation: imposer::config::SheetOrientation,

    #[arg(long, default_value_t = 2.0)]
    bleed: f32,

    #[arg(long, default_value_t = 5.0)]
    safety_margin: f32,

    #[arg(long)]
    rotate_verso: bool,

    #[arg(long)]
    no_rotate_copies: bool,

    #[arg(long)]
    tui: bool,
}

fn main() {
    let cli = Cli::parse();

    if cli.tui || cli.input.is_empty() || cli.output.is_none() {
        if let Err(e) = tui::run() {
            eprintln!("erreur TUI : {e}");
            std::process::exit(1);
        }
        return;
    }

    let output = cli.output.unwrap();

    // Utilise le dossier des sources si l'option `--input` est passée ;
    // la lecture est conservée telle que précédemment.
    let mut inputs: Vec<Vec<u8>> = Vec::new();
    for p in &cli.input {
        match fs::read(p) {
            Ok(b) => inputs.push(b),
            Err(e) => {
                eprintln!("erreur lecture entrée {} : {e}", p.display());
                std::process::exit(1);
            }
        }
    }

    let sources = match sources_from_bytes(&inputs) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("erreur : {e}");
            std::process::exit(1);
        }
    };

    let color_report: Vec<String> = sources.iter().map(SourcePage::color_label).collect();
    println!("Profils couleur détectés : {}", color_report.join(" ; "));
    let config = ImposeOptions {
        sheet_size: cli.sheet,
        sheet_orientation: cli.sheet_orientation,
        bleed_mm: cli.bleed,
        safety_margin_mm: cli.safety_margin,
        rotate_verso: cli.rotate_verso,
        rotate_copies: !cli.no_rotate_copies,
    };

    match generate(&sources, &config, output) {
        Ok(report) => {
            println!(
                "PDF généré : {} ({} octets)",
                report.output_path.display(),
                report.output_size
            );
        }
        Err(e) => {
            eprintln!("erreur : {e}");
            std::process::exit(1);
        }
    }
}
