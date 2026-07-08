import { Button, Card } from "@teton/smith-ui";
import { Compass, Home, MoveLeft } from "lucide-react";
import { useNavigate } from "react-router";

export default function NotFoundPage() {
	const navigate = useNavigate();

	return (
		<div className="min-h-screen bg-gray-50 flex items-center justify-center px-4">
			<div className="w-full max-w-md">
				<Card className="p-10">
					<div className="flex flex-col items-center text-center">
						<div className="flex h-16 w-16 items-center justify-center rounded-xl bg-blue-50 text-blue-600 shadow-sm">
							<Compass className="h-8 w-8" />
						</div>

						<p className="mt-6 text-6xl font-bold tracking-tight text-gray-900">
							404
						</p>
						<h1 className="mt-2 text-xl font-semibold text-gray-900">
							Page not found
						</h1>
						<p className="mt-2 text-sm text-gray-500">
							The page you&#39;re looking for doesn&#39;t exist or may have been
							moved.
						</p>

						<div className="mt-8 flex w-full flex-col gap-3 sm:flex-row">
							<Button
								tone="gray"
								variant="soft"
								size="md"
								icon={<MoveLeft className="h-4 w-4" />}
								onClick={() => navigate(-1)}
								className="w-full py-2.5"
							>
								Go back
							</Button>
							<Button
								tone="blue"
								variant="solid"
								size="md"
								icon={<Home className="h-4 w-4" />}
								onClick={() => navigate("/dashboard")}
								className="w-full py-2.5"
							>
								Dashboard
							</Button>
						</div>
					</div>
				</Card>
			</div>
		</div>
	);
}
