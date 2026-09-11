import { Button } from "@teton/smith-ui";
import { ArrowLeft, ArrowRight } from "lucide-react";
import { Link, Navigate, useLocation } from "react-router";
import {
	type DocsNavLink,
	docsNeighbours,
	readDoc,
	slugForPath,
} from "./content";
import { DocsMarkdown, useScrollOnNavigate } from "./markdown";

function PagerLink({
	link,
	direction,
}: {
	link: DocsNavLink;
	direction: "previous" | "next";
}) {
	const next = direction === "next";
	return (
		<Link
			to={link.href}
			className={`group rounded-lg border border-gray-200 px-4 py-3 transition-colors hover:border-blue-300 hover:bg-blue-50/40 ${
				next ? "text-right sm:col-start-2" : ""
			}`}
		>
			<span
				className={`flex items-center gap-1 text-xs text-gray-500 ${next ? "justify-end" : ""}`}
			>
				{!next && <ArrowLeft className="h-3 w-3" />}
				{next ? "Next" : "Previous"}
				{next && <ArrowRight className="h-3 w-3" />}
			</span>
			<span className="mt-0.5 block text-sm font-medium text-gray-900 group-hover:text-blue-700">
				{link.title}
			</span>
		</Link>
	);
}

export default function DocsPage() {
	const { pathname, hash } = useLocation();
	const doc = readDoc(slugForPath(pathname));
	useScrollOnNavigate(true);

	if (!doc) {
		return (
			<div className="py-20 text-center">
				<p className="text-sm font-semibold text-blue-600">404</p>
				<h1 className="mt-2 text-2xl font-bold tracking-tight text-gray-900">
					Page not found
				</h1>
				<p className="mt-2 text-gray-500">
					There is no docs page at{" "}
					<code className="font-mono text-sm">{pathname}</code>.
				</p>
				<Button
					to="/docs"
					className="mt-6"
					icon={<ArrowLeft className="h-4 w-4" />}
				>
					Back to the docs
				</Button>
			</div>
		);
	}

	// `/docs/introduction` and trailing slashes resolve to a file too; keep one
	// URL per page so the sidebar can match it.
	if (doc.href !== pathname) {
		return <Navigate to={`${doc.href}${hash}`} replace />;
	}

	const { previous, next } = docsNeighbours(doc.href);

	return (
		<article>
			<header className="mb-8">
				<h1 className="text-3xl font-bold tracking-tight text-gray-900">
					{doc.title}
				</h1>
				{doc.description && (
					<p className="mt-3 text-lg leading-relaxed text-gray-500">
						{doc.description}
					</p>
				)}
			</header>

			<DocsMarkdown content={doc.content} slug={doc.slug} />

			{(previous || next) && (
				<nav className="mt-16 grid gap-3 border-t border-gray-200 pt-6 sm:grid-cols-2">
					{previous && <PagerLink link={previous} direction="previous" />}
					{next && <PagerLink link={next} direction="next" />}
				</nav>
			)}
		</article>
	);
}
