// `cli/reference.json` is written by `cargo test` in cli/ from the clap
// definitions (see `cli/src/reference.rs`), so to change what `/docs/cli` says,
// edit the doc comments in `cli/src/cli.rs`, never this page. Bundled at build
// time like the markdown pages.
import raw from "../../../../cli/reference.json";

export interface CliOption {
	/** `--label`, or the value name of a positional argument. */
	name: string;
	short?: string;
	/** `<KEY=VALUE>`; absent for a flag that takes no value. */
	value?: string;
	required: boolean;
	/** Markdown — a Rust doc comment. */
	help: string;
	env?: string;
	default?: string;
}

export interface CliCommand {
	/** `sm get device`. */
	path: string;
	/** `sm get device` without the binary name, as the sidebar shows it. */
	title: string;
	anchor: string;
	aliases: string[];
	about: string;
	/** Markdown paragraphs after `about`. */
	description?: string;
	usage: string;
	arguments: CliOption[];
	options: CliOption[];
	/** One invocation per line; `#` lines are comments. */
	examples: string[];
}

interface RawCommand extends Omit<CliCommand, "title" | "anchor" | "aliases"> {
	aliases?: string[];
}

interface RawReference {
	name: string;
	version: string;
	about: string;
	global_options: CliOption[];
	commands: RawCommand[];
}

const reference = raw as RawReference;
const prefix = `${reference.name} `;

export const cliReference = {
	name: reference.name,
	version: reference.version,
	about: reference.about,
	globalOptions: reference.global_options,
	// Definition order, which is also `sm --help`'s order.
	commands: reference.commands.map((command): CliCommand => {
		const title = command.path.startsWith(prefix)
			? command.path.slice(prefix.length)
			: command.path;
		return {
			...command,
			aliases: command.aliases ?? [],
			title,
			anchor: title.replace(/\s+/g, "-"),
		};
	}),
};

export const CLI_INSTALL_ANCHOR = "install";
export const CLI_GLOBAL_OPTIONS_ANCHOR = "global-options";
