use crate::execute;
use crate::execute::{ProgressEvent, ProgressReporter};
use crate::plan::{FfmpegPreference, Plan};
use crate::{batch, config, plan};
use anyhow::{Context, Result};
use crossterm::event::{self, Event as CEvent, KeyCode};
use crossterm::execute as crossterm_execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Wrap};
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

pub struct InteractiveDefaults {
    pub source: Option<std::path::PathBuf>,
    pub destination: Option<std::path::PathBuf>,
    pub batch: bool,
    pub dest_dir: Option<std::path::PathBuf>,
    pub inputs: Vec<String>,
    pub recursive: bool,
    pub to_ext: Option<String>,
    pub move_source: bool,
    pub overwrite: bool,
    pub backup: bool,
    pub image_quality: Option<u8>,
    pub video_bitrate: Option<String>,
    pub audio_bitrate: Option<String>,
    pub preset: Option<String>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub ffmpeg_preference: FfmpegPreference,
    pub config_path: Option<std::path::PathBuf>,
    pub profile: Option<String>,
    pub plan_only: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TaskStatus {
    Pending,
    Running,
    Ok,
    Failed,
}

impl TaskStatus {
    fn short(self) -> &'static str {
        match self {
            TaskStatus::Pending => "P",
            TaskStatus::Running => "R",
            TaskStatus::Ok => "OK",
            TaskStatus::Failed => "ER",
        }
    }
}

struct TaskState {
    label: String,
    name: String,
    destination: String,
    status: TaskStatus,
    percent: Option<f64>,
    eta: Option<f64>,
    message: String,
    spinner_elapsed: f32,
    started_at: Option<Instant>,
    finished_at: Option<Instant>,
}

impl TaskState {
    fn new(plan: &Plan) -> Self {
        let label = plan.source.display().to_string();
        let name = plan
            .source
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.to_string())
            .unwrap_or_else(|| label.clone());
        let destination = plan.destination.display().to_string();
        Self {
            label,
            name,
            destination,
            status: TaskStatus::Pending,
            percent: None,
            eta: None,
            message: String::new(),
            spinner_elapsed: 0.0,
            started_at: None,
            finished_at: None,
        }
    }
}

struct UiState {
    tasks: Vec<TaskState>,
    task_map: HashMap<String, usize>,
    active_index: usize,
    logs: VecDeque<String>,
}

struct Theme {
    primary: Color,
    accent: Color,
    muted: Color,
    good: Color,
    bad: Color,
}

impl Theme {
    fn new() -> Self {
        Self {
            primary: Color::Cyan,
            accent: Color::Yellow,
            muted: Color::DarkGray,
            good: Color::Green,
            bad: Color::Red,
        }
    }
}

impl UiState {
    fn new(plans: &[Plan]) -> Self {
        let mut tasks = Vec::with_capacity(plans.len());
        let mut task_map = HashMap::new();
        for (idx, plan) in plans.iter().enumerate() {
            let task = TaskState::new(plan);
            task_map.insert(task.label.clone(), idx);
            tasks.push(task);
        }
        Self {
            tasks,
            task_map,
            active_index: 0,
            logs: VecDeque::with_capacity(200),
        }
    }

    fn task_stats(&self) -> (usize, usize, usize, usize) {
        let mut pending = 0;
        let mut running = 0;
        let mut ok = 0;
        let mut failed = 0;
        for task in &self.tasks {
            match task.status {
                TaskStatus::Pending => pending += 1,
                TaskStatus::Running => running += 1,
                TaskStatus::Ok => ok += 1,
                TaskStatus::Failed => failed += 1,
            }
        }
        (pending, running, ok, failed)
    }

    fn push_log(&mut self, line: String) {
        if self.logs.len() == 200 {
            self.logs.pop_front();
        }
        self.logs.push_back(line);
    }

    fn handle_event(&mut self, event: ProgressEvent) {
        let label = match &event {
            ProgressEvent::Started { label }
            | ProgressEvent::Spinner { label, .. }
            | ProgressEvent::Progress { label, .. }
            | ProgressEvent::Finished { label, .. } => label,
        };
        let Some(&index) = self.task_map.get(label) else {
            return;
        };
        self.active_index = index;
        let mut log_line = None;
        {
            let task = &mut self.tasks[index];
            match event {
                ProgressEvent::Started { .. } => {
                    task.status = TaskStatus::Running;
                    task.started_at = Some(Instant::now());
                    task.message = "starting".to_string();
                    log_line = Some(format!("Started: {}", task.name));
                }
                ProgressEvent::Spinner {
                    elapsed, message, ..
                } => {
                    if task.status == TaskStatus::Pending {
                        task.status = TaskStatus::Running;
                    }
                    task.spinner_elapsed = elapsed;
                    task.message = message;
                }
                ProgressEvent::Progress { percent, eta, .. } => {
                    if task.status == TaskStatus::Pending {
                        task.status = TaskStatus::Running;
                    }
                    task.percent = Some(percent);
                    task.eta = eta;
                    task.message = "processing".to_string();
                }
                ProgressEvent::Finished { ok, message, .. } => {
                    task.status = if ok {
                        TaskStatus::Ok
                    } else {
                        TaskStatus::Failed
                    };
                    task.percent = Some(100.0);
                    task.finished_at = Some(Instant::now());
                    task.message = message.clone();
                    if ok {
                        log_line = Some(format!("Done: {}", task.name));
                    } else {
                        log_line = Some(format!("Failed: {} ({})", task.name, message));
                    }
                }
            }
        }
        if let Some(line) = log_line {
            self.push_log(line);
        }
    }
}

enum FormOutcome {
    Quit,
    Run {
        plans: Vec<Plan>,
        overwrite: bool,
        plan_only: bool,
    },
}

pub enum RunOutcome {
    Exit,
    Back,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FormMode {
    Single,
    Batch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Screen {
    Welcome,
    Configure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Panel {
    Inputs,
    Options,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InputField {
    Source,
    Destination,
    BatchInputs,
    DestDir,
    ToExt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OptionField {
    Recursive,
    MoveSource,
    Overwrite,
    Backup,
    ImageQuality,
    VideoBitrate,
    AudioBitrate,
    Preset,
    VideoCodec,
    AudioCodec,
    FfmpegPref,
    ConfigPath,
    Profile,
    PlanOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextField {
    Source,
    Destination,
    BatchInputs,
    DestDir,
    ToExt,
    ImageQuality,
    VideoBitrate,
    AudioBitrate,
    Preset,
    VideoCodec,
    AudioCodec,
    ConfigPath,
    Profile,
}

enum Modal {
    Browser(BrowserState),
    Recent(RecentState),
}

#[derive(Clone)]
struct BrowserEntry {
    name: String,
    path: std::path::PathBuf,
    is_dir: bool,
}

struct BrowserState {
    target: TextField,
    cwd: std::path::PathBuf,
    entries: Vec<BrowserEntry>,
    selected: usize,
    filter: String,
}

struct RecentState {
    target: TextField,
    entries: Vec<String>,
    selected: usize,
    filter: String,
}

struct EditState {
    field: TextField,
    buffer: String,
}

struct FormState {
    mode: FormMode,
    source: String,
    destination: String,
    batch_inputs: String,
    dest_dir: String,
    to_ext: String,
    recursive: bool,
    move_source: bool,
    overwrite: bool,
    backup: bool,
    image_quality: String,
    video_bitrate: String,
    audio_bitrate: String,
    preset: String,
    video_codec: String,
    audio_codec: String,
    ffmpeg_pref: FfmpegPreference,
    config_path: String,
    profile: String,
    plan_only: bool,
}

impl FormState {
    fn new(defaults: &InteractiveDefaults) -> Self {
        Self {
            mode: if defaults.batch {
                FormMode::Batch
            } else {
                FormMode::Single
            },
            source: defaults
                .source
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            destination: defaults
                .destination
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            batch_inputs: defaults.inputs.join("\n"),
            dest_dir: defaults
                .dest_dir
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            to_ext: defaults.to_ext.clone().unwrap_or_default(),
            recursive: defaults.recursive,
            move_source: defaults.move_source,
            overwrite: defaults.overwrite,
            backup: defaults.backup,
            image_quality: defaults
                .image_quality
                .map(|q| q.to_string())
                .unwrap_or_default(),
            video_bitrate: defaults.video_bitrate.clone().unwrap_or_default(),
            audio_bitrate: defaults.audio_bitrate.clone().unwrap_or_default(),
            preset: defaults.preset.clone().unwrap_or_default(),
            video_codec: defaults.video_codec.clone().unwrap_or_default(),
            audio_codec: defaults.audio_codec.clone().unwrap_or_default(),
            ffmpeg_pref: defaults.ffmpeg_preference,
            config_path: defaults
                .config_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            profile: defaults.profile.clone().unwrap_or_default(),
            plan_only: defaults.plan_only,
        }
    }
}

struct WizardState {
    screen: Screen,
    welcome_selected: usize,
    focus: Panel,
    input_index: usize,
    option_index: usize,
    edit: Option<EditState>,
    modal: Option<Modal>,
    error: Option<String>,
    form: FormState,
    history: Vec<String>,
}

impl WizardState {
    fn new(defaults: &InteractiveDefaults) -> Self {
        Self {
            screen: Screen::Welcome,
            welcome_selected: 0,
            focus: Panel::Inputs,
            input_index: 0,
            option_index: 0,
            edit: None,
            modal: None,
            error: None,
            form: FormState::new(defaults),
            history: load_history().unwrap_or_default(),
        }
    }
}

pub fn run_interactive(defaults: InteractiveDefaults) -> Result<()> {
    loop {
        let result = run_wizard_tui(&defaults)?;
        match result {
            FormOutcome::Quit => return Ok(()),
            FormOutcome::Run {
                plans,
                overwrite,
                plan_only,
            } => {
                if plan_only {
                    for plan in plans {
                        println!("{}", plan::render_plan(&plan, overwrite));
                    }
                    return Ok(());
                }
                let outcome = if plans.len() == 1 {
                    run_single_tui(&plans[0], overwrite)?
                } else {
                    run_batch_tui(plans, overwrite)?
                };
                if matches!(outcome, RunOutcome::Exit) {
                    return Ok(());
                }
            }
        }
    }
}

pub fn run_single_tui(plan: &Plan, overwrite: bool) -> Result<RunOutcome> {
    run_tui(vec![plan.clone()], overwrite)
}

pub fn run_batch_tui(plans: Vec<Plan>, overwrite: bool) -> Result<RunOutcome> {
    run_tui(plans, overwrite)
}

fn run_wizard_tui(defaults: &InteractiveDefaults) -> Result<FormOutcome> {
    let _guard = TerminalGuard::new()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
    let mut state = WizardState::new(defaults);
    let tick_rate = Duration::from_millis(120);

    loop {
        terminal.draw(|frame| render_wizard(frame, &state))?;

        if event::poll(tick_rate)?
            && let CEvent::Key(key) = event::read()?
        {
            if state.edit.is_some() {
                handle_edit_key(&mut state, key.code)?;
                continue;
            }
            if state.modal.is_some() {
                handle_modal_key(&mut state, key.code)?;
                continue;
            }

            match state.screen {
                Screen::Welcome => match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(FormOutcome::Quit),
                    KeyCode::Up => {
                        if state.welcome_selected > 0 {
                            state.welcome_selected -= 1;
                        }
                    }
                    KeyCode::Down => {
                        if state.welcome_selected < 2 {
                            state.welcome_selected += 1;
                        }
                    }
                    KeyCode::Enter => match state.welcome_selected {
                        0 => {
                            state.form.mode = FormMode::Single;
                            state.screen = Screen::Configure;
                        }
                        1 => {
                            state.form.mode = FormMode::Batch;
                            state.screen = Screen::Configure;
                        }
                        _ => return Ok(FormOutcome::Quit),
                    },
                    _ => {}
                },
                Screen::Configure => match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        state.screen = Screen::Welcome;
                    }
                    KeyCode::Tab => {
                        state.focus = match state.focus {
                            Panel::Inputs => Panel::Options,
                            Panel::Options => Panel::Inputs,
                        };
                    }
                    KeyCode::Up => move_selection(&mut state, -1),
                    KeyCode::Down => move_selection(&mut state, 1),
                    KeyCode::Left => cycle_enum(&mut state, -1),
                    KeyCode::Right => cycle_enum(&mut state, 1),
                    KeyCode::Char(' ') => toggle_field(&mut state),
                    KeyCode::Char('b') => open_browser(&mut state),
                    KeyCode::Char('r') => open_recent(&mut state),
                    KeyCode::Enter => {
                        if let Some(field) = selected_text_field(&state) {
                            let buffer = get_text_value(&state.form, field);
                            state.edit = Some(EditState { field, buffer });
                        }
                    }
                    KeyCode::F(5) => match build_plans(&mut state) {
                        Ok((plans, overwrite, plan_only)) => {
                            return Ok(FormOutcome::Run {
                                plans,
                                overwrite,
                                plan_only,
                            });
                        }
                        Err(err) => state.error = Some(err.to_string()),
                    },
                    _ => {}
                },
            }
        }
    }
}

fn input_fields(mode: FormMode) -> Vec<InputField> {
    match mode {
        FormMode::Single => vec![InputField::Source, InputField::Destination],
        FormMode::Batch => vec![
            InputField::BatchInputs,
            InputField::DestDir,
            InputField::ToExt,
        ],
    }
}

fn option_fields(mode: FormMode) -> Vec<OptionField> {
    let mut fields = Vec::new();
    if mode == FormMode::Batch {
        fields.push(OptionField::Recursive);
    }
    fields.extend([
        OptionField::MoveSource,
        OptionField::Overwrite,
        OptionField::Backup,
        OptionField::ImageQuality,
        OptionField::VideoBitrate,
        OptionField::AudioBitrate,
        OptionField::Preset,
        OptionField::VideoCodec,
        OptionField::AudioCodec,
        OptionField::FfmpegPref,
        OptionField::ConfigPath,
        OptionField::Profile,
        OptionField::PlanOnly,
    ]);
    fields
}

fn move_selection(state: &mut WizardState, delta: isize) {
    match state.focus {
        Panel::Inputs => {
            let fields = input_fields(state.form.mode);
            let len = fields.len();
            if len == 0 {
                return;
            }
            state.input_index = clamp_index(state.input_index, len, delta);
        }
        Panel::Options => {
            let len = option_fields(state.form.mode).len();
            state.option_index = clamp_index(state.option_index, len, delta);
        }
    }
}

fn clamp_index(index: usize, len: usize, delta: isize) -> usize {
    let next = index as isize + delta;
    if next < 0 {
        0
    } else if next as usize >= len {
        len.saturating_sub(1)
    } else {
        next as usize
    }
}

fn selected_text_field(state: &WizardState) -> Option<TextField> {
    match state.focus {
        Panel::Inputs => input_fields(state.form.mode)
            .get(state.input_index)
            .map(|field| match field {
                InputField::Source => TextField::Source,
                InputField::Destination => TextField::Destination,
                InputField::BatchInputs => TextField::BatchInputs,
                InputField::DestDir => TextField::DestDir,
                InputField::ToExt => TextField::ToExt,
            }),
        Panel::Options => option_fields(state.form.mode)
            .get(state.option_index)
            .and_then(|field| match field {
                OptionField::ImageQuality => Some(TextField::ImageQuality),
                OptionField::VideoBitrate => Some(TextField::VideoBitrate),
                OptionField::AudioBitrate => Some(TextField::AudioBitrate),
                OptionField::Preset => Some(TextField::Preset),
                OptionField::VideoCodec => Some(TextField::VideoCodec),
                OptionField::AudioCodec => Some(TextField::AudioCodec),
                OptionField::ConfigPath => Some(TextField::ConfigPath),
                OptionField::Profile => Some(TextField::Profile),
                _ => None,
            }),
    }
}

fn get_text_value(form: &FormState, field: TextField) -> String {
    match field {
        TextField::Source => form.source.clone(),
        TextField::Destination => form.destination.clone(),
        TextField::BatchInputs => form.batch_inputs.clone(),
        TextField::DestDir => form.dest_dir.clone(),
        TextField::ToExt => form.to_ext.clone(),
        TextField::ImageQuality => form.image_quality.clone(),
        TextField::VideoBitrate => form.video_bitrate.clone(),
        TextField::AudioBitrate => form.audio_bitrate.clone(),
        TextField::Preset => form.preset.clone(),
        TextField::VideoCodec => form.video_codec.clone(),
        TextField::AudioCodec => form.audio_codec.clone(),
        TextField::ConfigPath => form.config_path.clone(),
        TextField::Profile => form.profile.clone(),
    }
}

fn apply_text_value(form: &mut FormState, field: TextField, value: String) {
    match field {
        TextField::Source => form.source = value,
        TextField::Destination => form.destination = value,
        TextField::BatchInputs => form.batch_inputs = value,
        TextField::DestDir => form.dest_dir = value,
        TextField::ToExt => form.to_ext = value,
        TextField::ImageQuality => form.image_quality = value,
        TextField::VideoBitrate => form.video_bitrate = value,
        TextField::AudioBitrate => form.audio_bitrate = value,
        TextField::Preset => form.preset = value,
        TextField::VideoCodec => form.video_codec = value,
        TextField::AudioCodec => form.audio_codec = value,
        TextField::ConfigPath => form.config_path = value,
        TextField::Profile => form.profile = value,
    }
}

fn handle_edit_key(state: &mut WizardState, key: KeyCode) -> Result<()> {
    let Some(edit) = state.edit.as_mut() else {
        return Ok(());
    };
    match key {
        KeyCode::Esc => {
            state.edit = None;
        }
        KeyCode::Enter => {
            let value = edit.buffer.clone();
            let field = edit.field;
            apply_text_value(&mut state.form, field, value);
            state.edit = None;
        }
        KeyCode::Tab => {
            if let Some(updated) = autocomplete_edit(edit.field, &edit.buffer) {
                edit.buffer = updated;
            }
        }
        KeyCode::Backspace => {
            edit.buffer.pop();
        }
        KeyCode::Char(ch) => {
            edit.buffer.push(ch);
        }
        _ => {}
    }
    Ok(())
}

fn autocomplete_edit(field: TextField, buffer: &str) -> Option<String> {
    if !matches!(
        field,
        TextField::Source
            | TextField::Destination
            | TextField::BatchInputs
            | TextField::DestDir
            | TextField::ConfigPath
    ) {
        return None;
    }
    if field == TextField::BatchInputs {
        let (prefix, token) = split_last_token(buffer);
        let completed = autocomplete_path(&token)?;
        return Some(format!("{prefix}{completed}"));
    }
    autocomplete_path(buffer)
}

fn split_last_token(buffer: &str) -> (String, String) {
    let (mut prefix, mut token) = if let Some(pos) = buffer.rfind('\n') {
        (buffer[..=pos].to_string(), buffer[pos + 1..].to_string())
    } else {
        ("".to_string(), buffer.to_string())
    };
    if let Some(pos) = token.rfind(',') {
        prefix.push_str(&token[..=pos]);
        token = token[pos + 1..].trim_start().to_string();
    }
    (prefix, token)
}

fn autocomplete_path(token: &str) -> Option<String> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (dir_prefix, base_prefix) = split_dir_prefix(trimmed);
    let dir_path = expand_tilde(&dir_prefix);
    let Ok(read_dir) = std::fs::read_dir(&dir_path) else {
        return None;
    };
    let mut matches: Vec<String> = read_dir
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(base_prefix) {
                Some(name)
            } else {
                None
            }
        })
        .collect();
    if matches.is_empty() {
        return None;
    }
    matches.sort();
    let common = common_prefix(&matches)?;
    if common == base_prefix {
        if matches.len() == 1 {
            return Some(format!("{dir_prefix}{}", matches[0]));
        }
        return None;
    }
    Some(format!("{dir_prefix}{common}"))
}

fn split_dir_prefix(input: &str) -> (String, &str) {
    if input.ends_with(std::path::MAIN_SEPARATOR) {
        return (input.to_string(), "");
    }
    if let Some(pos) = input.rfind(std::path::MAIN_SEPARATOR) {
        let (dir, base) = input.split_at(pos + 1);
        return (dir.to_string(), base);
    }
    ("".to_string(), input)
}

fn common_prefix(items: &[String]) -> Option<String> {
    let mut prefix = items.first()?.clone();
    for item in items.iter().skip(1) {
        let mut next = String::new();
        for (a, b) in prefix.chars().zip(item.chars()) {
            if a == b {
                next.push(a);
            } else {
                break;
            }
        }
        prefix = next;
        if prefix.is_empty() {
            break;
        }
    }
    Some(prefix)
}

fn expand_tilde(value: &str) -> std::path::PathBuf {
    if let Some(stripped) = value.strip_prefix("~/")
        && let Ok(home) = std::env::var("HOME")
    {
        return std::path::PathBuf::from(home).join(stripped);
    }
    std::path::PathBuf::from(value)
}

fn handle_modal_key(state: &mut WizardState, key: KeyCode) -> Result<()> {
    let Some(modal) = state.modal.take() else {
        return Ok(());
    };
    match modal {
        Modal::Browser(mut browser) => {
            let close = handle_browser_key(state, &mut browser, key)?;
            if !close {
                state.modal = Some(Modal::Browser(browser));
            }
        }
        Modal::Recent(mut recent) => {
            let close = handle_recent_key(state, &mut recent, key)?;
            if !close {
                state.modal = Some(Modal::Recent(recent));
            }
        }
    }
    Ok(())
}

fn handle_browser_key(
    state: &mut WizardState,
    browser: &mut BrowserState,
    key: KeyCode,
) -> Result<bool> {
    match key {
        KeyCode::Esc => {
            return Ok(true);
        }
        KeyCode::Up => {
            if browser.selected > 0 {
                browser.selected -= 1;
            }
        }
        KeyCode::Down => {
            if browser.selected + 1 < browser.entries.len() {
                browser.selected += 1;
            }
        }
        KeyCode::Backspace => {
            if !browser.filter.is_empty() {
                browser.filter.pop();
                refresh_browser_entries(browser)?;
            } else if let Some(parent) = browser.cwd.parent() {
                browser.cwd = parent.to_path_buf();
                refresh_browser_entries(browser)?;
            }
        }
        KeyCode::Enter => {
            if let Some(entry) = browser.entries.get(browser.selected).cloned() {
                if browser.target == TextField::BatchInputs {
                    if entry.is_dir {
                        browser.cwd = entry.path;
                        refresh_browser_entries(browser)?;
                    } else {
                        append_to_batch_inputs(&mut state.form, entry.path.display().to_string());
                        return Ok(true);
                    }
                } else if browser.target == TextField::DestDir {
                    if entry.is_dir {
                        apply_text_value(
                            &mut state.form,
                            browser.target,
                            entry.path.display().to_string(),
                        );
                        return Ok(true);
                    } else if let Some(parent) = entry.path.parent() {
                        apply_text_value(
                            &mut state.form,
                            browser.target,
                            parent.display().to_string(),
                        );
                        return Ok(true);
                    }
                } else if entry.is_dir {
                    browser.cwd = entry.path;
                    refresh_browser_entries(browser)?;
                } else {
                    apply_text_value(
                        &mut state.form,
                        browser.target,
                        entry.path.display().to_string(),
                    );
                    return Ok(true);
                }
            }
        }
        KeyCode::Char('a') => {
            if browser.target == TextField::BatchInputs
                && let Some(entry) = browser.entries.get(browser.selected)
            {
                append_to_batch_inputs(&mut state.form, entry.path.display().to_string());
            }
        }
        KeyCode::Char(ch) => {
            browser.filter.push(ch);
            refresh_browser_entries(browser)?;
        }
        _ => {}
    }
    Ok(false)
}

fn handle_recent_key(
    state: &mut WizardState,
    recent: &mut RecentState,
    key: KeyCode,
) -> Result<bool> {
    match key {
        KeyCode::Esc => {
            return Ok(true);
        }
        KeyCode::Up => {
            if recent.selected > 0 {
                recent.selected -= 1;
            }
        }
        KeyCode::Down => {
            if recent.selected + 1 < recent.entries.len() {
                recent.selected += 1;
            }
        }
        KeyCode::Backspace => {
            if !recent.filter.is_empty() {
                recent.filter.pop();
                refresh_recent_entries(state, recent);
            }
        }
        KeyCode::Enter => {
            if let Some(entry) = recent.entries.get(recent.selected).cloned() {
                apply_recent_selection(state, recent.target, entry);
                return Ok(true);
            }
        }
        KeyCode::Char(ch) => {
            recent.filter.push(ch);
            refresh_recent_entries(state, recent);
        }
        _ => {}
    }
    Ok(false)
}

fn toggle_field(state: &mut WizardState) {
    match state.focus {
        Panel::Inputs => {}
        Panel::Options => match option_fields(state.form.mode).get(state.option_index) {
            Some(OptionField::Recursive) => state.form.recursive = !state.form.recursive,
            Some(OptionField::MoveSource) => state.form.move_source = !state.form.move_source,
            Some(OptionField::Overwrite) => {
                state.form.overwrite = !state.form.overwrite;
                if state.form.overwrite {
                    state.form.backup = false;
                }
            }
            Some(OptionField::Backup) => {
                state.form.backup = !state.form.backup;
                if state.form.backup {
                    state.form.overwrite = false;
                }
            }
            Some(OptionField::PlanOnly) => state.form.plan_only = !state.form.plan_only,
            _ => {}
        },
    }
}

fn cycle_enum(state: &mut WizardState, delta: i8) {
    if state.focus != Panel::Options {
        return;
    }
    if option_fields(state.form.mode).get(state.option_index) != Some(&OptionField::FfmpegPref) {
        return;
    }
    state.form.ffmpeg_pref = match (state.form.ffmpeg_pref, delta) {
        (FfmpegPreference::Auto, 1) => FfmpegPreference::StreamCopy,
        (FfmpegPreference::StreamCopy, 1) => FfmpegPreference::Transcode,
        (FfmpegPreference::Transcode, 1) => FfmpegPreference::Auto,
        (FfmpegPreference::Auto, -1) => FfmpegPreference::Transcode,
        (FfmpegPreference::StreamCopy, -1) => FfmpegPreference::Auto,
        (FfmpegPreference::Transcode, -1) => FfmpegPreference::StreamCopy,
        (value, _) => value,
    };
}

fn open_browser(state: &mut WizardState) {
    let Some(target) = selected_path_field(state) else {
        return;
    };
    let cwd = initial_cwd(state, target);
    let mut browser = BrowserState {
        target,
        cwd,
        entries: Vec::new(),
        selected: 0,
        filter: String::new(),
    };
    if refresh_browser_entries(&mut browser).is_ok() {
        state.modal = Some(Modal::Browser(browser));
    }
}

fn open_recent(state: &mut WizardState) {
    let Some(target) = selected_path_field(state) else {
        return;
    };
    let mut recent = RecentState {
        target,
        entries: Vec::new(),
        selected: 0,
        filter: String::new(),
    };
    refresh_recent_entries(state, &mut recent);
    state.modal = Some(Modal::Recent(recent));
}

fn selected_path_field(state: &WizardState) -> Option<TextField> {
    match state.focus {
        Panel::Inputs => input_fields(state.form.mode)
            .get(state.input_index)
            .and_then(|field| match field {
                InputField::Source => Some(TextField::Source),
                InputField::Destination => Some(TextField::Destination),
                InputField::BatchInputs => Some(TextField::BatchInputs),
                InputField::DestDir => Some(TextField::DestDir),
                InputField::ToExt => None,
            }),
        Panel::Options => option_fields(state.form.mode)
            .get(state.option_index)
            .and_then(|field| match field {
                OptionField::ConfigPath => Some(TextField::ConfigPath),
                _ => None,
            }),
    }
}

fn initial_cwd(state: &WizardState, target: TextField) -> std::path::PathBuf {
    let value = get_text_value(&state.form, target);
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    }
    let path = expand_tilde(trimmed);
    if path.is_dir() {
        return path;
    }
    if let Some(parent) = path.parent() {
        return parent.to_path_buf();
    }
    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
}

fn refresh_browser_entries(browser: &mut BrowserState) -> Result<()> {
    let mut entries = Vec::new();
    if let Some(parent) = browser.cwd.parent() {
        entries.push(BrowserEntry {
            name: "..".to_string(),
            path: parent.to_path_buf(),
            is_dir: true,
        });
    }
    let filter = browser.filter.to_lowercase();
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(&browser.cwd) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if !filter.is_empty() && !name.to_lowercase().contains(&filter) {
                continue;
            }
            let is_dir = path.is_dir();
            let entry = BrowserEntry { name, path, is_dir };
            if is_dir {
                dirs.push(entry);
            } else {
                files.push(entry);
            }
        }
    }
    dirs.sort_by(|a, b| a.name.cmp(&b.name));
    files.sort_by(|a, b| a.name.cmp(&b.name));
    entries.extend(dirs);
    entries.extend(files);
    browser.entries = entries;
    if browser.selected >= browser.entries.len() {
        browser.selected = browser.entries.len().saturating_sub(1);
    }
    Ok(())
}

fn refresh_recent_entries(state: &WizardState, recent: &mut RecentState) {
    let filter = recent.filter.to_lowercase();
    let mut entries = Vec::new();
    for item in &state.history {
        if filter.is_empty() || item.to_lowercase().contains(&filter) {
            entries.push(item.clone());
        }
    }
    recent.entries = entries;
    if recent.selected >= recent.entries.len() {
        recent.selected = recent.entries.len().saturating_sub(1);
    }
}

fn apply_recent_selection(state: &mut WizardState, target: TextField, value: String) {
    if target == TextField::BatchInputs {
        append_to_batch_inputs(&mut state.form, value);
        return;
    }
    if target == TextField::DestDir {
        let path = expand_tilde(&value);
        if path.is_file()
            && let Some(parent) = path.parent()
        {
            apply_text_value(&mut state.form, target, parent.display().to_string());
            return;
        }
    }
    apply_text_value(&mut state.form, target, value);
}

fn append_to_batch_inputs(form: &mut FormState, value: String) {
    if form.batch_inputs.trim().is_empty() {
        form.batch_inputs = value;
    } else {
        form.batch_inputs.push('\n');
        form.batch_inputs.push_str(&value);
    }
}

fn build_plans(state: &mut WizardState) -> Result<(Vec<Plan>, bool, bool)> {
    state.error = None;
    let config_path = state.form.config_path.trim();
    let profile = state.form.profile.trim();
    let mut options = if !config_path.is_empty() || !profile.is_empty() {
        config::load_options(
            if config_path.is_empty() {
                None
            } else {
                Some(std::path::Path::new(config_path))
            },
            if profile.is_empty() {
                None
            } else {
                Some(profile)
            },
        )?
        .unwrap_or_default()
    } else {
        plan::ConversionOptions::default()
    };

    let image_quality = state.form.image_quality.trim();
    if !image_quality.is_empty() {
        let value: u8 = image_quality
            .parse()
            .context("image quality must be a number")?;
        if value == 0 || value > 100 {
            anyhow::bail!("image quality must be between 1 and 100");
        }
        options.image_quality = Some(value);
    }
    let video_bitrate = state.form.video_bitrate.trim();
    options.video_bitrate = if video_bitrate.is_empty() {
        None
    } else {
        Some(video_bitrate.to_string())
    };
    let audio_bitrate = state.form.audio_bitrate.trim();
    options.audio_bitrate = if audio_bitrate.is_empty() {
        None
    } else {
        Some(audio_bitrate.to_string())
    };
    let preset = state.form.preset.trim();
    options.preset = if preset.is_empty() {
        None
    } else {
        Some(preset.to_string())
    };
    let video_codec = state.form.video_codec.trim();
    options.video_codec = if video_codec.is_empty() {
        None
    } else {
        Some(video_codec.to_string())
    };
    let audio_codec = state.form.audio_codec.trim();
    options.audio_codec = if audio_codec.is_empty() {
        None
    } else {
        Some(audio_codec.to_string())
    };
    options.ffmpeg_preference = state.form.ffmpeg_pref;

    let mut plans = Vec::new();
    match state.form.mode {
        FormMode::Single => {
            let source = state.form.source.trim();
            let destination = state.form.destination.trim();
            if source.is_empty() || destination.is_empty() {
                anyhow::bail!("source and destination are required");
            }
            let plan = plan::build_plan(
                std::path::Path::new(source),
                std::path::Path::new(destination),
                state.form.move_source,
                state.form.backup,
                options,
            )?;
            plans.push(plan);
        }
        FormMode::Batch => {
            let dest_dir = state.form.dest_dir.trim();
            if dest_dir.is_empty() {
                anyhow::bail!("destination directory is required");
            }
            let inputs = parse_inputs(&state.form.batch_inputs);
            if inputs.is_empty() {
                anyhow::bail!("at least one input is required");
            }
            let sources = batch::collect_sources(&inputs, Vec::new(), state.form.recursive)?;
            if sources.is_empty() {
                anyhow::bail!("no inputs resolved for batch mode");
            }
            let batch_input = batch::BatchInput {
                dest_dir: std::path::PathBuf::from(dest_dir),
                to_ext: if state.form.to_ext.trim().is_empty() {
                    None
                } else {
                    Some(state.form.to_ext.trim().to_string())
                },
            };
            for source in sources {
                let destination = batch::dest_for_source(&batch_input, &source)?;
                let plan = plan::build_plan(
                    &source,
                    &destination,
                    state.form.move_source,
                    state.form.backup,
                    options.clone(),
                )?;
                plans.push(plan);
            }
        }
    }

    let mut additions = Vec::new();
    match state.form.mode {
        FormMode::Single => {
            additions.push(state.form.source.clone());
            additions.push(state.form.destination.clone());
        }
        FormMode::Batch => {
            additions.push(state.form.dest_dir.clone());
            additions.extend(parse_inputs(&state.form.batch_inputs));
        }
    }
    update_history(state, additions)?;

    Ok((plans, state.form.overwrite, state.form.plan_only))
}

fn parse_inputs(raw: &str) -> Vec<String> {
    raw.lines()
        .flat_map(|line| line.split(','))
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .map(|item| item.to_string())
        .collect()
}

fn render_wizard(frame: &mut Frame<'_>, state: &WizardState) {
    match state.screen {
        Screen::Welcome => render_welcome(frame, state),
        Screen::Configure => {
            render_config(frame, state);
            if let Some(modal) = &state.modal {
                match modal {
                    Modal::Browser(browser) => render_browser_modal(frame, browser),
                    Modal::Recent(recent) => render_recent_modal(frame, recent),
                }
            }
        }
    }
}

fn render_welcome(frame: &mut Frame<'_>, state: &WizardState) {
    let theme = Theme::new();
    let area = frame.area();
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(6),
            Constraint::Length(3),
        ])
        .split(area);

    let title = Paragraph::new(vec![
        Line::from(Span::styled(
            "mvx",
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "Move. Convert. Verify.",
            Style::default().fg(theme.muted),
        )),
    ])
    .block(
        Block::default().borders(Borders::ALL).title(Span::styled(
            "Welcome",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        )),
    );
    frame.render_widget(title, layout[0]);

    let options = ["Start single conversion", "Start batch conversion", "Quit"];
    let items: Vec<ListItem> = options
        .iter()
        .map(|label| ListItem::new(Line::from(*label)))
        .collect();
    let mut list_state = ratatui::widgets::ListState::default();
    list_state.select(Some(state.welcome_selected.min(options.len() - 1)));
    let list = List::new(items)
        .block(
            Block::default().borders(Borders::ALL).title(Span::styled(
                "Choose",
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            )),
        )
        .highlight_style(
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_stateful_widget(list, layout[1], &mut list_state);

    let footer = Paragraph::new(Line::from(vec![
        Span::styled(
            "Enter",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" select, "),
        Span::styled(
            "q",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" quit"),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(Span::styled("Help", Style::default().fg(theme.muted))),
    );
    frame.render_widget(footer, layout[2]);
}

fn render_config(frame: &mut Frame<'_>, state: &WizardState) {
    let theme = Theme::new();
    let area = frame.area();
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(area);

    let summary = match state.form.mode {
        FormMode::Single => format!(
            "Mode: single  Source: {}  Dest: {}",
            short_value(&state.form.source),
            short_value(&state.form.destination)
        ),
        FormMode::Batch => format!(
            "Mode: batch  Inputs: {}  Dest dir: {}",
            summarize_inputs(&state.form.batch_inputs),
            short_value(&state.form.dest_dir)
        ),
    };
    let header = Paragraph::new(Line::from(Span::styled(
        summary,
        Style::default().fg(theme.primary),
    )))
    .block(
        Block::default().borders(Borders::ALL).title(Span::styled(
            "Setup",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        )),
    );
    frame.render_widget(header, layout[0]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(layout[1]);

    let input_items = input_fields(state.form.mode);
    let input_list: Vec<ListItem> = input_items
        .iter()
        .map(|field| {
            let (label, value) = input_label_value(field, &state.form);
            ListItem::new(Line::from(format!("{label:<14} {value}")))
        })
        .collect();
    let mut input_state = ratatui::widgets::ListState::default();
    if !input_items.is_empty() {
        input_state.select(Some(state.input_index.min(input_items.len() - 1)));
    }
    let input_block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            "Inputs",
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ))
        .border_style(match state.focus {
            Panel::Inputs => Style::default().fg(theme.primary),
            Panel::Options => Style::default().fg(theme.muted),
        });
    let input_list = List::new(input_list).block(input_block).highlight_style(
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_stateful_widget(input_list, body[0], &mut input_state);

    let option_items = option_fields(state.form.mode);
    let option_list: Vec<ListItem> = option_items
        .iter()
        .map(|field| {
            let (label, value) = option_label_value(field, &state.form);
            ListItem::new(Line::from(format!("{label:<14} {value}")))
        })
        .collect();
    let mut option_state = ratatui::widgets::ListState::default();
    if !option_items.is_empty() {
        option_state.select(Some(state.option_index.min(option_items.len() - 1)));
    }
    let option_block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            "Options",
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ))
        .border_style(match state.focus {
            Panel::Options => Style::default().fg(theme.primary),
            Panel::Inputs => Style::default().fg(theme.muted),
        });
    let option_list = List::new(option_list).block(option_block).highlight_style(
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_stateful_widget(option_list, body[1], &mut option_state);

    let footer_text = if let Some(edit) = &state.edit {
        format!(
            "Edit: {} (Enter save, Tab autocomplete, Esc cancel)",
            edit_label(edit.field)
        )
    } else if let Some(error) = state.error.as_deref() {
        format!("Error: {error}")
    } else {
        "Tab switch panel, Enter edit, b browse, r recent, Space toggle, F5 run, Esc back"
            .to_string()
    };
    let footer = Paragraph::new(Line::from(Span::styled(
        footer_text,
        Style::default().fg(theme.muted),
    )))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(Span::styled("Help", Style::default().fg(theme.muted))),
    );
    frame.render_widget(footer, layout[2]);

    if let Some(edit) = &state.edit {
        let edit_area = centered_rect(70, 20, area);
        let edit_block = Block::default().borders(Borders::ALL).title(Span::styled(
            format!("Editing {}", edit_label(edit.field)),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ));
        let edit_text = Paragraph::new(Line::from(Span::styled(
            edit.buffer.as_str(),
            Style::default().fg(theme.primary),
        )))
        .block(edit_block)
        .wrap(Wrap { trim: true });
        frame.render_widget(edit_text, edit_area);
    }
}

fn render_browser_modal(frame: &mut Frame<'_>, browser: &BrowserState) {
    let theme = Theme::new();
    let area = centered_rect(80, 70, frame.area());
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(4),
            Constraint::Length(3),
        ])
        .split(area);

    let header = Paragraph::new(Line::from(Span::styled(
        format!("Browse: {}", browser.cwd.display()),
        Style::default()
            .fg(theme.primary)
            .add_modifier(Modifier::BOLD),
    )))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(Span::styled("Files", Style::default().fg(theme.accent))),
    );
    frame.render_widget(header, layout[0]);

    let filter_label = if browser.filter.is_empty() {
        "<type to filter>".to_string()
    } else {
        browser.filter.clone()
    };
    let filter = Paragraph::new(Line::from(Span::styled(
        format!("Filter: {filter_label}"),
        Style::default().fg(theme.muted),
    )))
    .block(Block::default().borders(Borders::ALL).title("Filter"));
    frame.render_widget(filter, layout[1]);

    let items: Vec<ListItem> = browser
        .entries
        .iter()
        .map(|entry| {
            let name = if entry.is_dir {
                format!("{}/", entry.name)
            } else {
                entry.name.clone()
            };
            ListItem::new(Line::from(name))
        })
        .collect();
    let mut list_state = ratatui::widgets::ListState::default();
    if !browser.entries.is_empty() {
        list_state.select(Some(browser.selected.min(browser.entries.len() - 1)));
    }
    let list = List::new(items)
        .block(
            Block::default().borders(Borders::ALL).title(Span::styled(
                "Browse",
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            )),
        )
        .highlight_style(
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_stateful_widget(list, layout[2], &mut list_state);

    let help = "Enter open/select, a add (batch), Backspace filter/up, Esc close";
    let footer = Paragraph::new(Line::from(Span::styled(
        help,
        Style::default().fg(theme.muted),
    )))
    .block(Block::default().borders(Borders::ALL).title("Help"));
    frame.render_widget(footer, layout[3]);
}

fn render_recent_modal(frame: &mut Frame<'_>, recent: &RecentState) {
    let theme = Theme::new();
    let area = centered_rect(70, 60, frame.area());
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(4),
            Constraint::Length(3),
        ])
        .split(area);

    let header = Paragraph::new(Line::from(Span::styled(
        "Recent paths",
        Style::default()
            .fg(theme.primary)
            .add_modifier(Modifier::BOLD),
    )))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(Span::styled("Recent", Style::default().fg(theme.accent))),
    );
    frame.render_widget(header, layout[0]);

    let items: Vec<ListItem> = recent
        .entries
        .iter()
        .map(|entry| ListItem::new(Line::from(entry.clone())))
        .collect();
    let mut list_state = ratatui::widgets::ListState::default();
    if !recent.entries.is_empty() {
        list_state.select(Some(recent.selected.min(recent.entries.len() - 1)));
    }
    let list = List::new(items)
        .block(
            Block::default().borders(Borders::ALL).title(Span::styled(
                "Pick",
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            )),
        )
        .highlight_style(
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_stateful_widget(list, layout[1], &mut list_state);

    let footer_text = if recent.filter.is_empty() {
        "Type to filter, Enter select, Esc close"
    } else {
        "Filtering... (Enter select, Esc close)"
    };
    let footer = Paragraph::new(Line::from(Span::styled(
        footer_text,
        Style::default().fg(theme.muted),
    )))
    .block(Block::default().borders(Borders::ALL).title("Help"));
    frame.render_widget(footer, layout[2]);
}

fn input_label_value(field: &InputField, form: &FormState) -> (String, String) {
    match field {
        InputField::Source => ("Source".to_string(), short_value(&form.source)),
        InputField::Destination => ("Destination".to_string(), short_value(&form.destination)),
        InputField::BatchInputs => ("Inputs".to_string(), summarize_inputs(&form.batch_inputs)),
        InputField::DestDir => ("Dest dir".to_string(), short_value(&form.dest_dir)),
        InputField::ToExt => ("To ext".to_string(), short_value(&form.to_ext)),
    }
}

fn option_label_value(field: &OptionField, form: &FormState) -> (String, String) {
    match field {
        OptionField::Recursive => ("Recursive".to_string(), yes_no(form.recursive)),
        OptionField::MoveSource => ("Move source".to_string(), yes_no(form.move_source)),
        OptionField::Overwrite => ("Overwrite".to_string(), yes_no(form.overwrite)),
        OptionField::Backup => ("Backup".to_string(), yes_no(form.backup)),
        OptionField::ImageQuality => (
            "Image quality".to_string(),
            short_value(&form.image_quality),
        ),
        OptionField::VideoBitrate => (
            "Video bitrate".to_string(),
            short_value(&form.video_bitrate),
        ),
        OptionField::AudioBitrate => (
            "Audio bitrate".to_string(),
            short_value(&form.audio_bitrate),
        ),
        OptionField::Preset => ("Preset".to_string(), short_value(&form.preset)),
        OptionField::VideoCodec => ("Video codec".to_string(), short_value(&form.video_codec)),
        OptionField::AudioCodec => ("Audio codec".to_string(), short_value(&form.audio_codec)),
        OptionField::FfmpegPref => (
            "FFmpeg mode".to_string(),
            match form.ffmpeg_pref {
                FfmpegPreference::Auto => "auto".to_string(),
                FfmpegPreference::StreamCopy => "stream-copy".to_string(),
                FfmpegPreference::Transcode => "transcode".to_string(),
            },
        ),
        OptionField::ConfigPath => ("Config path".to_string(), short_value(&form.config_path)),
        OptionField::Profile => ("Profile".to_string(), short_value(&form.profile)),
        OptionField::PlanOnly => ("Plan only".to_string(), yes_no(form.plan_only)),
    }
}

fn edit_label(field: TextField) -> &'static str {
    match field {
        TextField::Source => "Source",
        TextField::Destination => "Destination",
        TextField::BatchInputs => "Inputs",
        TextField::DestDir => "Dest dir",
        TextField::ToExt => "To ext",
        TextField::ImageQuality => "Image quality",
        TextField::VideoBitrate => "Video bitrate",
        TextField::AudioBitrate => "Audio bitrate",
        TextField::Preset => "Preset",
        TextField::VideoCodec => "Video codec",
        TextField::AudioCodec => "Audio codec",
        TextField::ConfigPath => "Config path",
        TextField::Profile => "Profile",
    }
}

fn short_value(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "<empty>".to_string()
    } else if trimmed.len() > 32 {
        format!("{}...", &trimmed[..29])
    } else {
        trimmed.to_string()
    }
}

fn summarize_inputs(value: &str) -> String {
    let items = parse_inputs(value);
    if items.is_empty() {
        "<empty>".to_string()
    } else if items.len() == 1 {
        items[0].clone()
    } else {
        format!("{} entries", items.len())
    }
}

fn yes_no(value: bool) -> String {
    if value {
        "yes".to_string()
    } else {
        "no".to_string()
    }
}

fn history_path() -> Result<std::path::PathBuf> {
    let base = match std::env::var("XDG_CONFIG_HOME") {
        Ok(path) => std::path::PathBuf::from(path),
        Err(_) => {
            let home = std::env::var("HOME").context("HOME not set")?;
            std::path::PathBuf::from(home).join(".config")
        }
    };
    Ok(base.join("mvx").join("history.txt"))
}

fn load_history() -> Result<Vec<String>> {
    let path = history_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let mut seen = std::collections::HashSet::new();
    let mut items = Vec::new();
    for line in contents.lines() {
        let value = line.trim();
        if value.is_empty() || !seen.insert(value.to_string()) {
            continue;
        }
        items.push(value.to_string());
    }
    Ok(items)
}

fn save_history(items: &[String]) -> Result<()> {
    let path = history_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let contents = items.join("\n");
    std::fs::write(&path, contents)?;
    Ok(())
}

fn update_history(state: &mut WizardState, additions: Vec<String>) -> Result<()> {
    let mut items = state.history.clone();
    for item in additions {
        let value = item.trim().to_string();
        if value.is_empty() {
            continue;
        }
        items.retain(|existing| existing != &value);
        items.insert(0, value);
    }
    items.truncate(50);
    save_history(&items)?;
    state.history = items;
    Ok(())
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn run_tui(plans: Vec<Plan>, overwrite: bool) -> Result<RunOutcome> {
    let (event_tx, event_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let is_batch = plans.len() > 1;
    let plans_for_worker = plans.clone();

    thread::spawn(move || {
        let reporter = ProgressReporter::tui(event_tx);
        let mut failed = Vec::new();
        for plan in plans_for_worker {
            if let Err(err) = execute::execute_plan_with_reporter(&plan, overwrite, &reporter) {
                failed.push((plan.source.display().to_string(), err.to_string()));
            }
        }
        let result = if failed.is_empty() {
            Ok(())
        } else if is_batch {
            Err(anyhow::anyhow!(format!(
                "batch completed with {} failures",
                failed.len()
            )))
        } else {
            Err(anyhow::anyhow!("conversion failed"))
        };
        let _ = done_tx.send(result);
    });

    let _guard = TerminalGuard::new()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
    let mut ui_state = UiState::new(&plans);
    let mut done = false;
    let mut done_result: Option<Result<()>> = None;
    let tick_rate = Duration::from_millis(120);

    loop {
        while let Ok(event) = event_rx.try_recv() {
            ui_state.handle_event(event);
        }

        if !done && let Ok(result) = done_rx.try_recv() {
            done = true;
            done_result = Some(result);
        }

        terminal.draw(|frame| render_ui(frame, &ui_state, done))?;

        if event::poll(tick_rate)?
            && let CEvent::Key(key) = event::read()?
        {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => {
                    if done {
                        break;
                    }
                }
                KeyCode::Char('b') => {
                    if done {
                        return Ok(RunOutcome::Back);
                    }
                }
                _ => {}
            }
        }
    }

    if let Some(result) = done_result {
        result?;
    }
    Ok(RunOutcome::Exit)
}

fn render_ui(frame: &mut Frame<'_>, ui_state: &UiState, done: bool) {
    let theme = Theme::new();
    let area = frame.area();
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Min(4),
            Constraint::Length(2),
        ])
        .split(area);

    let (pending, running, ok, failed) = ui_state.task_stats();
    let total = ui_state.tasks.len().max(1);
    let completed = ok + failed;
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            "mvx",
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  total "),
        Span::styled(total.to_string(), Style::default().fg(theme.primary)),
        Span::raw("  pending "),
        Span::styled(pending.to_string(), Style::default().fg(theme.muted)),
        Span::raw("  running "),
        Span::styled(running.to_string(), Style::default().fg(theme.accent)),
        Span::raw("  done "),
        Span::styled(completed.to_string(), Style::default().fg(theme.good)),
        Span::raw("  failed "),
        Span::styled(failed.to_string(), Style::default().fg(theme.bad)),
    ]))
    .block(
        Block::default().borders(Borders::ALL).title(Span::styled(
            "Status",
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        )),
    );
    frame.render_widget(header, layout[0]);

    let overall_percent = ((completed as f64 / total as f64) * 100.0).min(100.0);
    let gauge = Gauge::default()
        .block(
            Block::default().borders(Borders::ALL).title(Span::styled(
                "Overall",
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            )),
        )
        .gauge_style(Style::default().fg(theme.good))
        .percent(overall_percent.round() as u16);
    frame.render_widget(gauge, layout[1]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(layout[2]);

    let items: Vec<ListItem> = ui_state
        .tasks
        .iter()
        .map(|task| {
            let progress = task
                .percent
                .map(|p| format!("{:>3.0}%", p))
                .unwrap_or_else(|| "    ".to_string());
            let line = Line::from(format!(
                "{} {} {}",
                task.status.short(),
                progress,
                task.name
            ));
            ListItem::new(line)
        })
        .collect();
    let mut list_state = ratatui::widgets::ListState::default();
    if !ui_state.tasks.is_empty() {
        list_state.select(Some(ui_state.active_index));
    }
    let list = List::new(items)
        .block(
            Block::default().borders(Borders::ALL).title(Span::styled(
                "Queue",
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            )),
        )
        .highlight_style(
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_stateful_widget(list, body[0], &mut list_state);

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(4)])
        .split(body[1]);

    let active = ui_state.tasks.get(ui_state.active_index);
    let detail_lines = if let Some(task) = active {
        let eta = task
            .eta
            .map(|eta| format!("{:.1}s", eta))
            .unwrap_or_else(|| "-".to_string());
        vec![
            Line::from(format!("Source: {}", task.label)),
            Line::from(format!("Destination: {}", task.destination)),
            Line::from(format!(
                "Status: {}  Progress: {}",
                task.status.short(),
                task.percent
                    .map(|p| format!("{:.0}%", p))
                    .unwrap_or_else(|| "-".to_string())
            )),
            Line::from(format!("ETA: {eta}  Note: {}", task.message)),
        ]
    } else {
        vec![Line::from("No tasks")]
    };
    let details = Paragraph::new(detail_lines)
        .block(
            Block::default().borders(Borders::ALL).title(Span::styled(
                "Details",
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            )),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(details, right[0]);

    let log_height = right[1].height.saturating_sub(2) as usize;
    let log_items: Vec<ListItem> = ui_state
        .logs
        .iter()
        .rev()
        .take(log_height)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|line| ListItem::new(Line::from(line)))
        .collect();
    let logs = List::new(log_items).block(
        Block::default().borders(Borders::ALL).title(Span::styled(
            "Activity",
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        )),
    );
    frame.render_widget(logs, right[1]);

    let footer_text = if done {
        "Completed. Press q to exit or b to go back."
    } else {
        "Running... (press q after completion to exit)"
    };
    let footer = Paragraph::new(Line::from(Span::styled(
        footer_text,
        Style::default().fg(theme.muted),
    )))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(Span::styled("Help", Style::default().fg(theme.muted))),
    );
    frame.render_widget(footer, layout[3]);
}

struct TerminalGuard;

impl TerminalGuard {
    fn new() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        crossterm_execute!(stdout, EnterAlternateScreen)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = std::io::stdout();
        let _ = crossterm_execute!(stdout, LeaveAlternateScreen);
    }
}
