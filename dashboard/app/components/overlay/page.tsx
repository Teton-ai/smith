"use client";

import { useState } from "react";
import { Button } from "../button";
import { Modal } from "../modal";
import { SidePanel } from "../side-panel";

export default function OverlayPage() {
	const [basicOpen, setBasicOpen] = useState(false);
	const [wideOpen, setWideOpen] = useState(false);
	const [noFooterOpen, setNoFooterOpen] = useState(false);
	const [sidePanelOpen, setSidePanelOpen] = useState(false);

	return (
		<div className="max-w-5xl mx-auto px-6 py-12">
			<h1 className="text-3xl font-bold text-gray-900 mb-12">Overlay</h1>

			{/* Modal */}
			<section className="mb-16">
				<h2 className="text-xs font-medium text-gray-400 uppercase tracking-widest mb-6">
					Modal
				</h2>
				<div className="flex flex-wrap items-center gap-3">
					<Button onClick={() => setBasicOpen(true)}>Basic Modal</Button>
					<Button variant="secondary" onClick={() => setWideOpen(true)}>
						Wide Modal
					</Button>
					<Button variant="secondary" onClick={() => setNoFooterOpen(true)}>
						No Footer
					</Button>
				</div>
			</section>

			{/* Side Panel */}
			<section className="mb-16">
				<h2 className="text-xs font-medium text-gray-400 uppercase tracking-widest mb-6">
					Side Panel
				</h2>
				<div className="flex flex-wrap items-center gap-3">
					<Button onClick={() => setSidePanelOpen(true)}>
						Open Side Panel
					</Button>
				</div>
			</section>

			<SidePanel
				open={sidePanelOpen}
				onClose={() => setSidePanelOpen(false)}
				title="Details"
				subtitle="Side panel with slide animation"
				footer={
					<>
						<Button variant="secondary" onClick={() => setSidePanelOpen(false)}>
							Close
						</Button>
						<Button onClick={() => setSidePanelOpen(false)}>Save</Button>
					</>
				}
			>
				<p className="text-sm text-gray-600">
					A side panel slides in from the right. It supports the same header,
					body, and footer layout as the modal. Click the backdrop, press
					Escape, or use the X button to close.
				</p>
			</SidePanel>

			{/* Basic Modal */}
			<Modal
				open={basicOpen}
				onClose={() => setBasicOpen(false)}
				title="Confirm Action"
				subtitle="This is a basic modal"
				footer={
					<>
						<Button variant="secondary" onClick={() => setBasicOpen(false)}>
							Cancel
						</Button>
						<Button onClick={() => setBasicOpen(false)}>Confirm</Button>
					</>
				}
			>
				<p className="text-sm text-gray-600">
					Are you sure you want to perform this action? This cannot be undone.
				</p>
			</Modal>

			{/* Wide Modal */}
			<Modal
				open={wideOpen}
				onClose={() => setWideOpen(false)}
				title="Deploy Release"
				subtitle="Step 1 of 2 — Select deployment strategy"
				width="w-[900px]"
				footer={
					<>
						<Button variant="secondary" onClick={() => setWideOpen(false)}>
							Cancel
						</Button>
						<Button onClick={() => setWideOpen(false)}>Start Deployment</Button>
					</>
				}
			>
				<p className="text-sm text-gray-600">
					Wide modals are useful for multi-step flows with side-by-side content
					like device selection with a preview panel.
				</p>
			</Modal>

			{/* No Footer Modal */}
			<Modal
				open={noFooterOpen}
				onClose={() => setNoFooterOpen(false)}
				title="Information"
			>
				<p className="text-sm text-gray-600">
					A modal without a footer. Close it with the X button, press Escape, or
					click the backdrop.
				</p>
			</Modal>
		</div>
	);
}
