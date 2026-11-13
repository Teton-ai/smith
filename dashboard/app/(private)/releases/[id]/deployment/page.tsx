import { PageContainer } from "@teton/smith-ui";
import { Navigate, useParams } from "react-router";
import { useApiGetReleaseDeployment } from "@/app/api-client";

/** Keeps old links working: a release's deployment is now a page of its own. */
const ReleaseDeploymentRedirect = () => {
	const releaseId = Number(useParams().id);
	const { data, isLoading } = useApiGetReleaseDeployment(releaseId);
	if (isLoading) return null;
	if (data) return <Navigate to={`/deployments/${data.id}`} replace />;
	return (
		<PageContainer>
			<p className="text-sm text-gray-500">
				No deployment found for this release.
			</p>
		</PageContainer>
	);
};

export default ReleaseDeploymentRedirect;
