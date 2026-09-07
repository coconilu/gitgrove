const CI_PATH = ".github/workflows/ci.yml";

export function dispatchedRun(runs, { afterId, sha, branch }) {
	return runs
		.filter(
			(run) =>
				run.id > afterId &&
				run.head_sha === sha &&
				run.head_branch === branch &&
				run.event === "workflow_dispatch" &&
				run.path === CI_PATH,
		)
		.sort((a, b) => b.id - a.id)[0];
}

// Explicit workflow_dispatch works with GITHUB_TOKEN; no PR workflow approval is needed.
export async function runCI(
	api,
	branch,
	sha,
	{
		sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
		now = Date.now,
		timeout = 30 * 60 * 1000,
		log = console.log,
	} = {},
) {
	const endpoint =
		"/actions/workflows/ci.yml/runs?event=workflow_dispatch&branch=" +
		encodeURIComponent(branch) +
		"&per_page=100";
	const before = await api(endpoint);
	const afterId = Math.max(0, ...before.workflow_runs.map((run) => run.id));
	await api("/actions/workflows/ci.yml/dispatches", {
		method: "POST",
		body: { ref: branch },
	});
	const deadline = now() + timeout;
	let announced = null;
	while (now() < deadline) {
		const { workflow_runs: runs } = await api(endpoint);
		const run = dispatchedRun(runs, { afterId, sha, branch });
		if (run && run.id !== announced) {
			log("等待 CI: " + run.html_url + "（" + sha + "）");
			announced = run.id;
		}
		if (run?.status === "completed") {
			if (run.conclusion !== "success")
				throw new Error("CI 未通过，发布已停止: " + run.html_url);
			return run;
		}
		await sleep(10_000);
	}
	throw new Error(
		"等待提交 " + sha + " 的 CI 超时，发布已停止；可重跑失败任务。",
	);
}

// Both the version PR and the resulting merge commit need their own CI evidence.
export async function completeVersionRelease(
	pr,
	{ check, merge, assertCurrent },
) {
	if (pr.state !== "open" && !pr.merged)
		throw new Error("版本 PR 已关闭且未合并，停止发布");
	let sha = pr.merge_commit_sha;
	if (!pr.merged) {
		await check(pr.head.ref, pr.head.sha);
		sha = await merge(pr.number, pr.head.sha);
	}
	if (!/^[a-f0-9]{40}$/.test(sha ?? ""))
		throw new Error("缺少已合并的版本提交，停止发布");
	await assertCurrent(pr.base.ref, sha);
	await check(pr.base.ref, sha);
	return sha;
}

export async function mergeVersionPR(
	api,
	number,
	sha,
	base,
	baseSha,
	{
		sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
		attempts = 12,
	} = {},
) {
	for (let attempt = 0; attempt < attempts; attempt++) {
		const current = await api("/git/ref/heads/" + base);
		if (current.object.sha !== baseSha)
			throw new Error(
				"主分支在 CI 期间发生变化，停止自动合并；请从当前主分支重跑。",
			);
		try {
			const result = await api("/pulls/" + number + "/merge", {
				method: "PUT",
				body: { sha, merge_method: "squash" },
			});
			if (!result.merged) throw new Error("版本 PR 未能合并，发布已停止");
			return result.sha;
		} catch (error) {
			// Give required checks time to propagate, but never bypass protection.
			if (error.status !== 405 || attempt + 1 === attempts) throw error;
			await sleep(5_000);
		}
	}
}
