import { defineConfig } from "orval";

export default defineConfig({
	"petstore-file": {
		input: {
			target: "http://localhost:8080/openapi.json",
			override: {
				transformer: (options) => {
					if (options.components?.schemas == null) return options;

					// Remove null's from types, so we don't get e.g. `string | null | undefined` but only `string | undefined`
					for (const [schemaName, schema] of Object.entries(
						options.components?.schemas || {},
					)) {
						if ("properties" in schema && schema.properties != null) {
							for (let [propertyName, property] of Object.entries(
								schema.properties,
							)) {
								if ("type" in property && Array.isArray(property.type)) {
									property.type = property.type.filter((t) => t !== "null");
									if (property.type.length === 1) {
										property.type = property.type[0];
									}
								} else if ("oneOf" in property && property.oneOf != null) {
									property.oneOf = property.oneOf.filter((oneOf) => {
										return (oneOf as any).type !== "null";
									});
									if (property.oneOf.length === 1) {
										property = property.oneOf[0];
									}
								}
								schema.properties[propertyName] = property;
							}
							options.components.schemas[schemaName] = schema;
						}
					}

					return options;
				},
			},
		},
		output: {
			client: "react-query",
			target: "./app/api-client.ts",
			override: {
				query: {
					useInfinite: true,
					useInfiniteQueryParam: "offset",
				},
				mutator: {
					path: "./app/api-client-mutator.ts",
					name: "useClientMutator",
				},
			},
		},
	},
});
