use std::{
    cmp::Ordering,
    collections::HashSet,
    env, fs, io,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    prelude::*,
    widgets::{block::Title, *},
};

#[derive(Clone)]
struct Entry {
    name: String,
    path: PathBuf,
    is_dir: bool,
}

struct App {
    cwd: PathBuf,
    entries: Vec<Entry>,
    list_state: ListState,
    selected_paths: HashSet<PathBuf>,
}

impl App {
    fn new(start_dir: PathBuf) -> Result<Self> {
        let mut app = Self {
            cwd: start_dir,
            entries: Vec::new(),
            list_state: ListState::default(),
            selected_paths: HashSet::new(),
        };
        app.reload_entries()?;
        if !app.entries.is_empty() {
            app.list_state.select(Some(0));
        }
        Ok(app)
    }

    fn reload_entries(&mut self) -> Result<()> {
        self.entries = read_dir_sorted(&self.cwd)?;
        Ok(())
    }

    fn selected_index(&self) -> Option<usize> {
        self.list_state.selected()
    }

    fn selected_entry(&mut self) -> Option<&Entry> {
        self.selected_index().and_then(|i| self.entries.get(i))
    }

    pub fn move_by(&mut self, delta: isize) {
        let len = self.entries.len();
        if len == 0 {
            self.list_state.select(None);
            return;
        }
        let start = self.list_state.selected().unwrap_or(0) as isize;
        let len_i = len as isize;

        // Proper wrap for negative/positive deltas
        let idx = (start + delta).rem_euclid(len_i) as usize;
        self.list_state.select(Some(idx));
    }

    pub fn next(&mut self) {
        self.move_by(1);
    }

    pub fn prev(&mut self) {
        self.move_by(-1);
    }

    fn enter(&mut self) -> Result<()> {
        if let Some(e) = self.selected_entry() {
            if e.is_dir {
                // end borrow before mutating self
                let path = e.path.clone();
                self.cwd = path;
                self.reload_entries()?;
                self.list_state.select(Some(0));
            } else {
                open_with_editor(&e.path)?;
            }
        }
        Ok(())
    }

    fn toggle_mark(&mut self) {
        if let Some(e) = self.selected_entry() {
            let p = e.path.clone();
            if !self.selected_paths.insert(p.clone()) {
                self.selected_paths.remove(&p);
            }
        }
    }

    fn up_dir(&mut self) -> Result<()> {
        if let Some(parent) = self.cwd.parent() {
            self.cwd = parent.to_path_buf();
            self.reload_entries()?;
            self.list_state.select(Some(0));
        }
        Ok(())
    }
}

fn main() -> Result<()> {
    let start_dir = env::current_dir()?;
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let res = run_app(&mut terminal, start_dir);

    // Restore
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(e) = res {
        eprintln!("error: {e:?}");
        std::process::exit(1);
    }
    Ok(())
}

fn run_app(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    start_dir: PathBuf,
) -> Result<()> {
    let mut app = App::new(start_dir)?;
    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        // Use poll so we can redraw at intervals if needed (smooth resize, etc.)
        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(k) = event::read()? {
                // Ignore repeat events on key hold for some terminals
                if k.kind == KeyEventKind::Release {
                    continue;
                }
                match k.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Down | KeyCode::Char('j') => app.next(),
                    KeyCode::Up | KeyCode::Char('k') => app.prev(),
                    KeyCode::Backspace => app.up_dir()?,
                    KeyCode::Char('r') => app.reload_entries()?,
                    KeyCode::Char(' ') => app.toggle_mark(),
                    KeyCode::Enter => app.enter()?,
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

fn ui(f: &mut Frame, app: &mut App) {
    let size = f.size();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Title::from(Line::from(vec![
            Span::raw(" "),
            Span::styled("Ratatui File Picker", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" "),
        ])))
        .title(
            Title::from(Line::from(vec![
                Span::raw(" "),
                Span::raw(format!(
                    "cwd: {}  |  selected: {}  |  ↑/↓ move  ␣ toggle  Enter open  ⌫ up  r refresh  q quit",
                    app.cwd.display(),
                    app.selected_paths.len()
                )),
            ]))
            .alignment(Alignment::Right),
        )
        .border_type(BorderType::Rounded);

    let area = block.inner(size);
    f.render_widget(block, size);

    // Build list items
    let items: Vec<ListItem> = app
        .entries
        .iter()
        .map(|e| {
            let mark = if app.selected_paths.contains(&e.path) {
                "●"
            } else {
                "○"
            };
            let icon = if e.is_dir { "📁" } else { "📄" };
            let line = Line::from(vec![
                Span::raw(format!("{mark} {icon} ")),
                Span::styled(
                    &e.name,
                    if e.is_dir {
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    },
                ),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .highlight_symbol("➤ ")
        .highlight_style(Style::default().bg(Color::Gray).fg(Color::Black));

    f.render_stateful_widget(list, area, &mut app.list_state);
}

fn read_dir_sorted(dir: &Path) -> Result<Vec<Entry>> {
    let mut v: Vec<Entry> = fs::read_dir(dir)
        .with_context(|| format!("reading directory {}", dir.display()))?
        .filter_map(|res| {
            let entry = res.ok()?;
            let md = entry.metadata().ok()?;
            let is_dir = md.is_dir();
            let name = entry.file_name().to_string_lossy().into_owned();
            Some(Entry {
                name,
                path: entry.path(),
                is_dir,
            })
        })
        .collect();

    v.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    Ok(v)
}

fn open_with_editor(path: &Path) -> Result<()> {
    // Leave raw/alt to let the editor take over
    // We'll temporarily tear down the TUI, spawn, then rebuild automatically
    // by re-entering alt-screen on redraw.
    disable_raw_mode().ok();
    // We intentionally do not LeaveAlternateScreen here, because some editors
    // handle alt-screen themselves poorly. Use a small trick: print a reset.
    // But a safer cross-terminal approach is to fully leave alt-screen:
    let mut stdout = io::stdout();
    let _ = execute!(stdout, LeaveAlternateScreen);

    // Choose editor
    let editor = env::var("EDITOR").unwrap_or_else(|_| "less".to_string());
    let cmdline = format!(
        "{} {}",
        editor,
        shell_escape::escape(path.to_string_lossy().into_owned().into())
    );

    // If EDITOR has spaces/flags, run via sh -c
    let status = if editor.contains(' ') {
        Command::new("sh").arg("-c").arg(&cmdline).status()
    } else {
        Command::new(editor).arg(path).status()
    }
    .or_else(|_| Command::new("less").arg(path).status())
    .or_else(|_| Command::new("vi").arg(path).status())?;

    // Return to TUI
    let _ = execute!(io::stdout(), EnterAlternateScreen);
    enable_raw_mode().ok();

    if !status.success() {
        eprintln!("Editor exited with status: {:?}", status.code());
    }
    Ok(())
}

// Minimal shell-escape for safety in `sh -c` case.
// Pulled-in as a tiny re-implementation to avoid extra deps;
// but to keep the example self-contained, we do this:
//
// NOTE: We *do* use the `shell_escape` crate name above; if you
// prefer zero extra deps, replace the call with the simple implementation below.
//
// For simplicity of the tutorial, include the crate in Cargo.toml instead:
// shell-escape = "0.1"
mod shell_escape {
    pub fn escape(s: String) -> String {
        // Quote with single quotes and escape embedded single quotes: ' -> '\''
        if s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:@".contains(c))
        {
            s
        } else {
            format!("'{}'", s.replace('\'', r"'\''"))
        }
    }
}
