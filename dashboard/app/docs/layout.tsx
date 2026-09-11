import { useQuery } from "@tanstack/react-query";
import { BADGE_COLORS } from "@teton/smith-ui";
import { Github, Menu, Star, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { Link, Outlet, useLocation } from "react-router";
import { type ApiReference, methodVariant, useApiReference } from "./api/spec";
import {
	API_REFERENCE_HREF,
	type DocHeading,
	docsNavigation,
	isApiReferencePath,
	readDoc,
	slugForPath,
} from "./content";

const REPOSITORY_URL = "https://github.com/Teton-ai/smith";
const REPOSITORY_API_URL = "https://api.github.com/repos/Teton-ai/smith";

const TABS = [
	{ title: "Docs", href: "/docs" },
	{ title: "API reference", href: API_REFERENCE_HREF },
];

const GROUP_TITLE_CLASS =
	"mb-2 px-3 text-xs font-semibold uppercase tracking-wider text-gray-500";

/**
 * The last heading the reader has scrolled past. A position check rather than
 * an IntersectionObserver: a short final section never reaches the middle of
 * the viewport, so an observer would never mark it.
 */
function useActiveHeading(ids: string[]): string | null {
	const [active, setActive] = useState<string | null>(null);
	// `ids` is a fresh array every render; the joined string is what changes.
	const key = ids.join("|");

	useEffect(() => {
		const headingIds = key ? key.split("|") : [];
		if (headingIds.length === 0) {
			setActive(null);
			return;
		}

		let frame = 0;
		const measure = () => {
			frame = 0;
			let current = headingIds[0];
			for (const id of headingIds) {
				const element = document.getElementById(id);
				// Reached just before it slides under the 56px header.
				if (element && element.getBoundingClientRect().top <= 96) current = id;
			}
			// The last sections of a page may never scroll up to the line.
			const atBottom =
				window.scrollY > 0 &&
				window.innerHeight + window.scrollY >=
					document.documentElement.scrollHeight - 8;
			setActive(atBottom ? headingIds[headingIds.length - 1] : current);
		};
		const onScroll = () => {
			if (!frame) frame = window.requestAnimationFrame(measure);
		};

		measure();
		window.addEventListener("scroll", onScroll, { passive: true });
		window.addEventListener("resize", onScroll);
		return () => {
			window.cancelAnimationFrame(frame);
			window.removeEventListener("scroll", onScroll);
			window.removeEventListener("resize", onScroll);
		};
	}, [key]);

	return active;
}

function GuideNav({
	pathname,
	headings,
	onNavigate,
}: {
	pathname: string;
	headings: DocHeading[];
	onNavigate?: () => void;
}) {
	const sections = headings.filter((heading) => heading.level === 2);
	const activeId = useActiveHeading(sections.map((section) => section.id));

	return (
		<nav className="space-y-6">
			{docsNavigation.map((group) => (
				<div key={group.title}>
					<h2 className={GROUP_TITLE_CLASS}>{group.title}</h2>
					<ul className="space-y-0.5">
						{group.links.map((link) => {
							const active = link.href === pathname;
							return (
								<li key={link.href}>
									<Link
										to={link.href}
										onClick={onNavigate}
										aria-current={active ? "page" : undefined}
										className={`relative flex h-8 items-center rounded-md px-3 text-sm transition-colors ${
											active
												? "bg-blue-50 font-medium text-blue-700"
												: "text-gray-600 hover:bg-gray-100 hover:text-gray-900"
										}`}
									>
										{active && (
											<span className="absolute top-1.5 bottom-1.5 left-0 w-[3px] rounded-r-full bg-blue-600" />
										)}
										<span className="truncate">{link.title}</span>
									</Link>
									{active && sections.length > 0 && (
										<ul className="mt-1 mb-2 ml-3 border-l border-gray-200">
											{sections.map((section) => (
												<li key={section.id}>
													<a
														href={`#${section.id}`}
														onClick={onNavigate}
														className={`-ml-px block truncate border-l py-1 pl-3 text-[13px] transition-colors ${
															section.id === activeId
																? "border-blue-600 font-medium text-blue-700"
																: "border-transparent text-gray-500 hover:text-gray-900"
														}`}
													>
														{section.title}
													</a>
												</li>
											))}
										</ul>
									)}
								</li>
							);
						})}
					</ul>
				</div>
			))}
		</nav>
	);
}

function ApiNav({
	reference,
	onNavigate,
}: {
	reference?: ApiReference;
	onNavigate?: () => void;
}) {
	const ids = useMemo(
		() =>
			reference?.tags.flatMap((tag) =>
				tag.operations.map((operation) => operation.anchor),
			) ?? [],
		[reference],
	);
	const activeId = useActiveHeading(ids);

	if (!reference) {
		return (
			<div className="animate-pulse space-y-3 px-3">
				{[0, 1, 2, 3, 4, 5].map((index) => (
					<div key={index} className="h-4 rounded bg-gray-200" />
				))}
			</div>
		);
	}

	return (
		<nav className="space-y-6">
			{reference.tags.map((tag) => (
				<div key={tag.anchor}>
					<h2 className={GROUP_TITLE_CLASS}>
						<a
							href={`#${tag.anchor}`}
							onClick={onNavigate}
							className="hover:text-gray-900"
						>
							{tag.title}
						</a>
					</h2>
					<ul className="space-y-0.5">
						{tag.operations.map((operation) => (
							<li key={operation.anchor}>
								<a
									href={`#${operation.anchor}`}
									onClick={onNavigate}
									title={`${operation.method.toUpperCase()} ${operation.path}`}
									className={`flex items-center gap-2 rounded-md px-3 py-1 text-[13px] transition-colors ${
										operation.anchor === activeId
											? "bg-blue-50 text-blue-700"
											: "text-gray-600 hover:bg-gray-100 hover:text-gray-900"
									}`}
								>
									<span
										className={`w-12 shrink-0 rounded px-1 text-center font-mono text-[10px] font-semibold uppercase leading-4 ${
											BADGE_COLORS[methodVariant(operation.method)]
										}`}
									>
										{operation.method}
									</span>
									<span className="truncate">{operation.summary}</span>
								</a>
							</li>
						))}
					</ul>
				</div>
			))}
		</nav>
	);
}

function DocsToc({ headings }: { headings: DocHeading[] }) {
	const activeId = useActiveHeading(headings.map((heading) => heading.id));
	if (headings.length < 2) return null;

	return (
		<nav className="sticky top-24 max-h-[calc(100vh-7rem)] w-56 shrink-0 overflow-y-auto">
			<h2 className="mb-2 text-xs font-semibold uppercase tracking-wider text-gray-500">
				On this page
			</h2>
			<ul className="border-l border-gray-200">
				{headings.map((heading) => (
					<li key={heading.id}>
						<a
							href={`#${heading.id}`}
							className={`-ml-px block border-l py-1 text-[13px] leading-snug transition-colors ${
								heading.level === 3 ? "pl-6" : "pl-3"
							} ${
								heading.id === activeId
									? "border-blue-600 font-medium text-blue-700"
									: "border-transparent text-gray-500 hover:text-gray-900"
							}`}
						>
							{heading.title}
						</a>
					</li>
				))}
			</ul>
		</nav>
	);
}

function GitHubStars() {
	const { data: stars } = useQuery({
		queryKey: ["docs", "github-stars"],
		queryFn: async ({ signal }) => {
			const response = await fetch(REPOSITORY_API_URL, { signal });
			if (!response.ok) {
				throw new Error(`${response.status} ${response.statusText}`);
			}
			const repository = (await response.json()) as {
				stargazers_count: number;
			};
			return repository.stargazers_count;
		},
		// Unauthenticated GitHub API calls are limited to 60 an hour per IP.
		staleTime: 60 * 60 * 1000,
		refetchOnWindowFocus: false,
		retry: false,
	});

	return (
		<a
			href={REPOSITORY_URL}
			target="_blank"
			rel="noopener noreferrer"
			aria-label={
				stars === undefined
					? "Smith on GitHub"
					: `Star Smith on GitHub (${stars} stars)`
			}
			className="flex h-8 items-center overflow-hidden rounded-md border border-gray-200 text-sm font-medium text-gray-700 transition-colors hover:border-gray-300 hover:text-gray-900"
		>
			<span className="flex h-full items-center gap-1.5 bg-gray-50 px-2.5">
				<Github className="h-4 w-4" />
				<span className="hidden sm:inline">Star</span>
			</span>
			{stars !== undefined && (
				<span className="flex h-full items-center gap-1 border-l border-gray-200 px-2.5 tabular-nums">
					<Star className="h-3.5 w-3.5 text-gray-400" />
					{new Intl.NumberFormat("en", { notation: "compact" }).format(stars)}
				</span>
			)}
		</a>
	);
}

/**
 * Public on purpose, like `/components`: docs you have to log in to read are
 * docs nobody can link to.
 */
export default function DocsLayout() {
	const { pathname } = useLocation();
	const isApi = isApiReferencePath(pathname);
	const { data: reference } = useApiReference(isApi);
	const doc = isApi ? undefined : readDoc(slugForPath(pathname));
	const [menuOpen, setMenuOpen] = useState(false);

	const headings = useMemo<DocHeading[]>(() => {
		if (!isApi) return doc?.headings ?? [];
		return (
			reference?.tags.map((tag) => ({
				id: tag.anchor,
				title: tag.title,
				level: 2,
			})) ?? []
		);
	}, [isApi, doc, reference]);

	const title = isApi ? "API reference" : doc?.title;
	useEffect(() => {
		document.title = title ? `${title} · Smith Docs` : "Smith Docs";
		return () => {
			document.title = "Smith";
		};
	}, [title]);

	const nav = isApi ? (
		<ApiNav reference={reference} onNavigate={() => setMenuOpen(false)} />
	) : (
		<GuideNav
			pathname={pathname}
			headings={headings}
			onNavigate={() => setMenuOpen(false)}
		/>
	);

	return (
		<div data-docs className="min-h-screen bg-white text-gray-900">
			<header className="fixed inset-x-0 top-0 z-40 h-14 border-b border-gray-200 bg-white/90 backdrop-blur">
				<div className="flex h-full items-center gap-3 px-4 lg:px-6">
					<button
						type="button"
						onClick={() => setMenuOpen((open) => !open)}
						aria-label={menuOpen ? "Close navigation" : "Open navigation"}
						aria-expanded={menuOpen}
						className="-ml-1 rounded-md p-2 text-gray-500 hover:bg-gray-100 hover:text-gray-900 lg:hidden cursor-pointer"
					>
						{menuOpen ? (
							<X className="h-5 w-5" />
						) : (
							<Menu className="h-5 w-5" />
						)}
					</button>
					<Link
						to="/docs"
						onClick={() => setMenuOpen(false)}
						className="flex items-center gap-2 lg:w-58"
					>
						<img
							src="/logo.png"
							alt="Smith Logo"
							width={28}
							height={28}
							className="h-7 w-7 shrink-0 rounded-md"
						/>
						<span className="font-semibold">Smith</span>
						<span className="text-gray-400">Docs</span>
					</Link>
					<nav
						aria-label="Docs sections"
						className="hidden h-full items-stretch gap-6 sm:flex"
					>
						{TABS.map((tab) => {
							const current = (tab.href === API_REFERENCE_HREF) === isApi;
							return (
								<Link
									key={tab.href}
									to={tab.href}
									aria-current={current ? "page" : undefined}
									className={`flex items-center border-b-2 text-sm font-medium transition-colors ${
										current
											? "border-blue-500 text-blue-600"
											: "border-transparent text-gray-500 hover:text-gray-900"
									}`}
								>
									{tab.title}
								</Link>
							);
						})}
					</nav>
					<div className="ml-auto flex items-center">
						<GitHubStars />
					</div>
				</div>
			</header>

			{menuOpen && (
				<div className="fixed inset-x-0 top-14 bottom-0 z-30 overflow-y-auto bg-white px-3 py-5 lg:hidden">
					<nav
						aria-label="Docs sections"
						className="mb-6 flex gap-1 rounded-lg bg-gray-100 p-1"
					>
						{TABS.map((tab) => {
							const current = (tab.href === API_REFERENCE_HREF) === isApi;
							return (
								<Link
									key={tab.href}
									to={tab.href}
									onClick={() => setMenuOpen(false)}
									aria-current={current ? "page" : undefined}
									className={`flex-1 rounded-md px-3 py-1.5 text-center text-sm font-medium transition-colors ${
										current
											? "bg-white text-gray-900 shadow-sm"
											: "text-gray-500 hover:text-gray-900"
									}`}
								>
									{tab.title}
								</Link>
							);
						})}
					</nav>
					{nav}
				</div>
			)}

			<aside className="fixed top-14 bottom-0 left-0 hidden w-64 overflow-y-auto border-r border-gray-200 bg-gray-50/50 px-3 py-6 lg:block">
				{nav}
			</aside>

			<main className="pt-14 lg:pl-64">
				<div
					className={`mx-auto flex gap-12 px-4 py-10 sm:px-6 lg:px-10 ${
						isApi ? "max-w-6xl" : "max-w-5xl"
					}`}
				>
					<div className={`min-w-0 flex-1 ${isApi ? "" : "max-w-3xl"}`}>
						<Outlet />
					</div>
					<div className="hidden xl:block">
						<DocsToc headings={headings} />
					</div>
				</div>
			</main>
		</div>
	);
}
