import {
	AlertBanner,
	Badge,
	type BadgeVariant,
	Button,
	Card,
} from "@teton/smith-ui";
import { ChevronDown, ChevronRight, Lock } from "lucide-react";
import { type ReactNode, useState } from "react";
import { CodeBlock, InlineMarkdown, useScrollOnNavigate } from "../markdown";
import {
	type ApiBody,
	type ApiField,
	type ApiOperation,
	type ApiResponse,
	methodVariant,
	useApiReference,
} from "./spec";

const AUTH_LABELS: Record<string, string> = {
	auth_token: "Bearer token",
	device_token: "Device token",
};

function statusVariant(status: string): BadgeVariant {
	if (status.startsWith("2")) return "green";
	if (status.startsWith("3")) return "gray";
	if (status.startsWith("4")) return "yellow";
	return "red";
}

function Label({ children }: { children: ReactNode }) {
	return (
		<h4 className="mt-6 mb-2 text-xs font-semibold uppercase tracking-wide text-gray-500">
			{children}
		</h4>
	);
}

function FieldTable({ fields }: { fields: ApiField[] }) {
	if (fields.length === 0) return null;

	return (
		<div className="overflow-x-auto rounded-lg border border-gray-200">
			<table className="min-w-full text-left">
				<thead className="bg-gray-50">
					<tr>
						{["Name", "Type", "Description"].map((heading) => (
							<th
								key={heading}
								className="px-4 py-2 text-xs font-semibold uppercase tracking-wide text-gray-500"
							>
								{heading}
							</th>
						))}
					</tr>
				</thead>
				<tbody>
					{fields.map((field) => (
						<tr key={field.name} className="border-t border-gray-100">
							<td className="whitespace-nowrap px-4 py-2.5 align-top">
								<code className="font-mono text-[13px] font-medium text-gray-900">
									{field.name}
								</code>
								{field.required && (
									<span className="mt-0.5 block text-[10px] font-semibold uppercase tracking-wide text-orange-600">
										Required
									</span>
								)}
							</td>
							<td className="max-w-64 break-words px-4 py-2.5 align-top font-mono text-xs text-gray-500">
								{field.type}
							</td>
							<td className="px-4 py-2.5 align-top text-sm leading-relaxed text-gray-600">
								{field.description ? (
									<InlineMarkdown>{field.description}</InlineMarkdown>
								) : (
									<span className="text-gray-300">—</span>
								)}
							</td>
						</tr>
					))}
				</tbody>
			</table>
		</div>
	);
}

function Body({ body }: { body: ApiBody }) {
	// Examples are collapsed: expanded, they double the length of a page that
	// already has a table per body.
	const [showExample, setShowExample] = useState(false);

	return (
		<div className="space-y-2">
			<p className="text-xs text-gray-500">
				{body.name !== body.contentType && (
					<>
						<code className="font-mono text-gray-700">{body.name}</code>
						{" · "}
					</>
				)}
				{body.contentType}
			</p>
			<FieldTable fields={body.fields} />
			{body.example && (
				<>
					<Button
						variant="ghost"
						size="sm"
						onClick={() => setShowExample((open) => !open)}
						icon={
							showExample ? (
								<ChevronDown className="h-3.5 w-3.5" />
							) : (
								<ChevronRight className="h-3.5 w-3.5" />
							)
						}
					>
						{showExample ? "Hide example" : "Show example"}
					</Button>
					{showExample && (
						<CodeBlock code={body.example} lang="json" title={body.name} />
					)}
				</>
			)}
		</div>
	);
}

function Response({ response }: { response: ApiResponse }) {
	return (
		<div className="border-t border-gray-100 py-3 first:border-t-0 first:pt-0">
			<div className="flex items-baseline gap-3">
				<Badge variant={statusVariant(response.status)} className="font-mono">
					{response.status}
				</Badge>
				<span className="text-sm text-gray-700">{response.description}</span>
			</div>
			{(response.headers.length > 0 || response.body) && (
				<div className="mt-3 space-y-3">
					{response.headers.length > 0 && (
						<FieldTable fields={response.headers} />
					)}
					{response.body && <Body body={response.body} />}
				</div>
			)}
		</div>
	);
}

function Operation({ operation }: { operation: ApiOperation }) {
	return (
		<section
			id={operation.anchor}
			className="scroll-mt-20 border-t border-gray-200 py-8"
		>
			<div className="flex flex-wrap items-center gap-x-3 gap-y-2">
				<Badge
					variant={methodVariant(operation.method)}
					className="font-mono uppercase"
				>
					{operation.method}
				</Badge>
				<h3 className="min-w-0 break-all font-mono text-sm font-semibold text-gray-900">
					<a href={`#${operation.anchor}`} className="hover:text-blue-700">
						{operation.path}
					</a>
				</h3>
				<span className="ml-auto inline-flex items-center gap-1 text-xs text-gray-500">
					{operation.auth.length > 0 ? (
						<>
							<Lock className="h-3 w-3" />
							{operation.auth
								.map((scheme) => AUTH_LABELS[scheme] ?? scheme)
								.join(" or ")}
						</>
					) : (
						"Public"
					)}
				</span>
			</div>

			<p className="mt-3 text-base font-medium text-gray-900">
				{operation.summary}
			</p>
			{operation.description && (
				<div className="mt-1 text-[15px] leading-7 text-gray-700">
					<InlineMarkdown>{operation.description}</InlineMarkdown>
				</div>
			)}

			{operation.pathParams.length > 0 && (
				<>
					<Label>Path parameters</Label>
					<FieldTable fields={operation.pathParams} />
				</>
			)}
			{operation.queryParams.length > 0 && (
				<>
					<Label>Query parameters</Label>
					<FieldTable fields={operation.queryParams} />
				</>
			)}
			{operation.headerParams.length > 0 && (
				<>
					<Label>Headers</Label>
					<FieldTable fields={operation.headerParams} />
				</>
			)}
			{operation.requestBody && (
				<>
					<Label>Request body</Label>
					<Body body={operation.requestBody} />
				</>
			)}
			{operation.responses.length > 0 && (
				<>
					<Label>Responses</Label>
					{operation.responses.map((response) => (
						<Response key={response.status} response={response} />
					))}
				</>
			)}
		</section>
	);
}

function InfoRow({ label, children }: { label: string; children: ReactNode }) {
	return (
		<div className="flex flex-col gap-1 px-5 py-3 sm:flex-row sm:gap-4">
			<dt className="w-32 shrink-0 text-xs font-semibold uppercase tracking-wide text-gray-500 sm:pt-0.5">
				{label}
			</dt>
			<dd className="min-w-0 text-sm leading-relaxed text-gray-700">
				{children}
			</dd>
		</div>
	);
}

const CODE_CLASS =
	"rounded bg-gray-100 px-1.5 py-0.5 font-mono text-[0.85em] text-gray-900";

const LINK_CLASS =
	"font-medium text-blue-600 underline decoration-blue-200 underline-offset-2 hover:text-blue-700 hover:decoration-blue-500";

export default function ApiReferencePage() {
	const {
		data: reference,
		error,
		refetch,
		isFetching,
		baseUrl,
	} = useApiReference();
	useScrollOnNavigate(reference !== undefined);

	const specUrl = `${baseUrl}/openapi.json`;

	let content: ReactNode;
	if (!baseUrl) {
		content = (
			<AlertBanner tone="amber" title="No API configured">
				Set <code className={CODE_CLASS}>API_BASE_URL</code> for the dashboard
				to load the API reference.
			</AlertBanner>
		);
	} else if (error) {
		content = (
			<AlertBanner
				title="Couldn't load the API spec"
				action={
					<Button
						size="sm"
						tone="gray"
						loading={isFetching}
						onClick={() => refetch()}
					>
						Retry
					</Button>
				}
			>
				Fetching <code className={CODE_CLASS}>{specUrl}</code> failed:{" "}
				{error.message}
			</AlertBanner>
		);
	} else if (!reference) {
		content = (
			<div className="animate-pulse space-y-4">
				{[0, 1, 2].map((index) => (
					<div key={index} className="h-32 rounded-lg bg-gray-100" />
				))}
			</div>
		);
	} else {
		content = reference.tags.map((tag) => (
			<section key={tag.anchor} className="mb-6">
				<h2
					id={tag.anchor}
					className="scroll-mt-20 pb-2 text-xl font-semibold tracking-tight text-gray-900"
				>
					{tag.title}
				</h2>
				{tag.operations.map((operation) => (
					<Operation key={operation.anchor} operation={operation} />
				))}
			</section>
		));
	}

	return (
		<article>
			<header className="mb-8">
				<div className="flex flex-wrap items-center gap-3">
					<h1 className="text-3xl font-bold tracking-tight text-gray-900">
						API reference
					</h1>
					{reference?.version && <Badge pill>v{reference.version}</Badge>}
				</div>
				<p className="mt-3 text-lg leading-relaxed text-gray-500">
					Every endpoint of the Smith API, generated from the OpenAPI spec
					served by the API this dashboard is connected to.
				</p>
			</header>

			{baseUrl && (
				<Card className="mb-10">
					<dl className="divide-y divide-gray-100">
						<InfoRow label="Base URL">
							<code className={`${CODE_CLASS} break-all`}>{baseUrl}</code>
						</InfoRow>
						<InfoRow label="Authentication">
							Send <code className={CODE_CLASS}>Authorization: Bearer</code>{" "}
							with your token. <code className={CODE_CLASS}>sm auth show</code>{" "}
							prints the token of your CLI session.
						</InfoRow>
						<InfoRow label="Spec">
							<a href={specUrl} className={LINK_CLASS}>
								openapi.json
							</a>{" "}
							to generate a client, or{" "}
							<a
								href={`${baseUrl}/docs`}
								target="_blank"
								rel="noopener noreferrer"
								className={LINK_CLASS}
							>
								Swagger UI
							</a>{" "}
							to try requests.
						</InfoRow>
					</dl>
				</Card>
			)}

			{content}
		</article>
	);
}
