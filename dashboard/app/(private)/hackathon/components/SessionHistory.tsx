"use client";

import { Calendar, ChevronRight, Cpu, Loader2, Search, Tag } from "lucide-react";
import { useMemo, useState } from "react";
import {
	type ExtendedTestSessionSummary,
	useExtendedTestSessions,
} from "../hooks/useExtendedTest";

function formatSessionDate(dateString: string): string {
	const date = new Date(dateString);
	return date.toLocaleDateString("en-US", {
		month: "short",
		day: "numeric",
		hour: "2-digit",
		minute: "2-digit",
	});
}

function getStatusBadge(status: string) {
	switch (status) {
		case "completed":
			return "bg-green-100 text-green-800";
		case "canceled":
			return "bg-amber-100 text-amber-800";
		case "running":
		case "partial":
			return "bg-blue-100 text-blue-800";
		default:
			return "bg-gray-100 text-gray-800";
	}
}

interface SessionHistoryProps {
	onSelectSession: (session: ExtendedTestSessionSummary) => void;
}

export default function SessionHistory({ onSelectSession }: SessionHistoryProps) {
	const [searchQuery, setSearchQuery] = useState("");
	const { data: sessions, isLoading } = useExtendedTestSessions();

	const filteredSessions = useMemo(() => {
		if (!sessions || !searchQuery.trim()) return sessions;

		const query = searchQuery.toLowerCase();
		return sessions.filter((session) => {
			// Search in label filter
			if (session.label_filter?.toLowerCase().includes(query)) return true;

			// Search in date
			const dateStr = formatSessionDate(session.created_at).toLowerCase();
			if (dateStr.includes(query)) return true;

			// Search in status
			if (session.status.toLowerCase().includes(query)) return true;

			return false;
		});
	}, [sessions, searchQuery]);

	if (isLoading) {
		return (
			<div className="flex items-center justify-center py-12">
				<div className="flex flex-col items-center space-y-3">
					<Loader2 className="w-6 h-6 animate-spin text-indigo-600" />
					<p className="text-sm text-gray-500">Loading test history...</p>
				</div>
			</div>
		);
	}

	if (!sessions || sessions.length === 0) {
		return (
			<div className="text-center py-12">
				<Calendar className="w-10 h-10 text-gray-300 mx-auto mb-3" />
				<p className="text-gray-500">No test sessions yet</p>
				<p className="text-sm text-gray-400 mt-1">
					Select devices above to run your first test
				</p>
			</div>
		);
	}

	return (
		<div className="space-y-3">
			{/* Search Input */}
			<div className="relative">
				<Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400" />
				<input
					type="text"
					placeholder="Search by label, date, or status..."
					value={searchQuery}
					onChange={(e) => setSearchQuery(e.target.value)}
					className="w-full pl-10 pr-4 py-2 text-sm text-gray-900 placeholder-gray-400 bg-white border border-gray-200 rounded-lg focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:border-transparent"
				/>
			</div>

			{/* Sessions List */}
			<div className="space-y-2 max-h-96 overflow-y-auto">
				{filteredSessions && filteredSessions.length > 0 ? (
					filteredSessions.map((session) => (
						<button
							key={session.session_id}
							onClick={() => onSelectSession(session)}
							className="w-full px-4 py-3 bg-white border border-gray-200 rounded-lg hover:bg-gray-50 hover:border-gray-300 transition-colors text-left"
						>
							<div className="flex items-center justify-between">
								<div className="flex-1 min-w-0">
									<div className="flex items-center space-x-3">
										<span className="text-sm font-medium text-gray-900">
											{formatSessionDate(session.created_at)}
										</span>
										<span
											className={`px-2 py-0.5 text-xs rounded-full ${getStatusBadge(session.status)}`}
										>
											{session.status}
										</span>
									</div>
									<div className="flex items-center space-x-4 mt-1 text-xs text-gray-500">
										<span className="flex items-center space-x-1">
											<Tag className="w-3 h-3" />
											<span className="truncate max-w-[200px]">
												{session.label_filter || "All devices"}
											</span>
										</span>
										<span className="flex items-center space-x-1">
											<Cpu className="w-3 h-3" />
											<span>
												{session.device_count} device
												{session.device_count !== 1 ? "s" : ""}
											</span>
										</span>
										<span>
											{session.completed_count}/{session.device_count} completed
										</span>
									</div>
								</div>
								<ChevronRight className="w-5 h-5 text-gray-400 flex-shrink-0" />
							</div>
						</button>
					))
				) : (
					<div className="text-center py-8">
						<Search className="w-8 h-8 text-gray-300 mx-auto mb-2" />
						<p className="text-sm text-gray-500">No sessions match your search</p>
					</div>
				)}
			</div>
		</div>
	);
}
