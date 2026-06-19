import { useAuth0 } from "@auth0/auth0-react";
import { AlertTriangle, Loader2, LogIn } from "lucide-react";
import { useEffect } from "react";
import { useNavigate } from "react-router";
import { Button, Card } from "@/app/components/ui";

const LoginPage = () => {
	const { isLoading, error, loginWithPopup } = useAuth0();

	return (
		<div className="min-h-screen bg-gray-50 flex items-center justify-center px-4">
			<div className="w-full max-w-md">
				<Card className="p-8">
					<div className="flex flex-col items-center text-center">
						<img
							src="/logo.png"
							alt="Smith Logo"
							width={64}
							height={64}
							className="h-16 w-16 shrink-0 rounded-xl shadow-sm"
						/>
						<h1 className="mt-5 text-2xl font-bold text-gray-900">Smith</h1>
						<p className="mt-1 text-sm text-gray-500">
							Teton&#39;s Fleet Management System
						</p>

						{error && (
							<div className="mt-6 flex w-full items-start gap-3 rounded-lg border border-red-200 bg-red-50 p-4 text-left">
								<AlertTriangle className="mt-0.5 h-5 w-5 shrink-0 text-red-600" />
								<div>
									<p className="text-sm font-semibold text-red-800">
										Sign-in failed
									</p>
									<p className="mt-0.5 text-sm text-red-700">{error.message}</p>
								</div>
							</div>
						)}

						<Button
							tone="blue"
							variant="solid"
							size="md"
							loading={isLoading}
							disabled={isLoading}
							icon={!isLoading && <LogIn className="h-4 w-4" />}
							onClick={() => loginWithPopup()}
							className="mt-7 w-full py-2.5"
						>
							{isLoading ? "Signing in…" : "Log In"}
						</Button>
					</div>
				</Card>
			</div>
		</div>
	);
};

export default function Home() {
	const { isLoading, isAuthenticated } = useAuth0();
	const navigate = useNavigate();

	useEffect(() => {
		if (!isLoading && isAuthenticated) {
			navigate("/dashboard");
		}
	}, [isLoading, isAuthenticated, navigate]);

	// Once authenticated we navigate to /dashboard; show a centered spinner
	// during that redirect instead of flashing the login card.
	if (!isLoading && isAuthenticated) {
		return (
			<div className="min-h-screen bg-gray-50 flex items-center justify-center">
				<Loader2 className="h-8 w-8 animate-spin text-blue-600" />
			</div>
		);
	}

	return <LoginPage />;
}
