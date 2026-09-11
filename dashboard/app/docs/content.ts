import hljs from "highlight.js/lib/core";
import bash from "highlight.js/lib/languages/bash";
import ini from "highlight.js/lib/languages/ini";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import rust from "highlight.js/lib/languages/rust";
import sql from "highlight.js/lib/languages/sql";
import typescript from "highlight.js/lib/languages/typescript";
import yaml from "highlight.js/lib/languages/yaml";

// Every page under /docs is a markdown file in dashboard/docs, and its path is
// its URL (`cli/get.md` → `/docs/cli/get`). Bundled at build time so a page
// renders without a round trip, and so a broken file fails the build rather
// than a reader.
const files = import.meta.glob<string>("/docs/**/*.md", {
	query: "?raw",
	import: "default",
	eager: true,
});

export const DOCS_INDEX_SLUG = "introduction";
export const API_REFERENCE_HREF = "/docs/api";

export interface DocHeading {
	id: string;
	title: string;
	level: 2 | 3;
}

export interface Doc {
	slug: string;
	href: string;
	title: string;
	description?: string;
	content: string;
	headings: DocHeading[];
}

export function hrefForSlug(slug: string): string {
	return slug === DOCS_INDEX_SLUG ? "/docs" : `/docs/${slug}`;
}

export function slugForPath(pathname: string): string {
	return (
		pathname.replace(/^\/docs\/?/, "").replace(/\/+$/, "") || DOCS_INDEX_SLUG
	);
}

export function isApiReferencePath(pathname: string): boolean {
	return pathname.replace(/\/+$/, "") === API_REFERENCE_HREF;
}

export function slugify(text: string): string {
	return text
		.toLowerCase()
		.replace(/[^a-z0-9\s-]/g, "")
		.trim()
		.replace(/\s+/g, "-");
}

// CLI pages repeat "Usage" / "Options" under every subcommand, so ids are
// de-duplicated GitHub-style. The TOC (from the source) and the renderer (from
// the tree) each run their own slugger over the same headings in the same
// order, which is what keeps their anchors identical.
function createSlugger() {
	const seen = new Map<string, number>();
	return (text: string) => {
		const base = slugify(text);
		const count = seen.get(base) ?? 0;
		seen.set(base, count + 1);
		return count === 0 ? base : `${base}-${count}`;
	};
}

function extractHeadings(content: string): DocHeading[] {
	const slug = createSlugger();
	const headings: DocHeading[] = [];
	let fenced = false;

	for (const line of content.split("\n")) {
		if (/^\s*(```|~~~)/.test(line)) {
			fenced = !fenced;
			continue;
		}
		if (fenced) continue;

		const match = /^(#{2,3})\s+(.+?)\s*#*\s*$/.exec(line);
		if (!match) continue;

		const title = match[2]
			.replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
			.replace(/[`*]/g, "")
			.trim();
		headings.push({
			id: slug(title),
			title,
			level: match[1].length as 2 | 3,
		});
	}

	return headings;
}

// Frontmatter is flat `key: value` pairs; a YAML parser would be the largest
// dependency on the page to read two strings.
function parseFrontmatter(raw: string) {
	const match = /^---\r?\n([\s\S]*?)\r?\n---\r?\n?/.exec(raw);
	if (!match) return { data: {} as Record<string, string>, content: raw };

	const data: Record<string, string> = {};
	for (const line of match[1].split(/\r?\n/)) {
		const pair = /^([\w-]+):\s*(.*)$/.exec(line);
		if (pair) data[pair[1]] = pair[2].trim().replace(/^(["'])(.*)\1$/, "$2");
	}
	return { data, content: raw.slice(match[0].length) };
}

const docs = new Map<string, Doc>();
for (const [file, raw] of Object.entries(files)) {
	const slug = file.replace(/^\/docs\//, "").replace(/\.md$/, "");
	const { data, content } = parseFrontmatter(raw);
	docs.set(slug, {
		slug,
		href: hrefForSlug(slug),
		title: data.title ?? slug,
		description: data.description,
		content,
		headings: extractHeadings(content),
	});
}

export function readDoc(slug: string): Doc | undefined {
	return docs.get(slug);
}

/**
 * Pages link to each other with relative `.md` paths so the files also read
 * correctly on GitHub; this turns those into routes.
 */
export function resolveDocHref(href: string, fromSlug: string): string {
	if (/^[a-z][a-z0-9+.-]*:/i.test(href) || /^[#/]/.test(href)) return href;

	const [path, hash] = href.split("#");
	if (!path.endsWith(".md")) return href;

	const segments = fromSlug.split("/").slice(0, -1);
	for (const part of path.replace(/\.md$/, "").split("/")) {
		if (part === "..") segments.pop();
		else if (part !== "." && part !== "") segments.push(part);
	}
	const target = hrefForSlug(segments.join("/"));
	return hash ? `${target}#${hash}` : target;
}

// ── Navigation ───────────────────────────────────────────────────────────────

export interface DocsNavLink {
	title: string;
	href: string;
}

export interface DocsNavGroup {
	title: string;
	links: DocsNavLink[];
}

// Hand-ordered: the order pages should be read in is a writing decision. A
// file missing from here still renders at its URL, it just isn't advertised.
export const docsNavigation: DocsNavGroup[] = [
	{
		title: "Getting started",
		links: [
			{ title: "Introduction", href: "/docs" },
			{ title: "Install the CLI", href: "/docs/installation" },
			{ title: "Local development", href: "/docs/development" },
		],
	},
	{
		title: "Fleet",
		links: [{ title: "Deployments", href: "/docs/deployments" }],
	},
	{
		title: "CLI",
		links: [
			{ title: "Overview", href: "/docs/cli/overview" },
			{ title: "sm get", href: "/docs/cli/get" },
			{ title: "sm status", href: "/docs/cli/status" },
			{ title: "sm restart", href: "/docs/cli/restart" },
			{ title: "sm logs", href: "/docs/cli/logs" },
			{ title: "sm run", href: "/docs/cli/run" },
			{ title: "sm label", href: "/docs/cli/label" },
			{ title: "Other commands", href: "/docs/cli/other" },
		],
	},
	{
		title: "API",
		links: [
			{ title: "API reference", href: API_REFERENCE_HREF },
			{ title: "Integrations", href: "/docs/integrations" },
		],
	},
];

// The generated reference has its own navigation, so the prose pager skips it.
const readingOrder = docsNavigation
	.flatMap((group) => group.links)
	.filter((link) => link.href !== API_REFERENCE_HREF);

export function docsNeighbours(href: string): {
	previous?: DocsNavLink;
	next?: DocsNavLink;
} {
	const index = readingOrder.findIndex((link) => link.href === href);
	if (index === -1) return {};
	return { previous: readingOrder[index - 1], next: readingOrder[index + 1] };
}

// ── Markdown plugins ─────────────────────────────────────────────────────────

interface MdNode {
	type: string;
	lang?: string | null;
	meta?: string | null;
	data?: { hProperties?: Record<string, string> };
	children?: MdNode[];
}

/**
 * A fence's info string (```` ```toml title="magic.toml" ````) only exists in
 * the markdown tree; this copies the language and title onto the `<code>`
 * element, which is also how the renderer tells a block from inline code.
 */
export function remarkCodeMeta() {
	return (tree: MdNode) => {
		const walk = (node: MdNode) => {
			if (node.type === "code") {
				const title = /title="([^"]*)"/.exec(node.meta ?? "")?.[1];
				node.data = {
					...node.data,
					hProperties: {
						...node.data?.hProperties,
						dataLang: node.lang || "text",
						...(title ? { dataTitle: title } : {}),
					},
				};
			}
			node.children?.forEach(walk);
		};
		walk(tree);
	};
}

export interface HastNode {
	type: string;
	tagName?: string;
	value?: string;
	properties?: Record<string, unknown>;
	children?: HastNode[];
}

export function textOf(node: HastNode | undefined): string {
	if (!node) return "";
	if (node.type === "text") return node.value ?? "";
	return (node.children ?? []).map(textOf).join("");
}

export function rehypeHeadingIds() {
	return (tree: HastNode) => {
		const slug = createSlugger();
		const walk = (node: HastNode) => {
			if (
				node.type === "element" &&
				(node.tagName === "h2" || node.tagName === "h3")
			) {
				node.properties = { ...node.properties, id: slug(textOf(node)) };
			}
			node.children?.forEach(walk);
		};
		walk(tree);
	};
}

// ── Highlighting ─────────────────────────────────────────────────────────────

// Only the languages the docs use, so the docs chunk doesn't carry all of
// highlight.js. An unknown fence renders as plain text.
hljs.registerLanguage("bash", bash);
hljs.registerLanguage("ini", ini);
hljs.registerLanguage("javascript", javascript);
hljs.registerLanguage("json", json);
hljs.registerLanguage("rust", rust);
hljs.registerLanguage("sql", sql);
hljs.registerLanguage("typescript", typescript);
hljs.registerLanguage("yaml", yaml);

const LANGUAGE_ALIASES: Record<string, string> = {
	sh: "bash",
	shell: "bash",
	zsh: "bash",
	console: "bash",
	env: "ini",
	toml: "ini",
	js: "javascript",
	ts: "typescript",
	rs: "rust",
	yml: "yaml",
};

const LANGUAGE_LABELS: Record<string, string> = {
	bash: "Shell",
	sh: "Shell",
	shell: "Shell",
	zsh: "Shell",
	console: "Shell",
	env: ".env",
	ini: "INI",
	toml: "TOML",
	javascript: "JavaScript",
	js: "JavaScript",
	typescript: "TypeScript",
	ts: "TypeScript",
	json: "JSON",
	rust: "Rust",
	rs: "Rust",
	sql: "SQL",
	yaml: "YAML",
	yml: "YAML",
	text: "Text",
};

export function languageLabel(lang: string): string {
	return LANGUAGE_LABELS[lang] ?? lang.toUpperCase();
}

function escapeHtml(value: string): string {
	return value
		.replace(/&/g, "&amp;")
		.replace(/</g, "&lt;")
		.replace(/>/g, "&gt;");
}

export function highlightCode(code: string, lang: string): string {
	const language = LANGUAGE_ALIASES[lang] ?? lang;
	if (!hljs.getLanguage(language)) return escapeHtml(code);
	return hljs.highlight(code, { language, ignoreIllegals: true }).value;
}
