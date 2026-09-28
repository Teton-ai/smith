import { Badge, Card } from "@teton/smith-ui";
import { ApiTags, CODE_CLASS, InfoRow, LINK_CLASS } from "../api/page";
import { useScrollOnNavigate } from "../markdown";
import { smithdReference } from "./reference";

const SOCKET = "/run/smithd/smithd.sock";

export default function SmithdReferencePage() {
	useScrollOnNavigate(true);

	return (
		<article>
			<header className="mb-8">
				<div className="flex flex-wrap items-center gap-3">
					<h1 className="text-3xl font-bold tracking-tight text-gray-900">
						Daemon reference
					</h1>
					{smithdReference.version && (
						<Badge pill>v{smithdReference.version}</Badge>
					)}
				</div>
				<p className="mt-3 text-lg leading-relaxed text-gray-500">
					The local control API that <code className={CODE_CLASS}>smithd</code>{" "}
					serves on each device, for the{" "}
					<code className={CODE_CLASS}>smithd</code> subcommands and other
					services running alongside it.
				</p>
			</header>

			<Card className="mb-10">
				<dl className="divide-y divide-gray-100">
					<InfoRow label="Socket">
						<code className={`${CODE_CLASS} break-all`}>{SOCKET}</code>
					</InfoRow>
					<InfoRow label="Authentication">
						None. The socket is only accessible to root, so run requests as root
						on the device.
					</InfoRow>
					<InfoRow label="Example">
						<code className={`${CODE_CLASS} break-all`}>
							curl --unix-socket {SOCKET} http://localhost/watchdog
						</code>
					</InfoRow>
					<InfoRow label="Spec">
						<a
							href="https://github.com/Teton-ai/smith/blob/main/smithd/openapi.json"
							target="_blank"
							rel="noopener noreferrer"
							className={LINK_CLASS}
						>
							openapi.json
						</a>{" "}
						to generate a client.
					</InfoRow>
				</dl>
			</Card>

			<ApiTags reference={smithdReference} unauthenticated="Root only" />
		</article>
	);
}
