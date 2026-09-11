import { Check, Copy } from "lucide-react";
import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import { Link, useLocation } from "react-router";
import remarkGfm from "remark-gfm";
import {
	type HastNode,
	highlightCode,
	languageLabel,
	rehypeHeadingIds,
	remarkCodeMeta,
	resolveDocHref,
	textOf,
} from "./content";

/**
 * Scrolls to `#hash` (or the top) when the page changes. Pages render
 * client-side, and the API reference only once its spec has loaded, so the
 * browser's own jump to the hash fires before the target exists. Called by the
 * page rather than the layout so it runs after the page's content commits.
 */
export function useScrollOnNavigate(ready: boolean) {
	const { pathname, hash } = useLocation();
	const last = useRef<{ pathname: string; hash: string } | null>(null);

	useEffect(() => {
		if (!ready) return;
		const previous = last.current;
		if (previous?.pathname === pathname && previous.hash === hash) return;
		last.current = { pathname, hash };

		// Within a page the browser is already smooth-scrolling to the anchor.
		const samePage = previous?.pathname === pathname;
		const behavior: ScrollBehavior = samePage ? "smooth" : "instant";
		const target = hash
			? document.getElementById(decodeURIComponent(hash.slice(1)))
			: null;
		if (target) target.scrollIntoView({ behavior });
		else if (!samePage) window.scrollTo({ top: 0, behavior });
	}, [pathname, hash, ready]);
}

const LINK_CLASS =
	"font-medium text-blue-600 underline decoration-blue-200 underline-offset-2 transition-colors hover:text-blue-700 hover:decoration-blue-500";

const INLINE_CODE_CLASS =
	"rounded bg-gray-100 px-1.5 py-0.5 font-mono text-[0.85em] text-gray-900";

function CopyButton({ code }: { code: string }) {
	const [copied, setCopied] = useState(false);

	const onCopy = async () => {
		try {
			await navigator.clipboard.writeText(code);
			setCopied(true);
			setTimeout(() => setCopied(false), 1500);
		} catch (error) {
			// Denied or insecure context; the snippet is still selectable.
			console.error("Failed to copy snippet:", error);
		}
	};

	return (
		<button
			type="button"
			onClick={onCopy}
			className="flex items-center gap-1.5 rounded-md px-2 py-1 text-xs text-gray-400 transition-colors hover:bg-white/10 hover:text-gray-100 cursor-pointer"
		>
			{copied ? (
				<Check className="h-3.5 w-3.5" />
			) : (
				<Copy className="h-3.5 w-3.5" />
			)}
			{copied ? "Copied" : "Copy"}
		</button>
	);
}

export function CodeBlock({
	code,
	lang,
	title,
}: {
	code: string;
	lang: string;
	title?: string;
}) {
	const html = useMemo(() => highlightCode(code, lang), [code, lang]);

	return (
		<div className="my-5 overflow-hidden rounded-lg border border-gray-800 bg-gray-950">
			<div className="flex items-center gap-3 border-b border-white/10 py-1 pl-4 pr-1.5">
				<span className="min-w-0 truncate font-mono text-xs text-gray-400">
					{title ?? languageLabel(lang)}
				</span>
				<span className="ml-auto shrink-0">
					<CopyButton code={code} />
				</span>
			</div>
			<pre className="hljs-surface overflow-x-auto px-4 py-3 text-[13px] leading-relaxed">
				<code
					// biome-ignore lint/security/noDangerouslySetInnerHtml: highlight.js escapes the source; this is only its token markup
					dangerouslySetInnerHTML={{ __html: html }}
				/>
			</pre>
		</div>
	);
}

function Heading({
	level,
	id,
	children,
}: {
	level: 2 | 3;
	id?: string;
	children: ReactNode;
}) {
	const Tag = level === 2 ? "h2" : "h3";
	const className =
		level === 2
			? "group scroll-mt-20 mt-12 mb-3 text-xl font-semibold tracking-tight text-gray-900 first:mt-0"
			: "group scroll-mt-20 mt-8 mb-2 text-base font-semibold text-gray-900";

	// The anchor sits beside the text rather than around it, because a heading
	// may already contain a link and anchors can't nest.
	return (
		<Tag id={id} className={className}>
			{children}
			{id && (
				<a
					href={`#${id}`}
					aria-label="Link to this section"
					className="ml-2 font-normal text-gray-300 no-underline opacity-0 transition-opacity hover:text-blue-600 group-hover:opacity-100"
				>
					#
				</a>
			)}
		</Tag>
	);
}

function markdownComponents(slug: string): Components {
	return {
		h1: ({ children }) => (
			<h1 className="mt-12 mb-3 text-2xl font-bold tracking-tight text-gray-900">
				{children}
			</h1>
		),
		h2: ({ id, children }) => (
			<Heading level={2} id={id}>
				{children}
			</Heading>
		),
		h3: ({ id, children }) => (
			<Heading level={3} id={id}>
				{children}
			</Heading>
		),
		h4: ({ children }) => (
			<h4 className="mt-6 mb-2 text-sm font-semibold text-gray-900">
				{children}
			</h4>
		),
		p: ({ children }) => (
			<p className="my-4 text-[15px] leading-7 text-gray-700">{children}</p>
		),
		ul: ({ children }) => (
			<ul className="my-4 ml-5 list-outside list-disc space-y-1.5 marker:text-gray-400">
				{children}
			</ul>
		),
		ol: ({ children }) => (
			<ol className="my-4 ml-5 list-outside list-decimal space-y-1.5 marker:text-gray-400">
				{children}
			</ol>
		),
		li: ({ children }) => (
			<li className="pl-1 text-[15px] leading-7 text-gray-700 [&>p]:my-0">
				{children}
			</li>
		),
		a: ({ href = "", children }) => {
			const target = resolveDocHref(href, slug);
			if (target.startsWith("/")) {
				return (
					<Link to={target} className={LINK_CLASS}>
						{children}
					</Link>
				);
			}
			if (target.startsWith("#")) {
				return (
					<a href={target} className={LINK_CLASS}>
						{children}
					</a>
				);
			}
			return (
				<a
					href={target}
					target="_blank"
					rel="noopener noreferrer"
					className={LINK_CLASS}
				>
					{children}
				</a>
			);
		},
		strong: ({ children }) => (
			<strong className="font-semibold text-gray-900">{children}</strong>
		),
		blockquote: ({ children }) => (
			<div className="my-5 rounded-lg border border-blue-100 bg-blue-50/60 px-4 py-0.5 [&_p]:text-sm">
				{children}
			</div>
		),
		hr: () => <hr className="my-10 border-gray-200" />,
		table: ({ children }) => (
			<div className="my-5 overflow-x-auto rounded-lg border border-gray-200">
				<table className="min-w-full text-left">{children}</table>
			</div>
		),
		thead: ({ children }) => <thead className="bg-gray-50">{children}</thead>,
		th: ({ children }) => (
			<th className="px-4 py-2.5 text-xs font-semibold uppercase tracking-wide text-gray-500">
				{children}
			</th>
		),
		td: ({ children }) => (
			<td className="border-t border-gray-100 px-4 py-2.5 align-top text-sm text-gray-700">
				{children}
			</td>
		),
		img: ({ src, alt }) => (
			<img
				src={typeof src === "string" ? src : ""}
				alt={alt ?? ""}
				className="my-6 h-auto max-w-full rounded-lg"
			/>
		),
		// Fences and inline code both arrive here; only fences carry `data-lang`
		// (see `remarkCodeMeta`).
		code: ({ node, children }) => {
			const properties = (node as HastNode | undefined)?.properties;
			const lang = properties?.dataLang;
			if (typeof lang !== "string") {
				return <code className={INLINE_CODE_CLASS}>{children}</code>;
			}
			const title = properties?.dataTitle;
			return (
				<CodeBlock
					code={textOf(node as HastNode).replace(/\n$/, "")}
					lang={lang}
					title={typeof title === "string" ? title : undefined}
				/>
			);
		},
		// `CodeBlock` renders its own `pre`.
		pre: ({ children }) => <>{children}</>,
	};
}

export function DocsMarkdown({
	content,
	slug,
}: {
	content: string;
	slug: string;
}) {
	const components = useMemo(() => markdownComponents(slug), [slug]);

	return (
		<ReactMarkdown
			remarkPlugins={[remarkGfm, remarkCodeMeta]}
			rehypePlugins={[rehypeHeadingIds]}
			components={components}
		>
			{content}
		</ReactMarkdown>
	);
}

// Descriptions in the OpenAPI spec are Rust doc comments, i.e. markdown, but
// they sit in table cells and captions rather than prose.
const inlineComponents: Components = {
	p: ({ children }) => <p className="mt-1 first:mt-0">{children}</p>,
	code: ({ children }) => <code className={INLINE_CODE_CLASS}>{children}</code>,
	strong: ({ children }) => (
		<strong className="font-semibold text-gray-900">{children}</strong>
	),
	a: ({ href, children }) => (
		<a
			href={href}
			target="_blank"
			rel="noopener noreferrer"
			className={LINK_CLASS}
		>
			{children}
		</a>
	),
	ul: ({ children }) => (
		<ul className="mt-1 ml-4 list-outside list-disc">{children}</ul>
	),
	pre: ({ children }) => <>{children}</>,
};

export function InlineMarkdown({ children }: { children: string }) {
	return (
		<ReactMarkdown remarkPlugins={[remarkGfm]} components={inlineComponents}>
			{children}
		</ReactMarkdown>
	);
}
