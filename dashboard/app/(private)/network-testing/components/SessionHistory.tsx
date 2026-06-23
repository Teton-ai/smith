"use client";

import { Calendar, Cpu, Loader2, Tag } from "lucide-react";
import moment from "moment";
import { useMemo } from "react";
import { Badge, type BadgeVariant } from "@/app/components/ui";
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

const STATUS_VARIANT: Record<string, BadgeVariant> = {
	completed: "green",
	canceled: "yellow",
	running: "blue",
	partial: "blue",
	pending: "blue",
};

interface SessionHistoryProps {
	searchQuery: string;
	onSelectSession: (session: ExtendedTestSessionSummary) => void;
}

export default function SessionHistory({
	searchQuery,
	onSelectSession,
}: SessionHistoryProps) {
	const { data: sessions, isLoading } = useExtendedTestSessions();

	const filteredSessions = useMemo(() => {
		if (!sessions || !searchQuery.trim()) return sessions;

		const query = searchQuery.toLowerCase();
		return sessions.filter((session) => {
			if (session.label_filter?.toLowerCase().includes(query)) return true;
			if (formatSessionDate(session.created_at).toLowerCase().includes(query))
				return true;
			if (session.status.toLowerCase().includes(query)) return true;
			return false;
		});
	}, [sessions, searchQuery]);

	return (
		<div className="border border-gray-200/80 rounded-xl overflow-hidden bg-white shadow-sm">
			{/* Header */}
			<div className="bg-gray-50 px-4 py-3 border-b border-gray-200">
				<div className="grid grid-cols-[2fr_2fr_1fr_1fr_auto] gap-4 text-xs font-medium text-gray-500 uppercase tracking-wide items-center">
					<div>Started</div>
					<div>Filter</div>
					<div>Devices</div>
					<div>Progress</div>
					<div className="text-right">Status</div>
				</div>
			</div>

			{isLoading ? (
				<div className="flex items-center justify-center py-12">
					<Loader2 className="w-6 h-6 animate-spin text-blue-600" />
				</div>
			) : !sessions || sessions.length === 0 ? (
				<div className="text-center py-12">
					<Calendar className="w-10 h-10 text-gray-300 mx-auto mb-3" />
					<p className="text-gray-500">No test sessions yet</p>
					<p className="text-sm text-gray-400 mt-1">
						Use “New Test” to run your first test
					</p>
				</div>
			) : !filteredSessions || filteredSessions.length === 0 ? (
				<div className="text-center py-12">
					<p className="text-sm text-gray-500">No sessions match your search</p>
				</div>
			) : (
				<div className="divide-y divide-gray-200">
					{filteredSessions.map((session) => (
						<button
							key={session.session_id}
							type="button"
							onClick={() => onSelectSession(session)}
							className="w-full text-left px-4 py-3 cursor-pointer transition-colors"
						>
							<div className="grid grid-cols-[2fr_2fr_1fr_1fr_auto] gap-4 items-center">
								<div className="flex flex-col min-w-0">
									<span className="text-sm font-medium text-gray-900 truncate">
										{formatSessionDate(session.created_at)}
									</span>
									<span className="text-xs text-gray-400 mt-0.5">
										{moment(session.created_at).fromNow()}
									</span>
								</div>

								<div className="flex items-center space-x-2 min-w-0">
									<Tag className="w-3.5 h-3.5 text-gray-400 flex-shrink-0" />
									<span className="text-sm text-gray-600 truncate">
										{session.label_filter || "All devices"}
									</span>
								</div>

								<div className="flex items-center space-x-2">
									<Cpu className="w-3.5 h-3.5 text-gray-400 flex-shrink-0" />
									<span className="text-sm text-gray-600">
										{session.device_count}
									</span>
								</div>

								<div className="text-sm text-gray-600">
									{session.completed_count}/{session.device_count}
								</div>

								<div className="text-right">
									<Badge
										variant={STATUS_VARIANT[session.status] ?? "gray"}
										pill
									>
										{session.status}
									</Badge>
								</div>
							</div>
						</button>
					))}
				</div>
			)}
		</div>
	);
}
