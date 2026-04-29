"use client";

import { Geist, Geist_Mono } from "next/font/google";
import "./globals.css";
import Auth0ProviderWrapper from "./providers/auth0-provider";
import QueryProvider from "./providers/query-provider";

const geistSans = Geist({
	variable: "--font-geist-sans",
	subsets: ["latin"],
});

const geistMono = Geist_Mono({
	variable: "--font-geist-mono",
	subsets: ["latin"],
});

// Inlined as a synchronous <head> script so the listener is registered
// during HTML parse — before any /_next/static/ chunk's network response
// can come back. During a rolling ECS deploy the HTML and the chunks can
// be served by tasks on different builds and the very layout chunk that
// hosts our React tree may 404; a useEffect-based listener can't help
// because the component never mounts.
const CHUNK_ERROR_RELOAD_SCRIPT = `(function(){
	var KEY="__chunkErrorReloadAt",COOLDOWN=30000;
	function reloadOnce(){
		try{
			var last=Number(sessionStorage.getItem(KEY)||0);
			if(Date.now()-last<COOLDOWN)return;
			sessionStorage.setItem(KEY,String(Date.now()));
		}catch(e){}
		location.reload();
	}
	function isChunkLoadError(r){
		if(!r)return false;
		if(r.name==="ChunkLoadError")return true;
		var m=(r&&r.message)||String(r);
		return /Loading (CSS )?chunk \\S+ failed/i.test(m)
			||/Failed to fetch dynamically imported module/i.test(m)
			||/Importing a module script failed/i.test(m);
	}
	function isStaleNextAsset(t){
		if(!t||!t.tagName)return false;
		var u=t.tagName==="LINK"?t.href:t.tagName==="SCRIPT"?t.src:null;
		return !!u&&u.indexOf("/_next/static/")!==-1;
	}
	addEventListener("error",function(e){
		if(e&&e.error&&isChunkLoadError(e.error)){reloadOnce();return;}
		if(e&&typeof e.message==="string"&&isChunkLoadError({message:e.message})){reloadOnce();return;}
		if(e&&isStaleNextAsset(e.target))reloadOnce();
	},true);
	addEventListener("unhandledrejection",function(e){
		if(e&&isChunkLoadError(e.reason))reloadOnce();
	});
})();`;

export default function RootLayout({
	children,
}: Readonly<{
	children: React.ReactNode;
}>) {
	return (
		<html lang="en">
			<head>
				<script
					// biome-ignore lint/security/noDangerouslySetInnerHtml: hardcoded constant, no user input — must be inline so listeners register at parse time
					dangerouslySetInnerHTML={{ __html: CHUNK_ERROR_RELOAD_SCRIPT }}
				/>
				<link rel="icon" href="/logo.png" type="image/png" />
			</head>
			<body
				className={`${geistSans.variable} ${geistMono.variable} antialiased`}
			>
				<QueryProvider>
					<Auth0ProviderWrapper>{children}</Auth0ProviderWrapper>
				</QueryProvider>
			</body>
		</html>
	);
}
