"use client";

import { useAuth0 } from "@auth0/auth0-react";

export default function LogoutButton() {
	const { logout, isAuthenticated } = useAuth0();

	if (!isAuthenticated) {
		return null;
	}

	return (
		<button
			onClick={() =>
				logout({
					logoutParams: {
						returnTo:
							typeof window !== "undefined" ? window.location.origin : "",
					},
				})
			}
			className="bg-red-500 hover:bg-red-700 text-white font-bold py-2 px-4 rounded"
		>
			Log Out
		</button>
	);
}
