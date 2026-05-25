use fltk::{
    app,
    button::{Button, CheckButton},
    enums::{Align, Color, Font, FrameType},
    frame::Frame,
    group::{Pack, PackType, Scroll, Tile},
    input::{Input, IntInput},
    prelude::*,
    table::{Table, TableContext},
    text::{TextBuffer, TextDisplay},
    window::Window,
};
use fopener_actions::runner::ProcessActionRunner;
use fopener_config::{
    loader::{load_config, save_config},
    paths::default_config_path,
    schema::AppConfig,
};
use fopener_core::{
    matching::RuleMatcher,
    types::{ActionTemplate, AppEvent, FileCandidate, MatchDecision, WatchRule},
};
use fopener_watcher::engine::{WatcherEngine, WatcherHandle};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

// ─── Shared application state ────────────────────────────────────────────────

struct AppState {
    config: AppConfig,
    config_path: PathBuf,
    log_lines: Vec<String>,
    watcher_handle: Option<WatcherHandle>,
    is_running: bool,
    /// Channel receiver for AppEvents from the watcher thread
    event_rx: Option<std::sync::mpsc::Receiver<AppEvent>>,
}

impl AppState {
    fn new(config: AppConfig, config_path: PathBuf) -> Self {
        Self {
            config,
            config_path,
            log_lines: Vec::new(),
            watcher_handle: None,
            is_running: false,
            event_rx: None,
        }
    }

    fn push_log(&mut self, msg: impl Into<String>) {
        use chrono::Local;
        let ts = Local::now().format("%H:%M:%S").to_string();
        self.log_lines.push(format!("[{ts}] {}", msg.into()));
        // Keep last 500 lines
        if self.log_lines.len() > 500 {
            self.log_lines.drain(0..self.log_lines.len() - 500);
        }
    }
}

// ─── Rule edit dialog ─────────────────────────────────────────────────────────

/// Shows a modal dialog for adding or editing a rule.
/// Returns Some(updated_rule) on Save, None on Cancel.
fn show_rule_dialog(existing: Option<&WatchRule>) -> Option<WatchRule> {
    let template = existing.cloned().unwrap_or_else(|| WatchRule::new_default("New Rule"));

    let mut win = Window::new(100, 100, 520, 600, "Edit Rule");
    win.make_resizable(false);

    let mut scroll = Scroll::new(0, 0, 520, 555, "");
    scroll.set_type(fltk::group::ScrollType::Vertical);

    let mut pack = Pack::new(10, 10, 490, 540, "");
    pack.set_type(PackType::Vertical);
    pack.set_spacing(4);

    // Helper to add a labelled input row
    macro_rules! labeled_input {
        ($label:expr, $value:expr, $height:expr) => {{
            let mut row = Pack::new(0, 0, 490, $height, "");
            row.set_type(PackType::Horizontal);
            row.set_spacing(4);
            let mut lbl = Frame::new(0, 0, 150, $height, $label);
            lbl.set_align(Align::Right | Align::Inside);
            let mut inp = Input::new(0, 0, 330, $height, "");
            inp.set_value($value);
            row.end();
            inp
        }};
    }

    macro_rules! labeled_int_input {
        ($label:expr, $value:expr) => {{
            let mut row = Pack::new(0, 0, 490, 28, "");
            row.set_type(PackType::Horizontal);
            row.set_spacing(4);
            let mut lbl = Frame::new(0, 0, 150, 28, $label);
            lbl.set_align(Align::Right | Align::Inside);
            let mut inp = IntInput::new(0, 0, 330, 28, "");
            inp.set_value(&$value.to_string());
            row.end();
            inp
        }};
    }

    macro_rules! labeled_check {
        ($label:expr, $checked:expr) => {{
            let mut row = Pack::new(0, 0, 490, 28, "");
            row.set_type(PackType::Horizontal);
            row.set_spacing(4);
            let mut _spacer = Frame::new(0, 0, 150, 28, "");
            let chk = CheckButton::new(0, 0, 330, 28, $label);
            chk.set_checked($checked);
            row.end();
            chk
        }};
    }

    let inp_name = labeled_input!("Name:", &template.name, 28);
    let inp_path = labeled_input!("Watch Folder:", &template.path.to_string_lossy(), 28);
    let inp_mask = labeled_input!("File Mask:", &template.file_mask, 28);
    let inp_regex = labeled_input!("Regex (optional):", template.regex.as_deref().unwrap_or(""), 28);
    let chk_subdirs = labeled_check!("Include subdirs", template.include_subdirectories);
    let inp_executable = labeled_input!("Executable:", &template.action.executable.to_string_lossy(), 28);
    let inp_arguments = labeled_input!("Arguments:", &template.action.arguments.join(" "), 28);
    let inp_debounce = labeled_int_input!("Debounce (ms):", template.debounce_ms);
    let chk_wait_stable = labeled_check!("Wait until stable", template.wait_until_stable);
    let inp_stable_ms = labeled_int_input!("Stable check (ms):", template.stable_check_ms);
    let inp_stable_count = labeled_int_input!("Stable check count:", template.stable_checks_count);
    let inp_timeout = labeled_int_input!("Ready timeout (sec):", template.ready_timeout_sec);
    let chk_enabled = labeled_check!("Enabled", template.enabled);

    pack.end();
    scroll.end();

    // Button row
    let mut btn_row = Pack::new(10, 560, 490, 30, "");
    btn_row.set_type(PackType::Horizontal);
    btn_row.set_spacing(8);
    let mut btn_test = Button::new(0, 0, 120, 30, "Test Rule");
    let mut btn_save = Button::new(0, 0, 100, 30, "Save");
    let mut btn_cancel = Button::new(0, 0, 100, 30, "Cancel");
    btn_row.end();

    win.end();
    win.make_modal(true);
    win.show();

    // Result storage
    let result: Rc<RefCell<Option<WatchRule>>> = Rc::new(RefCell::new(None));

    // --- Test Rule button ---
    let inp_name_c = inp_name.clone();
    let inp_mask_c = inp_mask.clone();
    let inp_regex_c = inp_regex.clone();
    let inp_path_c = inp_path.clone();
    let chk_subdirs_c = chk_subdirs.clone();
    let rule_id = template.id.clone();
    btn_test.set_callback(move |_| {
        use fltk::dialog::input_default;
        let test_file = match input_default("Enter filename to test:", "example.xml") {
            Some(s) if !s.is_empty() => s,
            _ => return,
        };

        let test_rule = WatchRule {
            id: rule_id.clone(),
            name: inp_name_c.value(),
            enabled: true,
            path: PathBuf::from(inp_path_c.value()),
            include_subdirectories: chk_subdirs_c.is_checked(),
            file_mask: inp_mask_c.value(),
            regex: {
                let v = inp_regex_c.value();
                if v.trim().is_empty() { None } else { Some(v) }
            },
            action: ActionTemplate {
                executable: PathBuf::new(),
                arguments: vec![],
            },
            debounce_ms: 1000,
            wait_until_stable: true,
            stable_check_ms: 300,
            stable_checks_count: 3,
            ready_timeout_sec: 30,
        };

        let test_path = PathBuf::from(inp_path_c.value()).join(&test_file);
        let candidate = FileCandidate {
            path: test_path,
            file_name: test_file.clone(),
            size: None,
        };

        let msg = match RuleMatcher::new(&test_rule) {
            Ok(matcher) => match matcher.decide(&test_rule, &candidate) {
                MatchDecision::Matched => format!("'{}' would MATCH this rule.", test_file),
                MatchDecision::Ignored { reason } => format!("'{}' would be IGNORED: {}", test_file, reason),
            },
            Err(e) => format!("Rule is invalid: {e}"),
        };
        fltk::dialog::message_default(&msg);
    });

    // --- Save button ---
    let inp_name_s = inp_name.clone();
    let inp_path_s = inp_path.clone();
    let inp_mask_s = inp_mask.clone();
    let inp_regex_s = inp_regex.clone();
    let chk_subdirs_s = chk_subdirs.clone();
    let inp_executable_s = inp_executable.clone();
    let inp_arguments_s = inp_arguments.clone();
    let inp_debounce_s = inp_debounce.clone();
    let chk_wait_stable_s = chk_wait_stable.clone();
    let inp_stable_ms_s = inp_stable_ms.clone();
    let inp_stable_count_s = inp_stable_count.clone();
    let inp_timeout_s = inp_timeout.clone();
    let chk_enabled_s = chk_enabled.clone();
    let result_s = Rc::clone(&result);
    let saved_id = template.id.clone();
    let mut win_s = win.clone();

    btn_save.set_callback(move |_| {
        let name = inp_name_s.value();
        if name.trim().is_empty() {
            fltk::dialog::message_default("Rule name cannot be empty.");
            return;
        }
        let mask = inp_mask_s.value();
        if mask.trim().is_empty() {
            fltk::dialog::message_default("File mask cannot be empty.");
            return;
        }

        let regex_str = inp_regex_s.value();
        let regex = if regex_str.trim().is_empty() {
            None
        } else {
            // Validate regex
            if let Err(e) = regex::Regex::new(&regex_str) {
                fltk::dialog::message_default(&format!("Invalid regex: {e}"));
                return;
            }
            Some(regex_str)
        };

        let rule = WatchRule {
            id: saved_id.clone(),
            name,
            enabled: chk_enabled_s.is_checked(),
            path: PathBuf::from(inp_path_s.value()),
            include_subdirectories: chk_subdirs_s.is_checked(),
            file_mask: mask,
            regex,
            action: ActionTemplate {
                executable: PathBuf::from(inp_executable_s.value()),
                arguments: inp_arguments_s.value()
                    .split_whitespace()
                    .map(|s| s.to_string())
                    .collect(),
            },
            debounce_ms: inp_debounce_s.value().parse().unwrap_or(1000),
            wait_until_stable: chk_wait_stable_s.is_checked(),
            stable_check_ms: inp_stable_ms_s.value().parse().unwrap_or(300),
            stable_checks_count: inp_stable_count_s.value().parse().unwrap_or(3),
            ready_timeout_sec: inp_timeout_s.value().parse().unwrap_or(30),
        };

        *result_s.borrow_mut() = Some(rule);
        win_s.hide();
    });

    // --- Cancel button ---
    let mut win_c = win.clone();
    btn_cancel.set_callback(move |_| {
        win_c.hide();
    });

    while win.shown() {
        app::wait();
    }

    Rc::try_unwrap(result).ok().and_then(|r| r.into_inner())
}

// ─── Main window ─────────────────────────────────────────────────────────────

fn main() {
    use tracing_subscriber::EnvFilter;
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();

    let fltk_app = app::App::default().with_scheme(app::Scheme::Gtk);

    // Determine config path
    let config_path = default_config_path()
        .unwrap_or_else(|| PathBuf::from("config.json"));

    let config = load_config(&config_path).unwrap_or_default();

    let state: Rc<RefCell<AppState>> = Rc::new(RefCell::new(AppState::new(config, config_path)));

    // ── Main window layout ──────────────────────────────────────────────────
    let mut main_win = Window::new(100, 100, 900, 650, "F-Opener");
    main_win.make_resizable(true);

    // Top toolbar (button row)
    let mut toolbar = Pack::new(5, 5, 890, 32, "");
    toolbar.set_type(PackType::Horizontal);
    toolbar.set_spacing(6);

    let mut btn_start_all = Button::new(0, 0, 90, 32, "Start All");
    btn_start_all.set_color(Color::from_rgb(180, 220, 180));

    let mut btn_stop_all = Button::new(0, 0, 90, 32, "Stop All");
    btn_stop_all.set_color(Color::from_rgb(220, 180, 180));
    btn_stop_all.deactivate();

    let _sep1 = Frame::new(0, 0, 10, 32, "");

    let mut btn_add = Button::new(0, 0, 70, 32, "Add");
    let mut btn_edit = Button::new(0, 0, 70, 32, "Edit");
    let mut btn_delete = Button::new(0, 0, 70, 32, "Delete");

    let _sep2 = Frame::new(0, 0, 10, 32, "");

    let mut btn_reload = Button::new(0, 0, 110, 32, "Reload Config");

    toolbar.end();

    // Status label
    let mut status_frame = Frame::new(5, 42, 890, 20, "Status: Stopped");
    status_frame.set_align(Align::Left | Align::Inside);

    // Tile: top = rules table, bottom = log
    let tile = Tile::new(5, 67, 890, 578, "");

    // Rules table (top half)
    let mut rules_table = Table::new(5, 67, 890, 300, "");
    rules_table.set_rows(0);
    rules_table.set_cols(6);
    rules_table.set_col_header(true);
    rules_table.set_row_header(false);
    rules_table.set_col_resize(true);
    rules_table.set_row_height_all(22);

    // Column widths
    rules_table.set_col_width(0, 60);  // Enabled
    rules_table.set_col_width(1, 70);  // Status
    rules_table.set_col_width(2, 200); // Name
    rules_table.set_col_width(3, 220); // Folder
    rules_table.set_col_width(4, 100); // Mask
    rules_table.set_col_width(5, 180); // Regex

    // Log panel (bottom half)
    let log_buf = TextBuffer::default();
    let mut log_display = TextDisplay::new(5, 372, 890, 273, "");
    log_display.set_buffer(log_buf.clone());
    log_display.set_text_font(Font::Courier);
    log_display.set_text_size(12);
    log_display.wrap_mode(fltk::text::WrapMode::AtBounds, 0);

    tile.end();
    main_win.end();
    main_win.show();

    // ── Shared data for table drawing ───────────────────────────────────────
    // We use a separate Arc<Mutex<...>> for the table cell drawing since
    // fltk draw callbacks must be 'static.
    let table_data: Arc<Mutex<Vec<WatchRule>>> = Arc::new(Mutex::new(
        state.borrow().config.rules.clone()
    ));

    // Column headers
    let col_headers = ["Enabled", "Status", "Name", "Folder", "Mask", "Regex"];

    let table_data_draw = Arc::clone(&table_data);
    let running_flag: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    let running_flag_draw = Arc::clone(&running_flag);

    rules_table.draw_cell(move |t, ctx, row, col, x, y, w, h| {
        match ctx {
            TableContext::StartPage => {
                fltk::draw::set_font(Font::Helvetica, 13);
            }
            TableContext::ColHeader => {
                fltk::draw::push_clip(x, y, w, h);
                fltk::draw::draw_box(FrameType::ThinUpBox, x, y, w, h, Color::from_rgb(220, 220, 220));
                fltk::draw::set_draw_color(Color::Black);
                fltk::draw::set_font(Font::HelveticaBold, 13);
                if let Some(&header) = col_headers.get(col as usize) {
                    fltk::draw::draw_text2(header, x, y, w, h, Align::Center);
                }
                fltk::draw::pop_clip();
            }
            TableContext::Cell => {
                let data = table_data_draw.lock().unwrap();
                let rule = match data.get(row as usize) {
                    Some(r) => r,
                    None => return,
                };

                let is_selected = t.is_selected(row, col);
                let bg = if is_selected {
                    Color::from_rgb(100, 160, 220)
                } else if row % 2 == 0 {
                    Color::White
                } else {
                    Color::from_rgb(245, 245, 250)
                };

                fltk::draw::push_clip(x, y, w, h);
                fltk::draw::draw_box(FrameType::FlatBox, x, y, w, h, bg);
                fltk::draw::set_draw_color(if is_selected { Color::White } else { Color::Black });
                fltk::draw::set_font(Font::Helvetica, 13);

                let is_running = *running_flag_draw.lock().unwrap();
                let text = match col {
                    0 => if rule.enabled { "Yes" } else { "No" }.to_string(),
                    1 => if is_running && rule.enabled { "Running" } else { "Stopped" }.to_string(),
                    2 => rule.name.clone(),
                    3 => rule.path.to_string_lossy().to_string(),
                    4 => rule.file_mask.clone(),
                    5 => rule.regex.as_deref().unwrap_or("").to_string(),
                    _ => String::new(),
                };

                fltk::draw::draw_text2(&text, x + 3, y, w - 6, h, Align::Left | Align::Inside);
                fltk::draw::pop_clip();
            }
            _ => {}
        }
    });

    // Helper: refresh the table row count from state
    let mut refresh_table = {
        let state_r = Rc::clone(&state);
        let table_data_r = Arc::clone(&table_data);
        let mut rules_table_r = rules_table.clone();
        move || {
            let rules = state_r.borrow().config.rules.clone();
            let count = rules.len() as i32;
            *table_data_r.lock().unwrap() = rules;
            rules_table_r.set_rows(count);
            rules_table_r.redraw();
        }
    };

    refresh_table();

    // Helper: append to log
    let _append_log = {
        let state_l = Rc::clone(&state);
        let mut log_buf_l = log_buf.clone();
        let mut log_disp_l = log_display.clone();
        move |msg: &str| {
            state_l.borrow_mut().push_log(msg);
            let lines = state_l.borrow().log_lines.clone();
            let text = lines.join("\n") + "\n";
            log_buf_l.set_text(&text);
            // Scroll to bottom
            let last = log_buf_l.length();
            log_disp_l.scroll(log_disp_l.count_lines(0, last, true), 0);
        }
    };

    // ── Start All ────────────────────────────────────────────────────────────
    let state_start = Rc::clone(&state);
    let running_flag_start = Arc::clone(&running_flag);
    let _table_data_start = Arc::clone(&table_data);
    let mut btn_stop_all_s = btn_stop_all.clone();
    let mut btn_start_all_s = btn_start_all.clone();
    let mut status_s = status_frame.clone();
    let mut rules_table_s = rules_table.clone();
    let mut log_buf_s = log_buf.clone();
    let mut log_disp_s = log_display.clone();
    let state_log_s = Rc::clone(&state);

    btn_start_all.set_callback(move |_| {
        let is_running = *running_flag_start.lock().unwrap();
        if is_running {
            return;
        }

        let (tx, rx) = std::sync::mpsc::channel::<AppEvent>();
        let runner = Arc::new(ProcessActionRunner);

        let config = state_start.borrow().config.clone();
        let active_rules: Vec<_> = config.rules.into_iter().filter(|r| r.enabled).collect();
        if active_rules.is_empty() {
            fltk::dialog::message_default("No enabled rules to watch.");
            return;
        }

        let engine = WatcherEngine::new(active_rules, runner, tx);
        match engine.run() {
            Ok(handle) => {
                state_start.borrow_mut().watcher_handle = Some(handle);
                state_start.borrow_mut().event_rx = Some(rx);
                *running_flag_start.lock().unwrap() = true;
                state_start.borrow_mut().is_running = true;
                status_s.set_label("Status: Running");
                btn_start_all_s.deactivate();
                btn_stop_all_s.activate();
                rules_table_s.redraw();

                // Notify
                state_log_s.borrow_mut().push_log("Watcher started.");
                let lines = state_log_s.borrow().log_lines.clone();
                log_buf_s.set_text(&(lines.join("\n") + "\n"));
                let last = log_buf_s.length();
                log_disp_s.scroll(log_disp_s.count_lines(0, last, true), 0);
            }
            Err(e) => {
                fltk::dialog::message_default(&format!("Failed to start watcher: {e}"));
            }
        }
    });

    // ── Stop All ─────────────────────────────────────────────────────────────
    let state_stop = Rc::clone(&state);
    let running_flag_stop = Arc::clone(&running_flag);
    let mut btn_start_all_stop = btn_start_all.clone();
    let mut btn_stop_all_stop = btn_stop_all.clone();
    let mut status_stop = status_frame.clone();
    let mut rules_table_stop = rules_table.clone();
    let mut log_buf_stop = log_buf.clone();
    let mut log_disp_stop = log_display.clone();
    let state_log_stop = Rc::clone(&state);

    btn_stop_all.set_callback(move |_| {
        let mut st = state_stop.borrow_mut();
        if let Some(handle) = st.watcher_handle.take() {
            handle.stop();
        }
        st.event_rx = None;
        st.is_running = false;
        *running_flag_stop.lock().unwrap() = false;
        status_stop.set_label("Status: Stopped");
        btn_start_all_stop.activate();
        btn_stop_all_stop.deactivate();
        rules_table_stop.redraw();
        drop(st);

        state_log_stop.borrow_mut().push_log("Watcher stopped.");
        let lines = state_log_stop.borrow().log_lines.clone();
        log_buf_stop.set_text(&(lines.join("\n") + "\n"));
        let last = log_buf_stop.length();
        log_disp_stop.scroll(log_disp_stop.count_lines(0, last, true), 0);
    });

    // ── Add ──────────────────────────────────────────────────────────────────
    let state_add = Rc::clone(&state);
    let table_data_add = Arc::clone(&table_data);
    let mut rules_table_add = rules_table.clone();
    btn_add.set_callback(move |_| {
        if let Some(rule) = show_rule_dialog(None) {
            state_add.borrow_mut().config.rules.push(rule);
            let rules = state_add.borrow().config.rules.clone();
            let count = rules.len() as i32;
            *table_data_add.lock().unwrap() = rules;
            rules_table_add.set_rows(count);
            rules_table_add.redraw();
            // Auto-save
            let st = state_add.borrow();
            let _ = save_config(&st.config_path, &st.config);
        }
    });

    // ── Edit ─────────────────────────────────────────────────────────────────
    let state_edit = Rc::clone(&state);
    let table_data_edit = Arc::clone(&table_data);
    let mut rules_table_edit = rules_table.clone();
    btn_edit.set_callback(move |_| {
        let row = rules_table_edit.callback_row();
        let n_rules = state_edit.borrow().config.rules.len() as i32;
        if row < 0 || row >= n_rules {
            fltk::dialog::message_default("Select a rule to edit.");
            return;
        }
        let existing = state_edit.borrow().config.rules[row as usize].clone();
        if let Some(updated) = show_rule_dialog(Some(&existing)) {
            state_edit.borrow_mut().config.rules[row as usize] = updated;
            let rules = state_edit.borrow().config.rules.clone();
            *table_data_edit.lock().unwrap() = rules;
            rules_table_edit.redraw();
            let st = state_edit.borrow();
            let _ = save_config(&st.config_path, &st.config);
        }
    });

    // ── Delete ───────────────────────────────────────────────────────────────
    let state_del = Rc::clone(&state);
    let table_data_del = Arc::clone(&table_data);
    let mut rules_table_del = rules_table.clone();
    btn_delete.set_callback(move |_| {
        let row = rules_table_del.callback_row();
        let n_rules = state_del.borrow().config.rules.len() as i32;
        if row < 0 || row >= n_rules {
            fltk::dialog::message_default("Select a rule to delete.");
            return;
        }
        let name = state_del.borrow().config.rules[row as usize].name.clone();
        let confirm = fltk::dialog::choice2_default(
            &format!("Delete rule '{name}'?"),
            "Cancel", "Delete", "",
        );
        if confirm == Some(1) {
            state_del.borrow_mut().config.rules.remove(row as usize);
            let rules = state_del.borrow().config.rules.clone();
            let count = rules.len() as i32;
            *table_data_del.lock().unwrap() = rules;
            rules_table_del.set_rows(count);
            rules_table_del.redraw();
            let st = state_del.borrow();
            let _ = save_config(&st.config_path, &st.config);
        }
    });

    // ── Reload Config ────────────────────────────────────────────────────────
    let state_reload = Rc::clone(&state);
    let table_data_reload = Arc::clone(&table_data);
    let mut rules_table_reload = rules_table.clone();
    btn_reload.set_callback(move |_| {
        let path = state_reload.borrow().config_path.clone();
        match load_config(&path) {
            Ok(cfg) => {
                let rules = cfg.rules.clone();
                let count = rules.len() as i32;
                state_reload.borrow_mut().config = cfg;
                *table_data_reload.lock().unwrap() = rules;
                rules_table_reload.set_rows(count);
                rules_table_reload.redraw();
                fltk::dialog::message_default("Config reloaded.");
            }
            Err(e) => {
                fltk::dialog::message_default(&format!("Failed to reload config: {e}"));
            }
        }
    });

    // ── Event polling timer ──────────────────────────────────────────────────
    // Poll every 100 ms for AppEvents from the watcher thread
    {
        let state_timer = Rc::clone(&state);
        let running_flag_timer = Arc::clone(&running_flag);
        let log_buf_timer = log_buf.clone();
        let log_disp_timer = log_display.clone();
        let rules_table_timer = rules_table.clone();

        fn poll_events(
            state: &Rc<RefCell<AppState>>,
            _running_flag: &Arc<Mutex<bool>>,
            log_buf: &mut TextBuffer,
            log_disp: &mut TextDisplay,
            rules_table: &mut Table,
        ) {
            let mut new_msgs: Vec<String> = Vec::new();

            // Drain up to 50 events per tick
            let has_rx = state.borrow().event_rx.is_some();
            if has_rx {
                let mut count = 0;
                loop {
                    if count > 50 { break; }
                    let event = {
                        let st = state.borrow();
                        if let Some(rx) = &st.event_rx {
                            rx.try_recv().ok()
                        } else {
                            None
                        }
                    };
                    match event {
                        Some(evt) => {
                            new_msgs.push(format_event(&evt));
                            count += 1;
                        }
                        None => break,
                    }
                }
            }

            if !new_msgs.is_empty() {
                {
                    let mut st = state.borrow_mut();
                    for msg in &new_msgs {
                        st.push_log(msg);
                    }
                }
                let lines = state.borrow().log_lines.clone();
                log_buf.set_text(&(lines.join("\n") + "\n"));
                let last = log_buf.length();
                log_disp.scroll(log_disp.count_lines(0, last, true), 0);
                rules_table.redraw();
            }
        }

        // We use a recursive closure approach with add_timeout3
        // Wrap everything in a shared struct so it can be called recursively
        use std::sync::atomic::{AtomicBool, Ordering};
        static TIMER_ACTIVE: AtomicBool = AtomicBool::new(false);
        TIMER_ACTIVE.store(true, Ordering::SeqCst);

        // We capture state in a closure that re-schedules itself
        let state_t = Rc::clone(&state_timer);
        let rf_t = Arc::clone(&running_flag_timer);
        let lb_t = log_buf_timer.clone();
        let ld_t = log_disp_timer.clone();
        let rt_t = rules_table_timer.clone();

        // fltk add_timeout3 takes a closure. We use a trick: Box the closure
        // and re-schedule inside. Note: we cannot easily do recursive closures
        // in Rust without a wrapper type. We use app::add_timeout3 with a
        // raw function pointer approach via a static Rc.

        // Simpler approach: use a plain function registered as a timeout
        // that captures state via thread_local.
        thread_local! {
            static TIMER_STATE: RefCell<Option<Rc<RefCell<AppState>>>> = RefCell::new(None);
            static TIMER_RF: RefCell<Option<Arc<Mutex<bool>>>> = RefCell::new(None);
            static TIMER_LB: RefCell<Option<TextBuffer>> = RefCell::new(None);
            static TIMER_LD: RefCell<Option<TextDisplay>> = RefCell::new(None);
            static TIMER_RT: RefCell<Option<Table>> = RefCell::new(None);
        }

        TIMER_STATE.with(|s| *s.borrow_mut() = Some(Rc::clone(&state_t)));
        TIMER_RF.with(|r| *r.borrow_mut() = Some(Arc::clone(&rf_t)));
        TIMER_LB.with(|l| *l.borrow_mut() = Some(lb_t.clone()));
        TIMER_LD.with(|l| *l.borrow_mut() = Some(ld_t.clone()));
        TIMER_RT.with(|r| *r.borrow_mut() = Some(rt_t.clone()));

        fn on_timer() {
            TIMER_STATE.with(|s| {
                TIMER_RF.with(|rf| {
                    TIMER_LB.with(|lb| {
                        TIMER_LD.with(|ld| {
                            TIMER_RT.with(|rt| {
                                let s_b = s.borrow();
                                let rf_b = rf.borrow();
                                let mut lb_b = lb.borrow_mut();
                                let mut ld_b = ld.borrow_mut();
                                let mut rt_b = rt.borrow_mut();
                                if let (Some(state), Some(running_flag), Some(log_buf), Some(log_disp), Some(rules_table)) =
                                    (s_b.as_ref(), rf_b.as_ref(), lb_b.as_mut(), ld_b.as_mut(), rt_b.as_mut())
                                {
                                    poll_events(state, running_flag, log_buf, log_disp, rules_table);
                                }
                            });
                        });
                    });
                });
            });

            app::add_timeout3(0.1, |_| on_timer());
        }

        app::add_timeout3(0.1, |_| on_timer());
    }

    fltk_app.run().unwrap();
}

fn format_event(event: &AppEvent) -> String {
    match event {
        AppEvent::RuleStarted { rule_id } => format!("[INFO] Rule started: {rule_id}"),
        AppEvent::RuleStopped { rule_id } => format!("[INFO] Rule stopped: {rule_id}"),
        AppEvent::FileDetected { rule_id, path } => {
            format!("[INFO] [{rule_id}] Detected: {}", path.display())
        }
        AppEvent::FileIgnored { rule_id, path, reason } => {
            format!("[DEBUG] [{rule_id}] Ignored {}: {reason}", path.display())
        }
        AppEvent::FileMatched { rule_id, path } => {
            format!("[INFO] [{rule_id}] Matched: {}", path.display())
        }
        AppEvent::FileReady { rule_id, path } => {
            format!("[INFO] [{rule_id}] File ready: {}", path.display())
        }
        AppEvent::ActionStarted { rule_id, executable, .. } => {
            format!("[INFO] [{rule_id}] Launching: {}", executable.display())
        }
        AppEvent::ActionCompleted { rule_id, path } => {
            format!("[INFO] [{rule_id}] Opened: {}", path.display())
        }
        AppEvent::ActionFailed { rule_id, path, error } => {
            format!("[ERROR] [{rule_id}] Failed to open {}: {error}", path.display())
        }
        AppEvent::Warning { message } => format!("[WARN] {message}"),
        AppEvent::Error { message } => format!("[ERROR] {message}"),
    }
}
