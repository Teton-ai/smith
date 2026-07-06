import type { Device } from "@/app/api-client";

export const formatTimeAgo = (dateString: string) => {
	const now = new Date();
	const past = new Date(dateString);
	const diff = now.getTime() - past.getTime();
	const minutes = Math.floor(diff / 60000);
	const hours = Math.floor(minutes / 60);
	const days = Math.floor(hours / 24);

	if (days > 0) return `${days}d ago`;
	if (hours > 0) return `${hours}h ago`;
	return `${minutes}m ago`;
};

// Sort by last_seen descending (most recent first), never seen at the end
export const sortByLastSeen = (a: Device, b: Device) => {
	if (!a.last_seen && !b.last_seen) return 0;
	if (!a.last_seen) return 1;
	if (!b.last_seen) return -1;
	return new Date(b.last_seen).getTime() - new Date(a.last_seen).getTime();
};
