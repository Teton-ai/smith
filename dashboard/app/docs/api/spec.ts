import { useQuery } from "@tanstack/react-query";
import type { BadgeVariant } from "@teton/smith-ui";
import { useConfig } from "@/app/hooks/config";
import { slugify } from "../content";

// The reference is rendered from the spec the running API serves about itself,
// so it always describes the API this dashboard talks to, and there is no
// second hand-written copy of the handlers' utoipa annotations to go stale.

interface RawSchema {
	type?: string | string[];
	format?: string;
	description?: string;
	required?: string[];
	properties?: Record<string, RawSchema>;
	additionalProperties?: boolean | RawSchema;
	items?: RawSchema;
	oneOf?: RawSchema[];
	anyOf?: RawSchema[];
	allOf?: RawSchema[];
	enum?: unknown[];
	example?: unknown;
	$ref?: string;
}

interface RawParameter {
	name: string;
	in: "path" | "query" | "header" | "cookie";
	description?: string;
	required?: boolean;
	schema?: RawSchema;
}

type RawContent = Record<string, { schema?: RawSchema }>;

interface RawOperation {
	tags?: string[];
	summary?: string;
	description?: string;
	operationId?: string;
	parameters?: RawParameter[];
	requestBody?: { content?: RawContent };
	responses?: Record<
		string,
		{
			description?: string;
			headers?: Record<string, { description?: string; schema?: RawSchema }>;
			content?: RawContent;
		}
	>;
	security?: Record<string, string[]>[];
}

interface RawSpec {
	info?: { version?: string };
	paths?: Record<string, Record<string, RawOperation>>;
	components?: { schemas?: Record<string, RawSchema> };
	security?: Record<string, string[]>[];
}

export interface ApiField {
	name: string;
	type: string;
	required: boolean;
	description?: string;
}

export interface ApiBody {
	/** Schema name (`Device`, `Release[]`) or the bare type. */
	name: string;
	contentType: string;
	fields: ApiField[];
	/** Pretty-printed JSON, only for JSON bodies. */
	example?: string;
}

export interface ApiResponse {
	status: string;
	description?: string;
	headers: ApiField[];
	body?: ApiBody;
}

export interface ApiOperation {
	method: string;
	path: string;
	anchor: string;
	summary: string;
	description?: string;
	/** Security scheme names; empty means the route is public. */
	auth: string[];
	pathParams: ApiField[];
	queryParams: ApiField[];
	headerParams: ApiField[];
	requestBody?: ApiBody;
	responses: ApiResponse[];
}

export interface ApiTag {
	title: string;
	anchor: string;
	operations: ApiOperation[];
}

export interface ApiReference {
	version?: string;
	tags: ApiTag[];
}

const METHOD_ORDER = [
	"get",
	"post",
	"put",
	"patch",
	"delete",
	"head",
	"options",
];

// Examples recurse into referenced schemas; this bounds self-referencing ones.
const MAX_EXAMPLE_DEPTH = 6;

const EXAMPLE_TIMESTAMP = "2026-01-01T12:00:00Z";

const ACRONYMS: Record<string, string> = {
	api: "API",
	id: "ID",
	ip: "IP",
	lts: "LTS",
	os: "OS",
	rc: "RC",
	sse: "SSE",
	url: "URL",
};

/** `get_devices` → "Get devices", `extended-network-test` → "Extended network test". */
function sentenceCase(identifier: string): string {
	const words = identifier
		.replace(/^api_/, "")
		.split(/[_\-\s]+/)
		.filter(Boolean)
		.map((word) => ACRONYMS[word.toLowerCase()] ?? word.toLowerCase());
	if (words.length === 0) return identifier;
	words[0] = words[0].charAt(0).toUpperCase() + words[0].slice(1);
	return words.join(" ");
}

export function methodVariant(method: string): BadgeVariant {
	switch (method) {
		case "get":
			return "blue";
		case "post":
			return "green";
		case "put":
		case "patch":
			return "orange";
		case "delete":
			return "red";
		default:
			return "gray";
	}
}

function refName(ref: string): string {
	return ref.replace("#/components/schemas/", "");
}

function isNull(schema: RawSchema): boolean {
	return schema.type === "null";
}

export function parseApiReference(spec: RawSpec): ApiReference {
	const schemas = spec.components?.schemas ?? {};

	const deref = (schema: RawSchema): RawSchema =>
		schema.$ref ? (schemas[refName(schema.$ref)] ?? {}) : schema;

	// utoipa writes `Option<Ref>` as `oneOf: [{ type: "null" }, Ref]`. The
	// Required column already says a field can be absent, so show the Ref.
	const unwrap = (schema: RawSchema): RawSchema => {
		const variants = (schema.oneOf ?? schema.anyOf)?.filter((v) => !isNull(v));
		if (variants?.length !== 1) return schema;
		return {
			...variants[0],
			description: schema.description ?? variants[0].description,
		};
	};

	const typeLabel = (input: RawSchema | undefined): string => {
		if (!input) return "any";
		const schema = unwrap(input);
		if (schema.$ref) return refName(schema.$ref);

		const variants = schema.oneOf ?? schema.anyOf;
		if (variants) {
			return variants
				.filter((v) => !isNull(v))
				.map(typeLabel)
				.join(" | ");
		}
		if (schema.allOf) return schema.allOf.map(typeLabel).join(" & ");
		if (schema.enum)
			return schema.enum.map((v) => JSON.stringify(v)).join(" | ");
		if (schema.items) return `${typeLabel(schema.items)}[]`;
		if (typeof schema.additionalProperties === "object") {
			return `map<string, ${typeLabel(schema.additionalProperties)}>`;
		}

		const types = Array.isArray(schema.type) ? schema.type : [schema.type];
		const type = types.find((t) => t && t !== "null") ?? "object";
		return schema.format ? `${type} · ${schema.format}` : type;
	};

	const fieldsOf = (input: RawSchema): ApiField[] => {
		const schema = deref(unwrap(input));
		const parts = schema.allOf
			? schema.allOf.map((part) => deref(part))
			: [schema];

		const required = new Set(parts.flatMap((part) => part.required ?? []));
		const properties = parts.flatMap((part) =>
			Object.entries(part.properties ?? {}),
		);

		// Mandatory fields first: it's the order a reader fills a body in.
		return properties
			.map(([name, property]) => ({
				name,
				type: typeLabel(property),
				required: required.has(name),
				description: unwrap(property).description,
			}))
			.sort((a, b) => Number(b.required) - Number(a.required));
	};

	const exampleOf = (input: RawSchema | undefined, depth = 0): unknown => {
		if (!input || depth > MAX_EXAMPLE_DEPTH) return null;
		const schema = unwrap(input);

		if (schema.example !== undefined) return schema.example;
		if (schema.$ref) return exampleOf(deref(schema), depth + 1);
		if (schema.allOf) {
			return Object.assign(
				{},
				...schema.allOf.map((part) => exampleOf(part, depth + 1)),
			);
		}
		const variants = schema.oneOf ?? schema.anyOf;
		if (variants) {
			return exampleOf(
				variants.find((v) => !isNull(v)),
				depth + 1,
			);
		}
		if (schema.enum) return schema.enum[0];
		if (schema.items) return [exampleOf(schema.items, depth + 1)];
		if (schema.properties) {
			return Object.fromEntries(
				Object.entries(schema.properties).map(([name, property]) => [
					name,
					exampleOf(property, depth + 1),
				]),
			);
		}
		if (typeof schema.additionalProperties === "object") {
			return { key: exampleOf(schema.additionalProperties, depth + 1) };
		}

		const types = Array.isArray(schema.type) ? schema.type : [schema.type];
		switch (types.find((t) => t && t !== "null")) {
			case "boolean":
				return true;
			case "integer":
			case "number":
				return 0;
			case "object":
				return {};
			case "array":
				return [];
		}
		if (schema.format === "date-time") return EXAMPLE_TIMESTAMP;
		if (schema.format === "date") return EXAMPLE_TIMESTAMP.slice(0, 10);
		if (schema.format === "uuid") return "3fa85f64-5717-4562-b3fc-2c963f66afa6";
		return "string";
	};

	const bodyOf = (content: RawContent | undefined): ApiBody | undefined => {
		if (!content) return undefined;
		const contentType =
			"application/json" in content
				? "application/json"
				: Object.keys(content)[0];
		if (!contentType) return undefined;

		const schema = content[contentType]?.schema;
		if (!schema) return { name: contentType, contentType, fields: [] };

		// A list documents its element's fields.
		const element = unwrap(schema).items ?? schema;
		return {
			name: typeLabel(schema),
			contentType,
			fields: fieldsOf(element),
			example:
				contentType === "application/json"
					? JSON.stringify(exampleOf(schema), null, 2)
					: undefined,
		};
	};

	const parameterField = (parameter: RawParameter): ApiField => ({
		name: parameter.name,
		type: typeLabel(parameter.schema),
		required: parameter.required ?? false,
		description: parameter.description,
	});

	const byTag = new Map<string, ApiOperation[]>();
	const anchors = new Map<string, number>();

	for (const [path, methods] of Object.entries(spec.paths ?? {})) {
		for (const [method, operation] of Object.entries(methods)) {
			if (!METHOD_ORDER.includes(method)) continue;

			const base = slugify(
				(operation.operationId ?? `${method} ${path}`).replace(/[_/]+/g, " "),
			);
			const seen = anchors.get(base) ?? 0;
			anchors.set(base, seen + 1);

			const parameters = operation.parameters ?? [];
			const security = operation.security ?? spec.security ?? [];

			// Untagged routes are grouped by their first path segment.
			const tag = operation.tags?.[0] ?? path.split("/")[1] ?? "other";
			const operations = byTag.get(tag) ?? [];
			byTag.set(tag, operations);

			operations.push({
				method,
				path,
				anchor: seen === 0 ? base : `${base}-${seen}`,
				summary:
					operation.summary ??
					(operation.operationId
						? sentenceCase(operation.operationId)
						: `${method.toUpperCase()} ${path}`),
				description: operation.description,
				auth: security.flatMap((requirement) => Object.keys(requirement)),
				pathParams: parameters
					.filter((p) => p.in === "path")
					.map(parameterField),
				queryParams: parameters
					.filter((p) => p.in === "query")
					.map(parameterField),
				headerParams: parameters
					.filter((p) => p.in === "header")
					.map(parameterField),
				requestBody: bodyOf(operation.requestBody?.content),
				responses: Object.entries(operation.responses ?? {})
					.sort(([a], [b]) => a.localeCompare(b))
					.map(([status, response]) => ({
						status,
						description: response.description,
						headers: Object.entries(response.headers ?? {}).map(
							([name, header]) => ({
								name,
								type: typeLabel(header.schema),
								required: false,
								description: header.description,
							}),
						),
						body: bodyOf(response.content),
					})),
			});
		}
	}

	const tags = [...byTag.entries()]
		.map(([name, operations]) => ({
			title: sentenceCase(name),
			anchor: `tag-${slugify(name.replace(/[_/]+/g, " "))}`,
			operations: operations.sort(
				(a, b) =>
					a.path.localeCompare(b.path) ||
					METHOD_ORDER.indexOf(a.method) - METHOD_ORDER.indexOf(b.method),
			),
		}))
		.sort((a, b) => a.title.localeCompare(b.title));

	return { version: spec.info?.version, tags };
}

export function useApiReference(enabled = true) {
	const { config } = useConfig();
	const baseUrl = (config?.API_BASE_URL ?? "").replace(/\/+$/, "");

	const query = useQuery({
		queryKey: ["docs", "openapi", baseUrl],
		queryFn: async ({ signal }) => {
			const response = await fetch(`${baseUrl}/openapi.json`, { signal });
			if (!response.ok) {
				throw new Error(`${response.status} ${response.statusText}`);
			}
			return parseApiReference((await response.json()) as RawSpec);
		},
		enabled: enabled && baseUrl !== "",
		// The spec only changes when the API is redeployed.
		staleTime: Number.POSITIVE_INFINITY,
		refetchOnWindowFocus: false,
	});

	return { ...query, baseUrl };
}
