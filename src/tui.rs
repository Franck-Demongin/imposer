use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use imposer::config::{DocumentOrientation, ImpositionConfig, SheetOrientation, SheetSize};
use imposer::{ImposerError, Result, impose};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

struct RestoreTerminal;

impl Drop for RestoreTerminal {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Browser,
    Output,
    Options,
    Run,
}

const OPTION_COUNT: usize = 6;

#[derive(Clone)]
struct OptionsState {
    sheet: SheetSize,
    sheet_orientation: SheetOrientation,
    document_orientation: DocumentOrientation,
    bleed: f32,
    safety: f32,
    rotate: bool,
}

impl Default for OptionsState {
    fn default() -> Self {
        Self {
            sheet: SheetSize::A4,
            sheet_orientation: SheetOrientation::Auto,
            document_orientation: DocumentOrientation::Portrait,
            bleed: 2.0,
            safety: 5.0,
            rotate: true,
        }
    }
}

struct Browser {
    cwd: PathBuf,
    entries: Vec<PathBuf>,
    selected: usize,
}

impl Browser {
    fn new() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut browser = Self {
            cwd,
            entries: Vec::new(),
            selected: 0,
        };
        browser.refresh();
        browser
    }

    fn refresh(&mut self) {
        let mut entries = Vec::new();
        if let Ok(read_dir) = fs::read_dir(&self.cwd) {
            for entry in read_dir.flatten() {
                entries.push(entry.path());
            }
        }
        entries.sort_by(|a, b| {
            let a_dir = a.is_dir();
            let b_dir = b.is_dir();
            if a_dir != b_dir {
                return b_dir.cmp(&a_dir);
            }
            a.file_name()
                .unwrap_or_default()
                .cmp(b.file_name().unwrap_or_default())
        });
        self.entries = entries;
        if self.selected >= self.entries.len() {
            self.selected = self.entries.len().saturating_sub(1);
        }
    }

    fn go_up(&mut self) {
        if let Some(parent) = self.cwd.parent().map(Path::to_path_buf) {
            self.cwd = parent;
            self.selected = 0;
            self.refresh();
        }
    }

    fn selected_path(&self) -> Option<&PathBuf> {
        self.entries.get(self.selected)
    }

    fn enter_selected(&mut self) -> Option<PathBuf> {
        if let Some(path) = self.selected_path().cloned() {
            if path.is_dir() {
                self.cwd = path;
                self.selected = 0;
                self.refresh();
                None
            } else {
                Some(path)
            }
        } else {
            None
        }
    }
}

struct App {
    browser: Browser,
    input: Option<PathBuf>,
    output: String,
    options: OptionsState,
    focus: Focus,
    selected_option: usize,
    status: String,
    should_quit: bool,
}

impl App {
    fn new() -> Self {
        Self {
            browser: Browser::new(),
            input: None,
            output: String::new(),
            options: OptionsState::default(),
            focus: Focus::Browser,
            selected_option: 0,
            status: String::from(
                "Bienvenue. ↑↓ naviguer, Entrée ouvrir/sélectionner, Tab changer le focus, q quitter.",
            ),
            should_quit: false,
        }
    }

    fn next_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Browser => Focus::Output,
            Focus::Output => Focus::Options,
            Focus::Options => Focus::Run,
            Focus::Run => Focus::Browser,
        };
    }

    fn prev_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Browser => Focus::Run,
            Focus::Output => Focus::Browser,
            Focus::Options => Focus::Output,
            Focus::Run => Focus::Options,
        };
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }

        if self.focus != Focus::Output
            && (key.code == KeyCode::Char('q') || key.code == KeyCode::Esc)
        {
            self.should_quit = true;
            return;
        }

        match key.code {
            KeyCode::Tab => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.prev_focus();
                } else {
                    self.next_focus();
                }
                return;
            }
            KeyCode::BackTab => {
                self.prev_focus();
                return;
            }
            _ => {}
        }

        match self.focus {
            Focus::Browser => self.handle_browser_key(key),
            Focus::Output => self.handle_output_key(key),
            Focus::Options => self.handle_options_key(key),
            Focus::Run => self.handle_run_key(key),
        }
    }

    fn handle_browser_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => {
                if self.browser.selected > 0 {
                    self.browser.selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.browser.selected + 1 < self.browser.entries.len() {
                    self.browser.selected += 1;
                }
            }
            KeyCode::Enter | KeyCode::Right => {
                if let Some(path) = self.browser.enter_selected() {
                    let ext_ok = path
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|e| e.eq_ignore_ascii_case("pdf"))
                        .unwrap_or(false);
                    if ext_ok {
                        self.input = Some(path.clone());
                        let stem = path
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| String::from("output"));
                        let parent = path.parent().unwrap_or(Path::new("."));
                        self.output = parent
                            .join(format!("{}_imposed.pdf", stem))
                            .display()
                            .to_string();
                        self.status = format!("Source sélectionné : {}", path.display());
                    } else {
                        self.status = String::from("Fichier non PDF ignoré.");
                    }
                }
            }
            KeyCode::Backspace | KeyCode::Left => self.browser.go_up(),
            KeyCode::Char('o') | KeyCode::Char('O') => {
                let proposed = self.browser.cwd.join("output.pdf");
                self.output = proposed.display().to_string();
                self.status = format!("Dossier de sortie : {}", self.browser.cwd.display());
            }
            _ => {}
        }
    }

    fn handle_output_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c) => self.output.push(c),
            KeyCode::Backspace => {
                self.output.pop();
            }
            KeyCode::Enter => {
                self.status = format!("Sortie : {}", self.output);
            }
            _ => {}
        }
    }

    fn handle_options_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => {
                if self.selected_option > 0 {
                    self.selected_option -= 1;
                }
            }
            KeyCode::Down => {
                if self.selected_option + 1 < OPTION_COUNT {
                    self.selected_option += 1;
                }
            }
            KeyCode::Left | KeyCode::Char('-') | KeyCode::Char('[') => {
                self.change_option(-1);
            }
            KeyCode::Right | KeyCode::Char('+') | KeyCode::Char(']') => {
                self.change_option(1);
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.change_option(1);
            }
            _ => {}
        }
    }

    fn handle_run_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter | KeyCode::Char('g') | KeyCode::Char('G') => {
                self.run_imposition();
            }
            _ => {}
        }
    }

    fn change_option(&mut self, delta: i32) {
        match self.selected_option {
            0 => {
                let values = [SheetSize::A4, SheetSize::A3, SheetSize::Sra3];
                let idx = values
                    .iter()
                    .position(|v| *v == self.options.sheet)
                    .unwrap_or(0);
                let next = (idx as i32 + delta).rem_euclid(values.len() as i32) as usize;
                self.options.sheet = values[next];
            }
            1 => {
                let values = [
                    SheetOrientation::Auto,
                    SheetOrientation::Portrait,
                    SheetOrientation::Landscape,
                ];
                let idx = values
                    .iter()
                    .position(|v| *v == self.options.sheet_orientation)
                    .unwrap_or(0);
                let next = (idx as i32 + delta).rem_euclid(values.len() as i32) as usize;
                self.options.sheet_orientation = values[next];
            }
            2 => {
                let values = [
                    DocumentOrientation::Portrait,
                    DocumentOrientation::Landscape,
                ];
                let idx = values
                    .iter()
                    .position(|v| *v == self.options.document_orientation)
                    .unwrap_or(0);
                let next = (idx as i32 + delta).rem_euclid(values.len() as i32) as usize;
                self.options.document_orientation = values[next];
            }
            3 => {
                let next = self.options.bleed + delta as f32 * 0.5;
                self.options.bleed = if next < 0.0 { 0.0 } else { next };
            }
            4 => {
                let next = self.options.safety + delta as f32 * 0.5;
                self.options.safety = if next < 0.0 { 0.0 } else { next };
            }
            5 if delta != 0 => {
                self.options.rotate = !self.options.rotate;
            }
            _ => {}
        }
    }

    fn run_imposition(&mut self) {
        let input_path = match &self.input {
            Some(p) => p.clone(),
            None => {
                self.status = String::from("Aucun fichier source sélectionné.");
                return;
            }
        };
        if self.output.trim().is_empty() {
            self.status = String::from("Chemin de sortie vide.");
            return;
        }

        self.status = String::from("Exécution en cours...");

        let input_bytes = match fs::read(&input_path) {
            Ok(b) => b,
            Err(e) => {
                self.status = format!("Erreur lecture : {e}");
                return;
            }
        };

        let config = ImpositionConfig {
            sheet_size: self.options.sheet,
            sheet_orientation: self.options.sheet_orientation,
            bleed_mm: self.options.bleed,
            safety_margin_mm: self.options.safety,
            document_orientation: self.options.document_orientation,
            rotate_copies: self.options.rotate,
        };

        match impose(&input_bytes, &config) {
            Ok(bytes) => match fs::write(&self.output, &bytes) {
                Ok(()) => {
                    self.status = format!("PDF généré : {} ({} octets)", self.output, bytes.len());
                }
                Err(e) => {
                    self.status = format!("Erreur écriture : {e}");
                }
            },
            Err(e) => {
                self.status = format!("Erreur : {e}");
            }
        }
    }

    fn ui(&mut self, f: &mut ratatui::Frame) {
        let area = f.area();
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(8),
                Constraint::Length(3),
                Constraint::Length(4),
            ])
            .split(area);

        let title = Paragraph::new(Line::from("Imposer - Mode TUI"))
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .block(Block::default().borders(Borders::ALL).title("Titre"));
        f.render_widget(title, rows[0]);

        let main = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(rows[1]);

        let items: Vec<ListItem> = self
            .browser
            .entries
            .iter()
            .map(|p| {
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| p.display().to_string());
                let prefix = if p.is_dir() { "[D] " } else { "    " };
                ListItem::new(format!("{}{}", prefix, name))
            })
            .collect();
        let browser_list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!("Fichiers : {}", self.browser.cwd.display())),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("› ");
        let mut browser_state = ListState::default();
        if self.focus == Focus::Browser {
            browser_state.select(Some(self.browser.selected));
        }
        f.render_stateful_widget(browser_list, main[0], &mut browser_state);

        let option_rows = [
            ("Feuille", sheet_name(self.options.sheet).to_string()),
            (
                "Orientation feuille",
                sheet_orientation_name(self.options.sheet_orientation).to_string(),
            ),
            (
                "Orientation document",
                document_orientation_name(self.options.document_orientation).to_string(),
            ),
            ("Bord perdu (mm)", format!("{:.1}", self.options.bleed)),
            ("Marge sécurité (mm)", format!("{:.1}", self.options.safety)),
            (
                "Rotation des poses",
                if self.options.rotate {
                    String::from("Oui")
                } else {
                    String::from("Non")
                },
            ),
        ];
        let option_items: Vec<ListItem> = option_rows
            .iter()
            .map(|(label, value)| ListItem::new(format!("{:<24} : {}", label, value)))
            .collect();
        let options_list = List::new(option_items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Options (← → modifier)"),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("› ");
        let mut options_state = ListState::default();
        if self.focus == Focus::Options {
            options_state.select(Some(self.selected_option));
        }
        f.render_stateful_widget(options_list, main[1], &mut options_state);

        let output_block = Block::default()
            .borders(Borders::ALL)
            .title("Fichier de sortie");
        let output_style = if self.focus == Focus::Output {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        let output_widget = Paragraph::new(self.output.clone())
            .style(output_style)
            .block(output_block);
        f.render_widget(output_widget, rows[2]);

        let help = "Raccourcis: Tab focus | ↑↓ | Entrée ouvrir/sélect. | ←→ option | o dossier sortie | g exécuter | q quitter";
        let status_text = format!("{}\n{}", self.status, help);
        let status = Paragraph::new(status_text)
            .block(Block::default().borders(Borders::ALL).title("Statut"))
            .wrap(ratatui::widgets::Wrap { trim: true });
        f.render_widget(status, rows[3]);
    }
}

fn sheet_name(v: SheetSize) -> &'static str {
    match v {
        SheetSize::A4 => "A4",
        SheetSize::A3 => "A3",
        SheetSize::Sra3 => "SRA3",
    }
}

fn sheet_orientation_name(v: SheetOrientation) -> &'static str {
    match v {
        SheetOrientation::Auto => "Auto",
        SheetOrientation::Portrait => "Portrait",
        SheetOrientation::Landscape => "Paysage",
    }
}

fn document_orientation_name(v: DocumentOrientation) -> &'static str {
    match v {
        DocumentOrientation::Portrait => "Portrait",
        DocumentOrientation::Landscape => "Paysage",
    }
}

pub fn run() -> Result<()> {
    enable_raw_mode().map_err(|e| ImposerError::Generation(e.to_string()))?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(|e| ImposerError::Generation(e.to_string()))?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal =
        Terminal::new(backend).map_err(|e| ImposerError::Generation(e.to_string()))?;
    let _guard = RestoreTerminal;

    let mut app = App::new();
    loop {
        terminal
            .draw(|f| app.ui(f))
            .map_err(|e| ImposerError::Generation(e.to_string()))?;

        if app.should_quit {
            break;
        }

        if event::poll(std::time::Duration::from_millis(200))
            .map_err(|e| ImposerError::Generation(e.to_string()))?
            && let Event::Key(key) =
                event::read().map_err(|e| ImposerError::Generation(e.to_string()))?
        {
            app.handle_key(key);
        }
    }

    Ok(())
}
