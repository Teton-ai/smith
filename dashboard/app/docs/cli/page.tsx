import { Badge, Card } from "@teton/smith-ui";
import { Link } from "react-router";
import {
	CODE_CLASS,
	FieldTable,
	InfoRow,
	Label,
	LINK_CLASS,
} from "../api/page";
import type { ApiField } from "../api/spec";
import { CodeBlock, InlineMarkdown, useScrollOnNavigate } from "../markdown";
import {
	CLI_GLOBAL_OPTIONS_ANCHOR,
	CLI_INSTALL_ANCHOR,
	type CliCommand,
	type CliOption,
	cliReference,
} from "./reference";

const INSTALL_COMMAND = "curl -fsSL https://smith.teton.ai/install.sh | sh";

// The environment variable and default go in the description rather than
// columns of their own: most options have neither.
function optionField(option: CliOption): ApiField {
	const extras = [
		option.env && `Env: \`${option.env}\``,
		option.default && `Default: \`${option.default}\``,
	].filter(Boolean);

	return {
		name: option.short ? `${option.short}, ${option.name}` : option.name,
		type: option.value ?? "flag",
		required: option.required,
		description: [option.help, extras.join(" · ")].filter(Boolean).join("\n\n"),
	};
}

function SectionHeading({ id, children }: { id: string; children: string }) {
	return (
		<h2 className="text-xl font-semibold tracking-tight text-gray-900">
			<a href={`#${id}`} className="hover:text-blue-700">
				{children}
			</a>
		</h2>
	);
}

function Command({ command }: { command: CliCommand }) {
	return (
		<section
			id={command.anchor}
			className="scroll-mt-20 border-t border-gray-200 py-8"
		>
			<div className="flex flex-wrap items-center gap-x-3 gap-y-2">
				<h2 className="min-w-0 break-all font-mono text-base font-semibold text-gray-900">
					<a href={`#${command.anchor}`} className="hover:text-blue-700">
						{command.path}
					</a>
				</h2>
				{command.aliases.length > 0 && (
					<span className="flex flex-wrap items-center gap-1.5 text-xs text-gray-500">
						{command.aliases.length === 1 ? "Alias" : "Aliases"}
						{command.aliases.map((alias) => (
							<Badge key={alias} className="font-mono">
								{alias}
							</Badge>
						))}
					</span>
				)}
			</div>

			<p className="mt-3 text-base font-medium text-gray-900">
				{command.about}
			</p>
			{command.description && (
				<div className="mt-1 text-[15px] leading-7 text-gray-700">
					<InlineMarkdown>{command.description}</InlineMarkdown>
				</div>
			)}

			<Label>Usage</Label>
			<div className="overflow-x-auto whitespace-nowrap rounded-lg border border-gray-200 bg-gray-50 px-4 py-2.5 font-mono text-[13px] text-gray-900">
				{command.usage}
			</div>

			{command.arguments.length > 0 && (
				<>
					<Label>Arguments</Label>
					<FieldTable
						fields={command.arguments.map(optionField)}
						columns={["Argument"]}
					/>
				</>
			)}
			{command.options.length > 0 && (
				<>
					<Label>Options</Label>
					<FieldTable
						fields={command.options.map(optionField)}
						columns={["Option", "Value"]}
					/>
				</>
			)}
			{command.examples.length > 0 && (
				<>
					<Label>Examples</Label>
					<CodeBlock code={command.examples.join("\n")} lang="bash" />
				</>
			)}
		</section>
	);
}

/** Generated from `cli/reference.json`; see `./reference.ts`. */
export default function CliReferencePage() {
	useScrollOnNavigate(true);
	const { name, version, commands, globalOptions } = cliReference;

	return (
		<article>
			<header className="mb-8">
				<div className="flex flex-wrap items-center gap-3">
					<h1 className="text-3xl font-bold tracking-tight text-gray-900">
						CLI reference
					</h1>
					<Badge pill>v{version}</Badge>
				</div>
				<p className="mt-3 text-lg leading-relaxed text-gray-500">
					Every <code className={CODE_CLASS}>{name}</code> command, generated
					from the CLI's own definitions.{" "}
					<code className={CODE_CLASS}>{name} &lt;command&gt; --help</code>{" "}
					prints the same text.
				</p>
			</header>

			<Card className="mb-10">
				<dl className="divide-y divide-gray-100">
					<InfoRow label="Platforms">macOS and Linux, Intel and ARM</InfoRow>
					<InfoRow label="Authentication">
						<code className={CODE_CLASS}>{name} auth login</code> signs this
						machine in through your browser.{" "}
						<code className={CODE_CLASS}>{name} profile</code> switches between
						Smith instances.
					</InfoRow>
					<InfoRow label="Guide">
						<Link to="/docs/cli-usage" className={LINK_CLASS}>
							Using the CLI
						</Link>{" "}
						covers selecting devices, aliases and common workflows.
					</InfoRow>
				</dl>
			</Card>

			<section id={CLI_INSTALL_ANCHOR} className="scroll-mt-20 pb-8">
				<SectionHeading id={CLI_INSTALL_ANCHOR}>Install</SectionHeading>
				<p className="mt-3 text-[15px] leading-7 text-gray-700">
					The install script downloads the latest release for your platform into{" "}
					<code className={CODE_CLASS}>~/.smith/bin</code>. It needs{" "}
					<code className={CODE_CLASS}>unzip</code>; set{" "}
					<code className={CODE_CLASS}>SMITH_CLI_INSTALL</code> to install
					somewhere other than <code className={CODE_CLASS}>~/.smith</code>.
				</p>
				<CodeBlock code={INSTALL_COMMAND} lang="bash" />
				<p className="text-[15px] leading-7 text-gray-700">
					Then sign in and look at your fleet:
				</p>
				<CodeBlock code={`${name} auth login\n${name} get d`} lang="bash" />
			</section>

			{commands.map((command) => (
				<Command key={command.anchor} command={command} />
			))}

			{globalOptions.length > 0 && (
				<section
					id={CLI_GLOBAL_OPTIONS_ANCHOR}
					className="scroll-mt-20 border-t border-gray-200 py-8"
				>
					<SectionHeading id={CLI_GLOBAL_OPTIONS_ANCHOR}>
						Global options
					</SectionHeading>
					<p className="mt-3 mb-4 text-[15px] leading-7 text-gray-700">
						Accepted by every command.
					</p>
					<FieldTable
						fields={globalOptions.map(optionField)}
						columns={["Option", "Value"]}
					/>
				</section>
			)}
		</article>
	);
}
