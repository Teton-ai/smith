"use client";

import { useAuth0 } from "@auth0/auth0-react";
import type { ReactNode } from "react";

interface AuthGuardProps {
	children: ReactNode;
	fallback?: ReactNode;
}

export default function AuthGuard({ children, fallback }: AuthGuardProps) {
	const { isAuthenticated, isLoading } = useAuth0();

	if (isLoading) {
		return <div>Loading...</div>;
	}

	if (!isAuthenticated) {
		return fallback || <div>Please log in to access this content.</div>;
	}

	return <>{children}</>;
}
