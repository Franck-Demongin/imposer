use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use imposer::config::{DocumentOrientation, ImpositionConfig, SheetOrientation, SheetSize};
use imposer::{ImposerError, Result, impose_sources, sources_from_bytes};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

struct RestoreTerminal;

impl Drop for RestoreTerminal {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Browser,
    OutputDir,
    OutputName,
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

struct PendingRun {
    destination: PathBuf,
    source_paths: Vec<PathBuf>,
    sheet: SheetSize,
    sheet_orientation: SheetOrientation,
    document_orientation: DocumentOrientation,
    bleed: f32,
    safety: f32,
    rotate: bool,
}

struct App {
    browser: Browser,
    sources: Vec<PathBuf>,
    output_dir: String,
    output_dir_cursor: usize,
    output_name: String,
    output_name_cursor: usize,
    options: OptionsState,
    focus: Focus,
    selected_option: usize,
    status: String,
    should_quit: bool,
    run_button_rect: Option<ratatui::layout::Rect>,
    busy: bool,
    busy_frame: usize,
    busy_rx: Option<mpsc::Receiver<String>>,
    create_dir_popup: Option<PathBuf>,
    pending_generation: Option<PendingRun>,
}

fn insert_char_at(text: &mut String, cursor: &mut usize, c: char) {
    let chars: Vec<char> = text.chars().collect();
    let pos = (*cursor).min(chars.len());
    let mut next = String::with_capacity(text.len() + c.len_utf8());
    next.extend(chars[..pos].iter().copied());
    next.push(c);
    next.extend(chars[pos..].iter().copied());
    *text = next;
    *cursor = pos + 1;
}

fn backspace_at(text: &mut String, cursor: &mut usize) {
    let mut chars: Vec<char> = text.chars().collect();
    let pos = (*cursor).min(chars.len());
    if pos > 0 {
        chars.remove(pos - 1);
        *cursor = pos - 1;
        *text = chars.into_iter().collect();
    }
}

fn delete_at(text: &mut String, cursor: &mut usize) {
    let mut chars: Vec<char> = text.chars().collect();
    let pos = (*cursor).min(chars.len());
    if pos < chars.len() {
        chars.remove(pos);
        *text = chars.into_iter().collect();
    }
}

impl App {
    fn new() -> Self {
        Self {
            browser: Browser::new(),
            sources: Vec::new(),
            output_dir: String::new(),
            output_dir_cursor: 0,
            output_name: String::new(),
            output_name_cursor: 0,
            options: OptionsState::default(),
            focus: Focus::Browser,
            selected_option: 0,
            status: String::from(
                "Bienvenue. ↑↓ naviguer, Entrée ouvrir/sélectionner, Tab changer le focus, q quitter.",
            ),
            should_quit: false,
            run_button_rect: None,
            busy: false,
            busy_frame: 0,
            busy_rx: None,
            create_dir_popup: None,
            pending_generation: None,
        }
    }

    fn next_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Browser => Focus::OutputDir,
            Focus::OutputDir => Focus::OutputName,
            Focus::OutputName => Focus::Options,
            Focus::Options => Focus::Run,
            Focus::Run => Focus::Browser,
        };
    }

    fn prev_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Browser => Focus::Run,
            Focus::OutputDir => Focus::Browser,
            Focus::OutputName => Focus::OutputDir,
            Focus::Options => Focus::OutputName,
            Focus::Run => Focus::Options,
        };
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }

        // Modale de création de dossier : intercepte toutes les touches.
        if self.create_dir_popup.is_some() {
            match key.code {
                KeyCode::Char('o')
                | KeyCode::Char('O')
                | KeyCode::Char('y')
                | KeyCode::Char('Y')
                | KeyCode::Enter => {
                    if let (Some(dir), Some(run)) =
                        (self.create_dir_popup.take(), self.pending_generation.take())
                    {
                        match fs::create_dir_all(&dir) {
                            Ok(()) => self.execute_generation(run),
                            Err(e) => {
                                self.status = format!(
                                    "Impossible de créer le dossier {} : {e}",
                                    dir.display()
                                );
                            }
                        }
                    }
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc | KeyCode::Char('q') => {
                    self.create_dir_popup = None;
                    self.pending_generation = None;
                    self.status =
                        String::from("Génération annulée : dossier de sortie inexistant.");
                }
                _ => {}
            }
            return;
        }

        if !matches!(self.focus, Focus::OutputDir | Focus::OutputName)
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

        if key.code == KeyCode::Delete
            && !matches!(self.focus, Focus::OutputDir | Focus::OutputName)
        {
            self.sources.pop();
            self.status = if self.sources.is_empty() {
                "Aucune source sélectionnée (max. 2).".to_string()
            } else {
                format!(
                    "Dernière source retirée. {} source(s) restante(s).",
                    self.sources.len()
                )
            };
            return;
        }

        match self.focus {
            Focus::Browser => self.handle_browser_key(key),
            Focus::OutputDir => self.handle_output_dir_key(key),
            Focus::OutputName => self.handle_output_name_key(key),
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
                        .map(|e| {
                            matches!(
                                e.to_ascii_lowercase().as_str(),
                                "pdf" | "png" | "jpg" | "jpeg" | "psd"
                            )
                        })
                        .unwrap_or(false);
                    if ext_ok {
                        let selected_path = path.clone();
                        if self.sources.contains(&selected_path) {
                            self.status = "Source déjà sélectionnée.".to_string();
                        } else if self.sources.len() >= 2 {
                            self.status =
                                "Maximum de 2 sources atteint. Utilisez Suppr pour retirer la dernière."
                                    .to_string();
                        } else {
                            if self.output_dir.is_empty() {
                                let parent = selected_path.parent().unwrap_or(Path::new("."));
                                self.output_dir = parent.display().to_string();
                                self.output_dir_cursor = self.output_dir.chars().count();
                            }
                            if self.output_name.is_empty() {
                                let stem = selected_path
                                    .file_stem()
                                    .map(|s| s.to_string_lossy().to_string())
                                    .unwrap_or_else(|| String::from("output"));
                                self.output_name = format!("{}_imposed.pdf", stem);
                                self.output_name_cursor = self.output_name.chars().count();
                            }
                            self.sources.push(selected_path.clone());
                            self.status = format!(
                                "Source {} sélectionnée : {}",
                                self.sources.len(),
                                selected_path.display()
                            );
                        }
                    } else {
                        self.status = String::from("Fichier non PDF ignoré.");
                    }
                }
            }
            KeyCode::Backspace | KeyCode::Left => self.browser.go_up(),
            KeyCode::Char('o') | KeyCode::Char('O') => {
                self.output_dir = self.browser.cwd.display().to_string();
                self.output_dir_cursor = self.output_dir.chars().count();
                if self.output_name.is_empty() {
                    self.output_name = String::from("output.pdf");
                    self.output_name_cursor = self.output_name.chars().count();
                }
                self.status = format!("Dossier de sortie : {}", self.browser.cwd.display());
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                let target = self
                    .browser
                    .entries
                    .get(self.browser.selected)
                    .cloned()
                    .unwrap_or_else(|| self.browser.cwd.clone());
                let directory = if target.is_dir() {
                    target
                } else {
                    target.parent().unwrap_or(&self.browser.cwd).to_path_buf()
                };
                self.output_dir = directory.display().to_string();
                self.output_dir_cursor = self.output_dir.chars().count();
                if self.output_name.is_empty() {
                    self.output_name = String::from("output.pdf");
                    self.output_name_cursor = self.output_name.chars().count();
                }
                self.status = format!("Dossier de sortie : {}", directory.display());
            }
            _ => {}
        }
    }

    fn handle_output_dir_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c) if !c.is_control() => {
                insert_char_at(&mut self.output_dir, &mut self.output_dir_cursor, c);
            }
            KeyCode::Backspace => backspace_at(&mut self.output_dir, &mut self.output_dir_cursor),
            KeyCode::Delete => delete_at(&mut self.output_dir, &mut self.output_dir_cursor),
            KeyCode::Left => {
                if self.output_dir_cursor > 0 {
                    self.output_dir_cursor -= 1;
                }
            }
            KeyCode::Right => {
                let count = self.output_dir.chars().count();
                if self.output_dir_cursor < count {
                    self.output_dir_cursor += 1;
                }
            }
            KeyCode::Home => self.output_dir_cursor = 0,
            KeyCode::End => self.output_dir_cursor = self.output_dir.chars().count(),
            KeyCode::Enter => match self.output_path() {
                Some(path) => self.status = format!("Sortie : {}", path.display()),
                None => self.status = String::from("Dossier ou nom de sortie invalide."),
            },
            _ => {}
        }
    }

    fn handle_output_name_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c) if !c.is_control() => {
                insert_char_at(&mut self.output_name, &mut self.output_name_cursor, c);
            }
            KeyCode::Backspace => backspace_at(&mut self.output_name, &mut self.output_name_cursor),
            KeyCode::Delete => delete_at(&mut self.output_name, &mut self.output_name_cursor),
            KeyCode::Left => {
                if self.output_name_cursor > 0 {
                    self.output_name_cursor -= 1;
                }
            }
            KeyCode::Right => {
                let count = self.output_name.chars().count();
                if self.output_name_cursor < count {
                    self.output_name_cursor += 1;
                }
            }
            KeyCode::Home => self.output_name_cursor = 0,
            KeyCode::End => self.output_name_cursor = self.output_name.chars().count(),
            KeyCode::Enter => match self.output_path() {
                Some(path) => self.status = format!("Sortie : {}", path.display()),
                None => self.status = String::from("Dossier ou nom de sortie invalide."),
            },
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
            KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Char('g') | KeyCode::Char('G') => {
                self.run_imposition();
            }
            _ => {}
        }
    }

    fn poll_busy(&mut self) {
        if !self.busy {
            return;
        }
        if let Some(rx) = &self.busy_rx
            && let Ok(status) = rx.try_recv()
        {
            self.status = status;
            self.busy = false;
            self.busy_rx = None;
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

    fn output_path(&self) -> Option<PathBuf> {
        let directory = self.output_dir.trim();
        let raw_name = self.output_name.trim();
        if directory.is_empty() || raw_name.is_empty() {
            return None;
        }
        let name_path = Path::new(raw_name);
        let components: Vec<_> = name_path.components().collect();
        if name_path.is_absolute()
            || components.is_empty()
            || components.len() != 1
            || raw_name == "."
            || raw_name == ".."
        {
            return None;
        }
        let mut name = raw_name.to_string();
        if !raw_name.to_ascii_lowercase().ends_with(".pdf") {
            name.push_str(".pdf");
        }
        Some(PathBuf::from(directory).join(name))
    }

    fn execute_generation(&mut self, run: PendingRun) {
        self.status = String::from("Exécution en cours...");

        let destination = run.destination;
        let source_paths = run.source_paths;
        let sheet = run.sheet;
        let sheet_orientation = run.sheet_orientation;
        let document_orientation = run.document_orientation;
        let bleed = run.bleed;
        let safety = run.safety;
        let rotate = run.rotate;

        let (tx, rx) = mpsc::channel();
        self.busy_rx = Some(rx);
        self.busy = true;
        self.busy_frame = 0;

        std::thread::spawn(move || {
            let status = run_imposition_work(
                source_paths,
                destination,
                sheet,
                sheet_orientation,
                document_orientation,
                bleed,
                safety,
                rotate,
            );
            let _ = tx.send(status);
        });
    }

    fn run_imposition(&mut self) {
        if self.busy {
            self.status = String::from("Génération déjà en cours...");
            return;
        }
        if self.sources.is_empty() {
            self.status = String::from("Aucun fichier source sélectionné.");
            return;
        }
        let destination = match self.output_path() {
            Some(path) => path,
            None => {
                self.status = String::from("Dossier ou nom de sortie invalide.");
                return;
            }
        };

        let run = PendingRun {
            destination: destination.clone(),
            source_paths: self.sources.clone(),
            sheet: self.options.sheet,
            sheet_orientation: self.options.sheet_orientation,
            document_orientation: self.options.document_orientation,
            bleed: self.options.bleed,
            safety: self.options.safety,
            rotate: self.options.rotate,
        };

        if let Some(parent) = destination.parent()
            && !parent.as_os_str().is_empty()
        {
            if parent.is_dir() {
                // Dossier présent : on génère directement.
            } else if !parent.exists() {
                // Dossier absent : on propose de le créer.
                self.pending_generation = Some(run);
                self.create_dir_popup = Some(parent.to_path_buf());
                self.status = format!(
                    "Dossier de sortie introuvable : {} (créer ? o/n)",
                    parent.display()
                );
                return;
            } else {
                self.status = format!("Dossier de sortie introuvable : {}", parent.display());
                return;
            }
        }

        self.execute_generation(run);
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            return;
        }
        if self.busy || self.create_dir_popup.is_some() {
            return;
        }
        if let Some(rect) = self.run_button_rect
            && mouse.column >= rect.x
            && mouse.column < rect.x + rect.width
            && mouse.row >= rect.y
            && mouse.row < rect.y + rect.height
        {
            self.focus = Focus::Run;
            self.run_imposition();
        }
    }

    fn ui(&mut self, f: &mut ratatui::Frame) {
        let area = f.area();
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(7),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(5),
            ])
            .split(area);

        let title = Paragraph::new(Line::from("Imposer - Mode TUI"))
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!("v{}", env!("CARGO_PKG_VERSION"))),
            );
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

        let sources_text = if self.sources.is_empty() {
            "Aucune source sélectionnée (max. 2).".to_string()
        } else {
            self.sources
                .iter()
                .enumerate()
                .map(|(i, path)| format!("{}. {}", i + 1, path.display()))
                .collect::<Vec<_>>()
                .join(" | ")
        };
        let sources_widget = Paragraph::new(sources_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Sources (max. 2)"),
            )
            .wrap(ratatui::widgets::Wrap { trim: true });
        f.render_widget(sources_widget, rows[2]);

        render_text_field(
            f,
            rows[3],
            "Dossier de sortie",
            &self.output_dir,
            self.output_dir_cursor,
            self.focus == Focus::OutputDir,
        );

        render_text_field(
            f,
            rows[4],
            "Nom de sortie",
            &self.output_name,
            self.output_name_cursor,
            self.focus == Focus::OutputName,
        );

        let button_style = if self.focus == Focus::Run {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Green)
        };
        f.render_widget(Clear, rows[5]);
        let button_row = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(0),
                Constraint::Length(18),
                Constraint::Min(0),
            ])
            .split(rows[5]);
        let button_area = button_row[1];
        // Bouton focalisé : bloc plein (sans bordure) pour éviter le double
        // cadre vert autour de la bordure grise ; le fond vert remplit toute la
        // zone du bouton et le texte est centré horizontalement et verticalement.
        let button = if self.focus == Focus::Run {
            Paragraph::new("\nGénérer")
                .style(button_style)
                .alignment(Alignment::Center)
        } else {
            Paragraph::new("Générer")
                .style(button_style)
                .alignment(Alignment::Center)
                .block(Block::default().borders(Borders::ALL))
        };
        f.render_widget(button, button_area);
        self.run_button_rect = Some(button_area);

        // Pop-up de confirmation de création du dossier de sortie.
        if let Some(dir) = &self.create_dir_popup {
            let popup_area = centered_rect(60, 9, f.area());
            f.render_widget(Clear, popup_area);
            let message = format!(
                "Le dossier de sortie n'existe pas :\n{}\n\nLe créer ? (o)ui / (n)on",
                dir.display()
            );
            let popup = Paragraph::new(message)
                .wrap(ratatui::widgets::Wrap { trim: true })
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Créer le dossier ?"),
                );
            f.render_widget(popup, popup_area);
        }

        let help = "Raccourcis: Tab focus | ↑↓ | Entrée ajoute (max. 2) | textes: ←→ déplacement, Début/Fin, RetArr/Del | o dossier courant | d dossier sélectionné | Suppr source | g/Entrée/Tab+Entrée générer | clic souris sur Générer | q quitter";
        let displayed_status = if self.busy {
            let spinner = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠏"];
            format!(
                "{}  {}",
                spinner[self.busy_frame % spinner.len()],
                self.status
            )
        } else {
            self.status.clone()
        };
        if self.busy {
            self.busy_frame = self.busy_frame.wrapping_add(1);
        }
        let status_text = format!("{}\n{}", displayed_status, help);
        let status = Paragraph::new(status_text)
            .block(Block::default().borders(Borders::ALL).title("Statut"))
            .wrap(ratatui::widgets::Wrap { trim: true });
        f.render_widget(status, rows[6]);
    }
}

fn centered_rect(
    percent_x: u16,
    height: u16,
    area: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
    let height = height.min(area.height);
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1]);
    horizontal[1]
}

fn render_text_field(
    f: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    title: &str,
    value: &str,
    cursor: usize,
    focused: bool,
) {
    let chars: Vec<char> = value.chars().collect();
    let marker = if focused { "│" } else { " " };
    let cursor = cursor.min(chars.len());
    let mut text = String::new();
    for idx in 0..=chars.len() {
        if idx == cursor {
            text.push_str(marker);
        }
        if idx < chars.len() {
            text.push(chars[idx]);
        }
    }
    if text.is_empty() {
        text.push_str(marker);
    }
    let style = if focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let widget = Paragraph::new(text).style(style).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title.to_string()),
    );
    f.render_widget(widget, area);
}

#[allow(clippy::too_many_arguments)]
fn run_imposition_work(
    source_paths: Vec<PathBuf>,
    destination: PathBuf,
    sheet: SheetSize,
    sheet_orientation: SheetOrientation,
    document_orientation: DocumentOrientation,
    bleed: f32,
    safety: f32,
    rotate: bool,
) -> String {
    let mut input_bytes = Vec::with_capacity(source_paths.len());
    for path in &source_paths {
        match fs::read(path) {
            Ok(b) => input_bytes.push(b),
            Err(e) => {
                return format!("Erreur lecture {} : {e}", path.display());
            }
        }
    }

    let selected_count = source_paths.len();
    let mut sources = Vec::new();
    for (offset, bytes) in input_bytes.into_iter().enumerate() {
        match sources_from_bytes(std::slice::from_ref(&bytes)) {
            Ok(parsed) => {
                if selected_count > 1 && parsed.len() > 1 {
                    // Plusieurs sources choisies: chaque PDF contribue sa première page.
                    if let Some(first) = parsed.first() {
                        sources.push(first.clone());
                    } else {
                        return format!(
                            "Erreur : source vide pour \"{}\".",
                            source_paths[offset].display()
                        );
                    }
                } else {
                    sources.extend(parsed);
                }
            }
            Err(e) => {
                return format!("Erreur source \"{}\" : {e}", source_paths[offset].display());
            }
        }
    }

    let config = ImpositionConfig {
        sheet_size: sheet,
        sheet_orientation,
        bleed_mm: bleed,
        safety_margin_mm: safety,
        document_orientation,
        rotate_copies: rotate,
    };

    match impose_sources(&sources, &config) {
        Ok(bytes) => match fs::write(&destination, &bytes) {
            Ok(()) => {
                format!(
                    "PDF généré : {} ({} octets)",
                    destination.display(),
                    bytes.len()
                )
            }
            Err(e) => format!("Erreur écriture : {e}"),
        },
        Err(e) => format!("Erreur : {e}"),
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
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
        .map_err(|e| ImposerError::Generation(e.to_string()))?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal =
        Terminal::new(backend).map_err(|e| ImposerError::Generation(e.to_string()))?;
    let _guard = RestoreTerminal;

    let mut app = App::new();
    loop {
        terminal
            .draw(|f| app.ui(f))
            .map_err(|e| ImposerError::Generation(e.to_string()))?;

        app.poll_busy();

        if app.should_quit {
            break;
        }

        let poll_timeout = if app.busy {
            std::time::Duration::from_millis(80)
        } else {
            std::time::Duration::from_millis(200)
        };

        if event::poll(poll_timeout).map_err(|e| ImposerError::Generation(e.to_string()))? {
            match event::read().map_err(|e| ImposerError::Generation(e.to_string()))? {
                Event::Key(key) => app.handle_key(key),
                Event::Mouse(mouse) => app.handle_mouse(mouse),
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
    }

    Ok(())
}
