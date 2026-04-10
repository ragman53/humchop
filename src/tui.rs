//! TUI - Terminal User Interface for HumChop.
//!
//! Provides an interactive terminal UI for:
//! - Loading and previewing audio samples
//! - Recording hummed melodies
//! - Viewing detected notes
//! - Processing and saving chopped output
//!
//! Uses JDilla-style chopping by default (strength-based matching, chops keep original length).

use crate::audio_utils::{self, DEFAULT_SAMPLE_RATE};
use crate::error::HumChopError;
use crate::hum_analyzer::{HumAnalyzer, Note};
use crate::mapper::{Mapper, MapperConfig};
use crate::sample_chopper::SampleChopper;

#[cfg(feature = "audio-io")]
use crate::recorder::{calculate_audio_level, Recorder};

use crossterm::event::{self, Event, KeyCode, KeyEvent};
use crossterm::execute;
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Wrap},
    Frame, Terminal,
};
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[cfg(feature = "audio-io")]
use tokio::sync::mpsc as tokio_mpsc;

/// Maximum recording duration in seconds.
const MAX_RECORDING_DURATION_SECS: f64 = 15.0;

/// Application state.
#[derive(Debug, Clone, PartialEq)]
pub enum AppState {
    /// Initial state, waiting for sample
    Idle,
    /// Loading a sample file
    Loading,
    /// Sample loaded, ready to preview/record
    Ready,
    /// Recording hummed input
    Recording,
    /// Processing notes and chops
    Processing,
    /// Processing complete, showing results
    Complete,
    /// Error state
    Error,
}

impl Default for AppState {
    fn default() -> Self {
        AppState::Idle
    }
}

/// Application-wide state.
pub struct App {
    /// Current state
    pub state: AppState,
    /// Loaded sample data
    pub sample: Option<Vec<f32>>,
    /// Sample rate
    pub sample_rate: u32,
    /// Sample file path
    pub sample_path: Option<PathBuf>,
    /// Recorded hum data
    pub hum_data: Option<Vec<f32>>,
    /// Detected notes from hum
    pub notes: Vec<Note>,
    /// Processing output path
    pub output_path: Option<PathBuf>,
    /// Error message
    pub error_message: Option<String>,
    /// Recording duration in seconds
    pub recording_duration: f64,
    /// Recording start time
    pub recording_start: Option<Instant>,
    /// Processing progress (0.0 to 1.0)
    pub processing_progress: f32,
    /// Mapper configuration
    pub mapper_config: MapperConfig,
    /// Audio level for recording meter (0.0 to 1.0)
    pub audio_level: f32,
    /// Quit flag
    pub should_quit: bool,
    /// Audio recorder (only available with audio-io feature)
    #[cfg(feature = "audio-io")]
    pub recorder: Option<Recorder>,
    /// Audio receiver channel (only available with audio-io feature)
    #[cfg(feature = "audio-io")]
    pub audio_receiver: Option<tokio_mpsc::Receiver<Vec<f32>>>,
}

impl App {
    /// Create a new App instance.
    pub fn new() -> Self {
        Self {
            state: AppState::Idle,
            sample: None,
            sample_rate: DEFAULT_SAMPLE_RATE,
            sample_path: None,
            hum_data: None,
            notes: Vec::new(),
            output_path: None,
            error_message: None,
            recording_duration: 0.0,
            recording_start: None,
            processing_progress: 0.0,
            mapper_config: MapperConfig::default(),
            audio_level: 0.0,
            should_quit: false,
            #[cfg(feature = "audio-io")]
            recorder: None,
            #[cfg(feature = "audio-io")]
            audio_receiver: None,
        }
    }

    /// Load a sample file.
    pub fn load_sample(&mut self, path: &PathBuf) -> Result<(), HumChopError> {
        self.state = AppState::Loading;
        self.sample_path = Some(path.clone());

        let (samples, sample_rate) = audio_utils::load_audio(path)
            .map_err(|e| HumChopError::Other(format!("Failed to load audio: {}", e)))?;

        self.sample = Some(samples);
        self.sample_rate = sample_rate;
        self.state = AppState::Ready;

        Ok(())
    }

    /// Start recording.
    #[cfg(feature = "audio-io")]
    pub fn start_recording(&mut self) {
        if self.state != AppState::Ready {
            return;
        }

        // Initialize recorder if not already done
        if self.recorder.is_none() {
            self.recorder = Some(Recorder::new());
        }

        // Start recording and get receiver
        let recorder = match self.recorder.as_mut() {
            Some(r) => r,
            None => {
                self.set_error("Failed to initialize recorder".to_string());
                return;
            }
        };

        match recorder.start_recording() {
            Ok(receiver) => {
                self.audio_receiver = Some(receiver);
                self.state = AppState::Recording;
                self.recording_start = Some(Instant::now());
                self.hum_data = Some(Vec::new());
                self.audio_level = 0.0;
                log::info!("Recording started");
            }
            Err(e) => {
                self.set_error(format!("Failed to start recording: {}", e));
            }
        }
    }

    #[cfg(not(feature = "audio-io"))]
    pub fn start_recording(&mut self) {
        // Without audio-io, just simulate recording
        if self.state != AppState::Ready {
            return;
        }

        self.state = AppState::Recording;
        self.recording_start = Some(Instant::now());
        self.hum_data = Some(Vec::new());
        self.audio_level = 0.0;
    }

    /// Stop recording and process.
    #[cfg(feature = "audio-io")]
    pub fn stop_recording(&mut self) {
        if self.state != AppState::Recording {
            return;
        }

        // Stop the recorder
        if let Some(ref mut recorder) = self.recorder {
            recorder.stop_recording();
        }
        self.audio_receiver = None;

        self.recording_duration = self
            .recording_start
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(0.0);

        log::info!(
            "Recording stopped, duration: {:.2}s",
            self.recording_duration
        );
        self.process_hum();
    }

    #[cfg(not(feature = "audio-io"))]
    pub fn stop_recording(&mut self) {
        if self.state != AppState::Recording {
            return;
        }

        self.recording_duration = self
            .recording_start
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(0.0);

        self.process_hum();
    }

    /// Process recorded hum data.
    fn process_hum(&mut self) {
        let hum_samples = match self.hum_data.take() {
            Some(s) => s,
            None => {
                self.state = AppState::Error;
                self.error_message = Some("No recording data".to_string());
                return;
            }
        };

        let sample = match self.sample.clone() {
            Some(s) => s,
            None => {
                self.state = AppState::Error;
                self.error_message = Some("No sample loaded".to_string());
                return;
            }
        };

        self.state = AppState::Processing;
        self.processing_progress = 0.0;

        // Analyze pitch
        self.processing_progress = 0.2;
        let analyzer = HumAnalyzer::new(self.sample_rate);

        match analyzer.transcribe(&hum_samples) {
            Ok(notes) => {
                self.notes = notes.clone();
                self.processing_progress = 0.5;

                // Chop and map using JDilla-style
                let chopper = SampleChopper::new(self.sample_rate);
                let chops = match chopper.chop(&sample, notes.len()) {
                    Ok(c) => c,
                    Err(e) => {
                        self.state = AppState::Error;
                        self.error_message = Some(format!("Failed to chop: {}", e));
                        return;
                    }
                };

                self.processing_progress = 0.7;

                let mapper = Mapper::with_config(self.sample_rate, self.mapper_config.clone());
                let mapped = match mapper.process(&notes, &chops) {
                    Ok(m) => m,
                    Err(e) => {
                        self.state = AppState::Error;
                        self.error_message = Some(format!("Failed to map: {}", e));
                        return;
                    }
                };

                self.processing_progress = 0.85;

                let output = mapper.render_output(&mapped);

                // Save output
                let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
                let output_path = PathBuf::from(format!("output_chopped_{}.wav", timestamp));

                match audio_utils::write_wav(&output_path, &output, self.sample_rate) {
                    Ok(()) => {
                        self.output_path = Some(output_path);
                        self.processing_progress = 1.0;
                        self.state = AppState::Complete;
                    }
                    Err(e) => {
                        self.state = AppState::Error;
                        self.error_message = Some(format!("Failed to write output: {}", e));
                    }
                }
            }
            Err(e) => {
                self.state = AppState::Error;
                self.error_message = Some(format!("Failed to analyze hum: {}", e));
            }
        }
    }

    /// Reset to ready state.
    pub fn reset(&mut self) {
        self.hum_data = None;
        self.notes.clear();
        self.output_path = None;
        self.error_message = None;
        self.recording_duration = 0.0;
        self.recording_start = None;
        self.processing_progress = 0.0;

        if self.sample.is_some() {
            self.state = AppState::Ready;
        } else {
            self.state = AppState::Idle;
        }
    }

    /// Set error state.
    pub fn set_error(&mut self, message: String) {
        self.state = AppState::Error;
        self.error_message = Some(message);
    }

    /// Toggle pitch matching mode.
    pub fn toggle_pitch_matching(&mut self) {
        self.mapper_config.strength_matching = !self.mapper_config.strength_matching;
    }
}

/// Render the main layout.
fn render_ui(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(10),   // Main content
            Constraint::Length(6), // Footer / Status
        ])
        .split(frame.area());

    // Header
    render_header(frame, chunks[0], app);

    // Main content
    render_main(frame, chunks[1], app);

    // Footer
    render_footer(frame, chunks[2], app);
}

/// Render the header with title and shortcuts.
fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let title = match app.state {
        AppState::Idle => "HumChop - Load a sample file to begin",
        AppState::Loading => "HumChop - Loading...",
        AppState::Ready => "HumChop - Ready",
        AppState::Recording => "HumChop - Recording...",
        AppState::Processing => "HumChop - Processing...",
        AppState::Complete => "HumChop - Complete!",
        AppState::Error => "HumChop - Error",
    };

    let state_color = match app.state {
        AppState::Idle => Color::DarkGray,
        AppState::Ready => Color::Green,
        AppState::Recording => Color::Red,
        AppState::Processing => Color::Yellow,
        AppState::Complete => Color::Green,
        AppState::Error => Color::Red,
        _ => Color::White,
    };

    let block = Block::default()
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(title, Style::default().fg(state_color)),
        ]))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    frame.render_widget(block, area);
}

/// Render main content area.
fn render_main(frame: &mut Frame, area: Rect, app: &App) {
    match app.state {
        AppState::Idle | AppState::Loading => {
            render_idle_content(frame, area, app);
        }
        AppState::Ready => {
            render_ready_content(frame, area, app);
        }
        AppState::Recording => {
            render_recording_content(frame, area, app);
        }
        AppState::Processing => {
            render_processing_content(frame, area, app);
        }
        AppState::Complete => {
            render_complete_content(frame, area, app);
        }
        AppState::Error => {
            render_error_content(frame, area, app);
        }
    }
}

/// Render idle/loading content.
fn render_idle_content(frame: &mut Frame, area: Rect, app: &App) {
    let text = if app.state == AppState::Loading {
        vec![
            Line::from("Loading audio file..."),
            Line::from(""),
            Line::from(Span::raw(format!("Path: {:?}", app.sample_path))),
        ]
    } else {
        vec![
            Line::from("Welcome to HumChop!"),
            Line::from(""),
            Line::from("Usage: humchop <audio_file>"),
            Line::from(""),
            Line::from("JDilla-style mode: chops keep original length,"),
            Line::from("notes determine which chop plays (by strength/pitch)."),
            Line::from(""),
            Line::from("Supported formats: WAV, MP3, FLAC"),
            Line::from(""),
            Line::from("Key bindings:"),
            Line::from("  r - Start/stop recording"),
            Line::from("  m - Toggle matching mode (strength/pitch)"),
            Line::from("  q - Quit"),
        ]
    };

    let paragraph = Paragraph::new(text)
        .block(Block::default().title("Info").borders(Borders::ALL))
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, area);
}

/// Render ready content (sample loaded).
fn render_ready_content(frame: &mut Frame, area: Rect, app: &App) {
    let sample_info = if let Some(ref path) = app.sample_path {
        format!("File: {}", path.display())
    } else {
        "No file loaded".to_string()
    };

    let sample_stats = if let Some(ref samples) = app.sample {
        let duration = samples.len() as f64 / app.sample_rate as f64;
        format!(
            "Samples: {} | Duration: {:.2}s | Rate: {} Hz",
            samples.len(),
            duration,
            app.sample_rate
        )
    } else {
        "No sample data".to_string()
    };

    let matching_mode = if app.mapper_config.strength_matching {
        "Strength (JDilla)"
    } else {
        "Pitch"
    };

    let text = vec![
        Line::from(Span::raw(sample_info)),
        Line::from(Span::raw(sample_stats)),
        Line::from(""),
        Line::from(Span::raw(format!(
            "Mode: JDilla | Matching: {}",
            matching_mode
        ))),
        Line::from(""),
        Line::from("Press 'r' to start recording your hum"),
    ];

    let paragraph = Paragraph::new(text)
        .block(
            Block::default()
                .title("Sample Loaded")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, area);
}

/// Render recording content.
fn render_recording_content(frame: &mut Frame, area: Rect, app: &App) {
    let elapsed = app
        .recording_start
        .map(|t| t.elapsed().as_secs_f64())
        .unwrap_or(0.0);

    let level_bar = format!(
        "{:.1}s / 15.0s [{}]",
        elapsed.min(15.0),
        "#".repeat((elapsed as usize).min(30))
    );

    let text = vec![
        Line::from(vec![
            Span::raw("Recording... "),
            Span::styled(elapsed.to_string(), Style::default().fg(Color::Red)),
            Span::raw(" seconds"),
        ]),
        Line::from(""),
        Line::from("Press 'r' again to stop"),
        Line::from(""),
        Line::from("Level:"),
        Line::from(Span::raw(level_bar)),
    ];

    let paragraph = Paragraph::new(text)
        .block(Block::default().title("Recording").borders(Borders::ALL))
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, area);

    // Render level meter
    let meter_area = Rect::new(area.x + 1, area.y + 6, area.width.saturating_sub(2), 2);
    let gauge = Gauge::default()
        .ratio((elapsed / 15.0) as f64)
        .label(format!("{:.1}s", elapsed))
        .style(Style::default().fg(Color::Red));
    frame.render_widget(gauge, meter_area);
}

/// Render processing content.
fn render_processing_content(frame: &mut Frame, area: Rect, app: &App) {
    let progress = app.processing_progress;
    let progress_bar = "#".repeat((progress * 20.0) as usize);
    let remaining = " ".repeat(((1.0 - progress) * 20.0) as usize);

    let text = vec![
        Line::from("Processing (JDilla-style)..."),
        Line::from(""),
        Line::from(format!(
            "Analyzing: {} Chopping: {} Mapping: {}",
            if progress > 0.1 { "✓" } else { "○" },
            if progress > 0.4 { "✓" } else { "○" },
            if progress > 0.7 { "✓" } else { "○" },
        )),
        Line::from(""),
        Line::from(vec![
            Span::raw("["),
            Span::styled(progress_bar, Style::default().fg(Color::Green)),
            Span::raw(remaining),
            Span::raw("] "),
            Span::raw(format!("{:.0}%", progress * 100.0)),
        ]),
    ];

    let paragraph = Paragraph::new(text)
        .block(Block::default().title("Processing").borders(Borders::ALL))
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, area);
}

/// Render complete content.
fn render_complete_content(frame: &mut Frame, area: Rect, app: &App) {
    let output_str = app
        .output_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    let notes_str = format!("{} notes detected", app.notes.len());

    let notes_list: Vec<ListItem> = app
        .notes
        .iter()
        .take(10)
        .map(|n| {
            let name = n.to_note_name();
            let time = format!("{:.2}s", n.onset_sec);
            ListItem::new(format!(
                "{} @ {} (vel: {:.0}%)",
                name,
                time,
                n.velocity * 100.0
            ))
        })
        .collect();

    let notes_widget = List::new(notes_list).block(
        Block::default()
            .title(format!("Detected Notes ({})", notes_str))
            .borders(Borders::ALL),
    );

    frame.render_widget(notes_widget, area);

    // Show output info
    let info_area = Rect::new(
        area.x,
        area.y + area.height.saturating_sub(3),
        area.width,
        3,
    );
    let info = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("✓ ", Style::default().fg(Color::Green)),
            Span::raw("Output saved: "),
            Span::raw(&output_str),
        ]),
        Line::from(""),
        Line::from("Press 'r' to start over or 'q' to quit"),
    ])
    .wrap(Wrap { trim: true });
    frame.render_widget(info, info_area);
}

/// Render error content.
fn render_error_content(frame: &mut Frame, area: Rect, app: &App) {
    let error_msg = app
        .error_message
        .as_ref()
        .map(|s| s.as_str())
        .unwrap_or("Unknown error");

    let text = vec![
        Line::from(vec![
            Span::styled("✗ ", Style::default().fg(Color::Red)),
            Span::raw("Error:"),
        ]),
        Line::from(""),
        Line::from(Span::raw(error_msg)),
        Line::from(""),
        Line::from("Press 'r' to try again or 'q' to quit"),
    ];

    let paragraph = Paragraph::new(text)
        .block(
            Block::default()
                .title("Error")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red)),
        )
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, area);
}

/// Render footer with status and controls.
fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    let matching_mode = if app.mapper_config.strength_matching {
        "Strength"
    } else {
        "Pitch"
    };

    let status = match app.state {
        AppState::Idle => "[q] Quit | [r] Start Recording".to_string(),
        AppState::Loading => "[q] Quit".to_string(),
        AppState::Ready => format!("[q] Quit | [r] Record | [m] Mode: {}", matching_mode),
        AppState::Recording => "[r] Stop Recording".to_string(),
        AppState::Processing => "[q] Cancel".to_string(),
        AppState::Complete => "[r] New Recording | [q] Quit".to_string(),
        AppState::Error => "[r] Try Again | [q] Quit".to_string(),
    };

    let block = Block::default()
        .title(Line::from(Span::raw(status)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    frame.render_widget(block, area);
}

/// Run the TUI event loop.
pub fn run_tui(mut app: App) -> io::Result<()> {
    // Set up terminal
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    // Main event loop
    loop {
        // Render
        terminal.draw(|f| render_ui(f, &app))?;

        // Check for quit
        if app.should_quit {
            break;
        }

        // Handle events with timeout
        if let Ok(true) = crossterm::event::poll(Duration::from_millis(100)) {
            if let Event::Key(key) = event::read()? {
                handle_key_event(key, &mut app);
            }
        }

        // Update recording timer display
        if app.state == AppState::Recording {
            if let Some(start) = app.recording_start {
                let elapsed = start.elapsed().as_secs_f64();

                // Auto-stop at 15 seconds
                if elapsed >= 15.0 {
                    app.stop_recording();
                }
            }
        }
    }

    // Restore terminal
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}

/// Handle key events.
fn handle_key_event(key: KeyEvent, app: &mut App) {
    match key.code {
        KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
            app.should_quit = true;
        }

        KeyCode::Char('r') | KeyCode::Char('R') => match app.state {
            AppState::Ready => {
                app.start_recording();
            }
            AppState::Recording => {
                app.stop_recording();
            }
            AppState::Complete | AppState::Error => {
                app.reset();
            }
            _ => {}
        },

        KeyCode::Char('m') | KeyCode::Char('M') => {
            if app.state == AppState::Ready {
                app.toggle_pitch_matching();
            }
        }

        _ => {}
    }
}

/// Run TUI from CLI arguments.
pub fn run_with_args(args: &[String]) -> io::Result<()> {
    let mut app = App::new();

    // Parse arguments
    if args.len() < 2 {
        // Run in idle mode
        return run_tui(app);
    }

    // Load sample from argument
    let sample_path = PathBuf::from(&args[1]);

    if !sample_path.exists() {
        app.set_error(format!("File not found: {}", sample_path.display()));
        return run_tui(app);
    }

    match app.load_sample(&sample_path) {
        Ok(()) => {
            // Run TUI
            run_tui(app)
        }
        Err(e) => {
            app.set_error(format!("Failed to load sample: {}", e));
            run_tui(app)
        }
    }
}
