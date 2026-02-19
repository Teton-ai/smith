use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use clap::{CommandFactory, Parser};
use colored::Colorize;
use models::device::DeviceFilter;
use reedline::{
    ColumnarMenu, Completer, Emacs, FileBackedHistory, Highlighter, Hinter, History, KeyCode,
    KeyModifiers, MenuBuilder, Prompt, PromptEditMode, PromptHistorySearch,
    PromptHistorySearchStatus, Reedline, ReedlineEvent, ReedlineMenu, Signal, Span, StyledText,
    Suggestion,
};

use crate::api::SmithAPI;
use crate::auth;
use crate::config::Config;

// ── Prompt ──────────────────────────────────────────────────────────────────

struct SmithPrompt;

impl Prompt for SmithPrompt {
    fn render_prompt_left(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed("sm")
    }

    fn render_prompt_right(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed("")
    }

    fn render_prompt_indicator(&self, _edit_mode: PromptEditMode) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(" > ")
    }

    fn render_prompt_multiline_indicator(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed("... ")
    }

    fn render_prompt_history_search_indicator(
        &self,
        history_search: PromptHistorySearch,
    ) -> std::borrow::Cow<'_, str> {
        let prefix = match history_search.status {
            PromptHistorySearchStatus::Passing => "",
            PromptHistorySearchStatus::Failing => "failing ",
        };
        std::borrow::Cow::Owned(format!(
            "({}reverse-search: {}) ",
            prefix, history_search.term
        ))
    }
}

// ── Completion cache ────────────────────────────────────────────────────────

#[derive(Default, Clone)]
struct CompletionCache {
    device_serials: Vec<String>,
    device_hostnames: Vec<String>,
    label_keys: Vec<String>,
    label_values: HashMap<String, Vec<String>>,
    distribution_ids: Vec<String>,
}

// ── Build static command tree from clap ─────────────────────────────────────

fn collect_command_names(cmd: &clap::Command) -> Vec<(String, String)> {
    let mut result = Vec::new();
    for sub in cmd.get_subcommands() {
        if sub.is_hide_set() {
            continue;
        }
        let desc = sub.get_about().map(|a| a.to_string()).unwrap_or_default();
        result.push((sub.get_name().to_string(), desc.clone()));
        for alias in sub.get_visible_aliases() {
            result.push((alias.to_string(), desc.clone()));
        }
    }
    result
}

// ── Completer ───────────────────────────────────────────────────────────────

#[derive(Clone)]
struct SmithCompleter {
    clap_cmd: clap::Command,
    cache: Arc<Mutex<CompletionCache>>,
}

impl SmithCompleter {
    fn complete_at_position(&self, line: &str, pos: usize) -> Vec<Suggestion> {
        let line_to_cursor = &line[..pos];
        let parts: Vec<&str> = line_to_cursor.split_whitespace().collect();
        let trailing_space = line_to_cursor.ends_with(' ');
        let completing_word = if trailing_space || parts.is_empty() {
            ""
        } else {
            parts.last().copied().unwrap_or("")
        };
        let depth = if trailing_space {
            parts.len()
        } else {
            parts.len().saturating_sub(1)
        };

        // Walk down the clap command tree to find where we are
        let mut current_cmd = &self.clap_cmd;
        for part in parts.iter().take(depth.min(parts.len())) {
            if let Some(sub) = current_cmd
                .get_subcommands()
                .find(|s| s.get_name() == *part || s.get_visible_aliases().any(|a| a == *part))
            {
                current_cmd = sub;
            } else {
                break;
            }
        }

        let span_start = pos - completing_word.len();
        let span = Span::new(span_start, pos);

        // Check if we're in a position where dynamic completions apply
        if let Some(suggestions) = self.dynamic_completions(&parts, trailing_space, span) {
            return suggestions;
        }

        // Check if we should complete flags
        if completing_word.starts_with('-') {
            return self.flag_completions(current_cmd, completing_word, span);
        }

        // Otherwise, suggest subcommands
        let mut suggestions = Vec::new();
        for sub in current_cmd.get_subcommands() {
            if sub.is_hide_set() {
                continue;
            }
            let name = sub.get_name().to_string();
            let desc = sub.get_about().map(|a| a.to_string());
            if name.starts_with(completing_word) {
                suggestions.push(Suggestion {
                    value: name,
                    description: desc.clone(),
                    style: None,
                    extra: None,
                    span,
                    append_whitespace: true,
                    match_indices: None,
                });
            }
            for alias in sub.get_visible_aliases() {
                let alias = alias.to_string();
                if alias.starts_with(completing_word) {
                    suggestions.push(Suggestion {
                        value: alias,
                        description: desc.clone(),
                        style: None,
                        extra: None,
                        span,
                        append_whitespace: true,
                        match_indices: None,
                    });
                }
            }
        }
        suggestions
    }

    fn flag_completions(&self, cmd: &clap::Command, prefix: &str, span: Span) -> Vec<Suggestion> {
        let mut suggestions = Vec::new();
        for arg in cmd.get_arguments() {
            if let Some(long) = arg.get_long() {
                let flag = format!("--{}", long);
                if flag.starts_with(prefix) {
                    suggestions.push(Suggestion {
                        value: flag,
                        description: arg.get_help().map(|h| h.to_string()),
                        style: None,
                        extra: None,
                        span,
                        append_whitespace: true,
                        match_indices: None,
                    });
                }
            }
            if let Some(short) = arg.get_short() {
                let flag = format!("-{}", short);
                if flag.starts_with(prefix) {
                    suggestions.push(Suggestion {
                        value: flag,
                        description: arg.get_help().map(|h| h.to_string()),
                        style: None,
                        extra: None,
                        span,
                        append_whitespace: true,
                        match_indices: None,
                    });
                }
            }
        }
        suggestions
    }

    fn dynamic_completions(
        &self,
        parts: &[&str],
        trailing_space: bool,
        span: Span,
    ) -> Option<Vec<Suggestion>> {
        let cache = self.cache.lock().ok()?;

        // Determine the "resolved" command path (ignoring the last token if no trailing space)
        let cmd_parts: Vec<&str> = if trailing_space {
            parts.to_vec()
        } else if parts.len() > 1 {
            parts[..parts.len() - 1].to_vec()
        } else {
            return None;
        };

        let prefix = if trailing_space {
            ""
        } else {
            parts.last().copied().unwrap_or("")
        };

        // Commands that take device serial numbers as positional args
        let needs_device = matches!(
            cmd_parts.as_slice(),
            ["get", "device" | "devices" | "d"]
                | ["get", "commands" | "cmds"]
                | ["status", "device" | "devices" | "d"]
                | ["status", "service" | "services" | "svc"]
                | ["restart", "device" | "devices" | "d"]
                | ["restart", "service" | "services" | "svc"]
                | ["logs"]
                | ["test-network"]
                | ["tunnel"]
                | ["run"]
                | ["label"]
                | ["approve"]
                | ["revoke"]
        );

        if needs_device {
            let mut suggestions: Vec<Suggestion> = cache
                .device_serials
                .iter()
                .filter(|s| s.starts_with(prefix))
                .map(|s| Suggestion {
                    value: s.clone(),
                    description: Some("device".to_string()),
                    style: None,
                    extra: None,
                    span,
                    append_whitespace: true,
                    match_indices: None,
                })
                .collect();

            // Also offer hostnames
            for hostname in &cache.device_hostnames {
                if hostname.starts_with(prefix)
                    && !cache.device_serials.iter().any(|s| s == hostname)
                {
                    suggestions.push(Suggestion {
                        value: hostname.clone(),
                        description: Some("hostname".to_string()),
                        style: None,
                        extra: None,
                        span,
                        append_whitespace: true,
                        match_indices: None,
                    });
                }
            }

            if !suggestions.is_empty() {
                return Some(suggestions);
            }
        }

        // Label completions for --label / -l flag
        if parts.len() >= 2 {
            let prev = if trailing_space {
                parts.last().copied().unwrap_or("")
            } else if parts.len() >= 2 {
                parts[parts.len() - 2]
            } else {
                ""
            };

            if prev == "--label" || prev == "-l" {
                // Suggest label keys with = suffix
                let suggestions: Vec<Suggestion> = cache
                    .label_keys
                    .iter()
                    .filter(|k| k.starts_with(prefix))
                    .map(|k| Suggestion {
                        value: format!("{}=", k),
                        description: Some("label key".to_string()),
                        style: None,
                        extra: None,
                        span,
                        append_whitespace: false,
                        match_indices: None,
                    })
                    .collect();
                if !suggestions.is_empty() {
                    return Some(suggestions);
                }
            }

            // If currently typing a label value (contains =)
            if !trailing_space
                && let Some(current) = parts.last()
                && let Some((key, val_prefix)) = current.split_once('=')
                && let Some(values) = cache.label_values.get(key)
            {
                let suggestions: Vec<Suggestion> = values
                    .iter()
                    .filter(|v| v.starts_with(val_prefix))
                    .map(|v| Suggestion {
                        value: format!("{}={}", key, v),
                        description: Some("label value".to_string()),
                        style: None,
                        extra: None,
                        span,
                        append_whitespace: true,
                        match_indices: None,
                    })
                    .collect();
                if !suggestions.is_empty() {
                    return Some(suggestions);
                }
            }
        }

        // Releases commands — suggest distribution IDs
        let needs_distro = matches!(
            cmd_parts.as_slice(),
            ["releases", "get"] | ["releases", "deploy"] | ["releases", "publish"]
        );

        if needs_distro {
            let suggestions: Vec<Suggestion> = cache
                .distribution_ids
                .iter()
                .filter(|s| s.starts_with(prefix))
                .map(|s| Suggestion {
                    value: s.clone(),
                    description: Some("release".to_string()),
                    style: None,
                    extra: None,
                    span,
                    append_whitespace: true,
                    match_indices: None,
                })
                .collect();
            if !suggestions.is_empty() {
                return Some(suggestions);
            }
        }

        None
    }
}

impl Completer for SmithCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        self.complete_at_position(line, pos)
    }
}

// ── Highlighter ─────────────────────────────────────────────────────────────

#[derive(Clone)]
struct SmithHighlighter {
    known_commands: Vec<String>,
}

impl Highlighter for SmithHighlighter {
    fn highlight(&self, line: &str, _cursor: usize) -> StyledText {
        let mut styled = StyledText::new();
        let parts: Vec<&str> = line.splitn(2, ' ').collect();
        let first_word = parts.first().copied().unwrap_or("");
        let rest = if line.len() > first_word.len() {
            &line[first_word.len()..]
        } else {
            ""
        };

        let is_known = self.known_commands.iter().any(|c| c == first_word)
            || ["help", "exit", "quit", "clear"].contains(&first_word);

        if is_known {
            styled.push((
                nu_ansi_term::Style::new()
                    .fg(nu_ansi_term::Color::Green)
                    .bold(),
                first_word.to_string(),
            ));
        } else if !first_word.is_empty() {
            styled.push((
                nu_ansi_term::Style::new().fg(nu_ansi_term::Color::Red),
                first_word.to_string(),
            ));
        }

        if !rest.is_empty() {
            styled.push((nu_ansi_term::Style::default(), rest.to_string()));
        }

        styled
    }
}

// ── Hinter (ghost text for commands + history) ──────────────────────────────

#[derive(Clone)]
struct SmithHinter {
    command_names: Vec<String>,
    style: nu_ansi_term::Style,
    current_hint: String,
}

impl SmithHinter {
    fn new(command_names: Vec<String>) -> Self {
        Self {
            command_names,
            style: nu_ansi_term::Style::new().fg(nu_ansi_term::Color::DarkGray),
            current_hint: String::new(),
        }
    }
}

impl Hinter for SmithHinter {
    fn handle(
        &mut self,
        line: &str,
        _pos: usize,
        history: &dyn History,
        use_ansi_coloring: bool,
        _cwd: &str,
    ) -> String {
        self.current_hint = String::new();

        if line.is_empty() {
            return String::new();
        }

        // First try: match against command names (for the first word)
        let _first_word = line.split_whitespace().next().unwrap_or("");
        let is_single_word = !line.contains(' ');

        if is_single_word {
            for cmd in &self.command_names {
                if cmd.starts_with(line) && cmd != line {
                    let suffix = &cmd[line.len()..];
                    self.current_hint = suffix.to_string();
                    if use_ansi_coloring {
                        return self.style.paint(suffix).to_string();
                    }
                    return suffix.to_string();
                }
            }
        }

        // Fall back to history search
        let history_search = history
            .search(reedline::SearchQuery::last_with_prefix(
                line.to_string(),
                history.session(),
            ))
            .ok()
            .and_then(|results| results.into_iter().next());

        if let Some(entry) = history_search {
            let suggestion = entry.command_line;
            if suggestion.len() > line.len() {
                let suffix = &suggestion[line.len()..];
                self.current_hint = suffix.to_string();
                if use_ansi_coloring {
                    return self.style.paint(suffix).to_string();
                }
                return suffix.to_string();
            }
        }

        String::new()
    }

    fn complete_hint(&self) -> String {
        self.current_hint.clone()
    }

    fn next_hint_token(&self) -> String {
        let mut reached_content = false;
        let result: String = self
            .current_hint
            .chars()
            .take_while(|c| match (c.is_whitespace(), reached_content) {
                (true, true) => false,
                (true, false) => true,
                (false, _) => {
                    reached_content = true;
                    true
                }
            })
            .collect();
        result
    }
}

// ── Cache fetching ──────────────────────────────────────────────────────────

async fn refresh_cache(cache: &Arc<Mutex<CompletionCache>>, config: &Config) {
    let secrets = match auth::get_secrets(config).await {
        Ok(Some(s)) => s,
        _ => return,
    };

    let api = SmithAPI::new(secrets, config);

    // Fetch devices
    if let Ok(devices) = api.get_devices(DeviceFilter::default()).await {
        let serials: Vec<String> = devices.iter().map(|d| d.serial_number.clone()).collect();

        let mut label_keys = std::collections::HashSet::new();
        let mut label_values: HashMap<String, Vec<String>> = HashMap::new();

        for device in &devices {
            for (k, v) in device.labels.iter() {
                label_keys.insert(k.clone());
                label_values
                    .entry(k.clone())
                    .or_default()
                    .push(v.to_string());
            }
        }

        // Deduplicate label values
        for values in label_values.values_mut() {
            values.sort();
            values.dedup();
        }

        if let Ok(mut c) = cache.lock() {
            c.device_serials = serials;
            c.label_keys = label_keys.into_iter().collect();
            c.label_values = label_values;
        }
    }

    // Fetch distributions for release completions
    if let Ok(distros) = api.get_distributions().await
        && let Ok(mut c) = cache.lock()
    {
        c.distribution_ids = distros.iter().map(|d| d.id.to_string()).collect();
    }
}

// ── Help text ───────────────────────────────────────────────────────────────

fn print_repl_help(cmd: &clap::Command) {
    println!("{}", "Available commands:".bold());
    println!();

    let mut entries: Vec<(String, String)> = Vec::new();
    for sub in cmd.get_subcommands() {
        if sub.is_hide_set() {
            continue;
        }
        let name = sub.get_name().to_string();
        let desc = sub.get_about().map(|a| a.to_string()).unwrap_or_default();
        entries.push((name, desc));
    }

    // Add REPL-specific commands
    entries.push(("help".to_string(), "Show this help message".to_string()));
    entries.push(("clear".to_string(), "Clear the screen".to_string()));
    entries.push(("exit".to_string(), "Exit the REPL".to_string()));

    let max_name = entries.iter().map(|(n, _)| n.len()).max().unwrap_or(0);

    for (name, desc) in &entries {
        println!(
            "  {:<width$}  {}",
            name.bright_cyan(),
            desc.dimmed(),
            width = max_name
        );
    }
    println!();
    println!(
        "{}",
        "Press Tab for completions, Up/Down for history, Ctrl-R to search history.".dimmed()
    );
}

// ── REPL entry point ────────────────────────────────────────────────────────

pub async fn run_repl(config: &mut Config) -> anyhow::Result<()> {
    println!(
        "{} {} {}",
        "Smith CLI".bold(),
        env!("CARGO_PKG_VERSION"),
        "— Interactive Mode".dimmed()
    );
    println!(
        "{}",
        "Type 'help' for available commands, Tab for completions, 'exit' to quit.".dimmed()
    );
    println!();

    let clap_cmd = crate::cli::Cli::command();

    // Build known command names for the highlighter
    let known_commands: Vec<String> = collect_command_names(&clap_cmd)
        .into_iter()
        .map(|(name, _)| name)
        .collect();

    let cache = Arc::new(Mutex::new(CompletionCache::default()));

    // Spawn background cache refresh
    let cache_clone = Arc::clone(&cache);
    let config_for_cache = config.clone();
    tokio::spawn(async move {
        refresh_cache(&cache_clone, &config_for_cache).await;
    });

    let completer = SmithCompleter {
        clap_cmd: crate::cli::Cli::command(),
        cache: Arc::clone(&cache),
    };

    let highlighter = SmithHighlighter {
        known_commands: known_commands.clone(),
    };

    // Set up history
    let history_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".smith")
        .join("interactive_history");

    if let Some(parent) = history_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let history = FileBackedHistory::with_file(1000, history_path.clone())
        .map_err(|e| anyhow::anyhow!("Failed to create history file: {}", e))?;

    // Set up keybindings — start from defaults so backspace/arrows/etc. all work
    let mut keybindings = reedline::default_emacs_keybindings();
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu("completion_menu".to_string()),
            ReedlineEvent::MenuNext,
        ]),
    );
    let edit_mode = Emacs::new(keybindings);

    let completion_menu = ColumnarMenu::default()
        .with_name("completion_menu")
        .with_columns(4)
        .with_column_padding(2);

    let hinter = SmithHinter::new(known_commands.clone());

    let mut line_editor = Reedline::create()
        .with_completer(Box::new(completer))
        .with_menu(ReedlineMenu::EngineCompleter(Box::new(completion_menu)))
        .with_quick_completions(true)
        .with_partial_completions(true)
        .with_highlighter(Box::new(highlighter))
        .with_hinter(Box::new(hinter))
        .with_history(Box::new(history))
        .with_edit_mode(Box::new(edit_mode));

    let prompt = SmithPrompt;

    // Periodic cache refresh tracking
    let mut last_cache_refresh = std::time::Instant::now();

    loop {
        // Refresh cache every 60 seconds
        if last_cache_refresh.elapsed() > std::time::Duration::from_secs(60) {
            let cache_clone = Arc::clone(&cache);
            let config_for_cache = config.clone();
            tokio::spawn(async move {
                refresh_cache(&cache_clone, &config_for_cache).await;
            });
            last_cache_refresh = std::time::Instant::now();
        }

        let sig = line_editor.read_line(&prompt);

        match sig {
            Ok(Signal::Success(line)) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                match line {
                    "exit" | "quit" => {
                        println!("{}", "Goodbye!".dimmed());
                        break;
                    }
                    "clear" => {
                        // ANSI clear screen
                        print!("\x1b[2J\x1b[1;1H");
                        continue;
                    }
                    "help" => {
                        let cmd = crate::cli::Cli::command();
                        print_repl_help(&cmd);
                        continue;
                    }
                    _ => {}
                }

                // Parse and dispatch the command
                let args = std::iter::once("sm").chain(line.split_whitespace());
                match crate::cli::Cli::try_parse_from(args) {
                    Ok(cli) => {
                        if let Some(command) = cli.command {
                            if matches!(command, crate::cli::Commands::Interactive) {
                                println!("{}", "Already in interactive mode!".yellow());
                                continue;
                            }
                            if let Err(e) = crate::dispatch_command(command, config).await {
                                eprintln!("{}: {}", "Error".red().bold(), e);
                            }
                        } else {
                            let cmd = crate::cli::Cli::command();
                            print_repl_help(&cmd);
                        }
                    }
                    Err(e) => {
                        // clap errors include help text, version, etc.
                        let rendered = e.render().to_string();
                        eprintln!("{}", rendered);
                    }
                }
            }
            Ok(Signal::CtrlD) => {
                println!("{}", "Goodbye!".dimmed());
                break;
            }
            Ok(Signal::CtrlC) => {
                // Just cancel the current line, don't exit
                continue;
            }
            Err(e) => {
                eprintln!("{}: {}", "REPL error".red().bold(), e);
                break;
            }
        }
    }

    Ok(())
}
