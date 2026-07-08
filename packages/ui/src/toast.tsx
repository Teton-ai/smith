import { Check, X } from "lucide-react";

export interface ToastState {
	message: string;
	type: "success" | "error";
}

/** Fixed top-right toast notification. Render with the page's toast state;
 *  pass `null` to render nothing. */
export function Toast({
	toast,
	onClose,
}: {
	toast: ToastState | null;
	onClose: () => void;
}) {
	if (!toast) return null;
	return (
		<div
			className={`fixed top-4 right-4 z-50 p-4 rounded-lg shadow-lg border transition-all duration-300 ease-in-out ${
				toast.type === "success"
					? "bg-green-50 text-green-800 border-green-200"
					: "bg-red-50 text-red-800 border-red-200"
			}`}
		>
			<div className="flex items-center space-x-2">
				{toast.type === "success" ? (
					<Check className="w-5 h-5 text-green-600" />
				) : (
					<X className="w-5 h-5 text-red-600" />
				)}
				<span className="text-sm font-medium">{toast.message}</span>
				<button
					type="button"
					onClick={onClose}
					className="ml-2 text-gray-400 hover:text-gray-600 cursor-pointer"
				>
					<X className="w-4 h-4" />
				</button>
			</div>
		</div>
	);
}
