export function createGitHubAPI(repo, token, request = fetch) {
	return async (endpoint, { method = "GET", body, missing = false } = {}) => {
		const response = await request(
			"https://api.github.com/repos/" + repo + endpoint,
			{
				method,
				headers: {
					Authorization: "Bearer " + token,
					Accept: "application/vnd.github+json",
					"Content-Type": "application/json",
				},
				body: body ? JSON.stringify(body) : undefined,
				signal: AbortSignal.timeout(30_000),
			},
		);
		if (response.status === 404 && missing) return null;
		const text = await response.text();
		let data;
		try {
			data = text ? JSON.parse(text) : null;
		} catch {
			data = null;
		}
		if (!response.ok) {
			// Preserve GitHub's reason; status alone hides required-check/approval failures.
			const detail =
				typeof data?.message === "string" ? ": " + data.message : "";
			const error = new Error(
				"GitHub " + endpoint + ": HTTP " + response.status + detail,
			);
			error.status = response.status;
			throw error;
		}
		return data;
	};
}
