import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";
import "@/app/globals.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { RouterProvider } from "react-router";
import Auth0ProviderWrapper from "@/app/providers/auth0-provider";
import QueryProvider from "@/app/providers/query-provider";
import { router } from "./router";

const rootElement = document.getElementById("root");
if (!rootElement) {
	throw new Error("root element not found");
}

createRoot(rootElement).render(
	<StrictMode>
		<QueryProvider>
			<Auth0ProviderWrapper>
				<RouterProvider router={router} />
			</Auth0ProviderWrapper>
		</QueryProvider>
	</StrictMode>,
);
