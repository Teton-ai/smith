//! The CLI's reference as data, rendered by the dashboard at `/docs/cli`.
//!
//! Every command, flag and description already exists as a doc comment on the
//! clap definitions in `cli.rs`, so the docs read them from here instead of
//! keeping a second copy that goes stale. Written by a test rather than a
//! subcommand so nothing about the docs ships in the binary: `cargo test`
//! rewrites `cli/reference.json` whenever it no longer matches, and fails so the
//! change gets committed.

use clap::{Arg, ArgAction, Command};
use serde::Serialize;

#[derive(Serialize)]
pub struct Reference {
    name: String,
    version: String,
    about: String,
    /// Options every command accepts, listed once rather than under each.
    global_options: Vec<Opt>,
    commands: Vec<Cmd>,
}

#[derive(Serialize)]
struct Cmd {
    /// The full invocation without options, e.g. `sm get device`.
    path: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    aliases: Vec<String>,
    about: String,
    /// The doc comment after its first paragraph, which is `about`.
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    usage: String,
    arguments: Vec<Opt>,
    options: Vec<Opt>,
    examples: Vec<String>,
}

#[derive(Serialize)]
struct Opt {
    /// `--label`, or the value name for a positional.
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    short: Option<String>,
    /// `<KEY=VALUE>`, absent for a flag that takes no value.
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<String>,
    required: bool,
    help: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    env: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default: Option<String>,
}

/// The heading `after_long_help` opens with when a command has examples. Lines
/// under it are the examples, one invocation each; a `#` line is a comment.
const EXAMPLES: &str = "Examples:";

pub fn reference(mut cli: Command) -> Reference {
    cli.build();
    let global_options = cli
        .get_arguments()
        .filter(|arg| arg.is_global_set() && documented(arg))
        .map(opt)
        .collect();

    let mut commands = Vec::new();
    collect(&cli, &mut commands);

    Reference {
        name: cli.get_name().to_string(),
        version: cli.get_version().unwrap_or_default().to_string(),
        about: text(cli.get_about()),
        global_options,
        commands,
    }
}

/// Leaves only: `sm get` does nothing on its own, so the page lists
/// `sm get device` and not the group above it.
fn collect(parent: &Command, out: &mut Vec<Cmd>) {
    for sub in parent.get_subcommands() {
        if sub.is_hide_set() || sub.get_name() == "help" {
            continue;
        }
        if sub.has_subcommands() {
            collect(sub, out);
            continue;
        }

        let mut sub = sub.clone();
        let usage = sub.render_usage().to_string();
        let usage = usage.trim_start_matches("Usage:").trim().to_string();
        let path = sub
            .get_bin_name()
            .unwrap_or_else(|| sub.get_name())
            .to_string();

        // clap's long about repeats the short one as its first paragraph.
        let description = sub
            .get_long_about()
            .map(|about| about.to_string())
            .and_then(|long| long.split_once("\n\n").map(|(_, rest)| rest.to_string()));
        let (arguments, options) = sub
            .get_arguments()
            .filter(|arg| !arg.is_global_set() && documented(arg))
            .partition::<Vec<_>, _>(|arg| arg.is_positional());

        out.push(Cmd {
            path,
            aliases: sub.get_visible_aliases().map(String::from).collect(),
            about: text(sub.get_about()),
            description,
            usage,
            arguments: arguments.into_iter().map(opt).collect(),
            options: options.into_iter().map(opt).collect(),
            examples: examples(&sub),
        });
    }
}

/// clap's own `--help` and `--version` are on every command and say nothing a
/// reader needs a table for.
fn documented(arg: &Arg) -> bool {
    !arg.is_hide_set()
        && !matches!(
            arg.get_action(),
            ArgAction::Help | ArgAction::HelpShort | ArgAction::HelpLong | ArgAction::Version
        )
}

fn opt(arg: &Arg) -> Opt {
    let value = arg.get_action().takes_values().then(|| {
        arg.get_value_names()
            .map(|names| {
                names
                    .iter()
                    .map(|name| format!("<{name}>"))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_else(|| format!("<{}>", arg.get_id().as_str().to_uppercase()))
    });
    let defaults: Vec<_> = arg
        .get_default_values()
        .iter()
        .map(|value| value.to_string_lossy().into_owned())
        .collect();

    Opt {
        name: match arg.get_long() {
            Some(long) => format!("--{long}"),
            None => value
                .clone()
                .unwrap_or_else(|| arg.get_id().as_str().to_string()),
        },
        short: arg.get_short().map(|short| format!("-{short}")),
        value: if arg.is_positional() { None } else { value },
        required: arg.is_required_set(),
        help: text(arg.get_long_help().or(arg.get_help())),
        env: arg.get_env().map(|env| env.to_string_lossy().into_owned()),
        // A flag's implicit `false` is noise; a default only means something
        // when the option takes a value.
        default: (arg.get_action().takes_values() && !defaults.is_empty())
            .then(|| defaults.join(", ")),
    }
}

fn examples(cmd: &Command) -> Vec<String> {
    let Some(after) = cmd.get_after_long_help() else {
        return Vec::new();
    };
    let after = after.to_string();
    let Some((_, list)) = after.split_once(EXAMPLES) else {
        return Vec::new();
    };
    list.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect()
}

fn text(styled: Option<&clap::builder::StyledStr>) -> String {
    styled.map(|s| s.to_string()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use crate::cli::Cli;
    use clap::CommandFactory;

    /// Compared against the committed file rather than just written, so a
    /// stale reference fails the run and gets committed instead of left behind.
    #[test]
    fn reference_json_is_current() -> anyhow::Result<()> {
        let reference = super::reference(Cli::command());
        let json = format!("{}\n", serde_json::to_string_pretty(&reference)?);
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("reference.json");

        let current = std::fs::read_to_string(&path).unwrap_or_default();
        if current != json {
            std::fs::write(&path, &json)?;
            anyhow::bail!("cli/reference.json was stale and has been rewritten. Commit it.");
        }
        Ok(())
    }
}
