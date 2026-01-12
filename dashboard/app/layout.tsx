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

export default function RootLayout({
	children,
}: Readonly<{
	children: React.ReactNode;
}>) {
	return (
		<html lang="en">
			<head>
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
