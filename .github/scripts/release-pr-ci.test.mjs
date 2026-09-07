import assert from "node:assert/strict";
import test from "node:test";
import { pullRequestRun, runPullRequestCI } from "./release-ci.mjs";
import { createGitHubAPI } from "./release-github.mjs";

const sha = "a".repeat(40);
const repo = { full_name: "owner/repo" };
const pr = {
	number: 11,
	state: "open",
	head: { ref: "codex/release-123", sha, repo },
	base: { ref: "master", sha: "b".repeat(40), repo },
};
const pending = {
	id: 200,
	event: "pull_request",
	path: ".github/workflows/ci.yml",
	head_sha: sha,
	head_branch: pr.head.ref,
	repository: repo,
	pull_requests: [
		{ number: pr.number, head: { sha }, base: { ref: "master" } },
	],
	status: "completed",
	conclusion: "action_required",
	html_url: "https://github.com/owner/repo/actions/runs/200",
};

test("真实回归：dispatch 成功仍有待批准 PR suite，批准一次并等 PR CI 才放行", async () => {
	const writes = [];
	let reads = 0;
	const run = await runPullRequestCI(
		async (path, options) => {
			if (options) {
				writes.push({ path, ...options });
				return null;
			}
			if (path === "/pulls/11") return pr;
			reads++;
			const actual =
				reads < 3
					? pending
					: reads === 3
						? { ...pending, status: "in_progress", conclusion: null }
						: { ...pending, conclusion: "success" };
			return {
				workflow_runs: [
					{
						...pending,
						id: 201,
						event: "workflow_dispatch",
						conclusion: "success",
					},
					actual,
				],
			};
		},
		pr,
		{ sleep: async () => {}, log: () => {} },
	);
	assert.equal(run.id, pending.id);
	assert.equal(reads, 4);
	assert.deepEqual(writes, [
		{ path: "/actions/runs/200/approve", method: "POST" },
	]);
});

test("不批准无关 PR、旧提交、其他分支 / 工作流或其他仓库", () => {
	for (const patch of [
		{ pull_requests: [{ number: 12, head: { sha }, base: { ref: "master" } }] },
		{ head_sha: "c".repeat(40) },
		{ head_branch: "feature" },
		{ path: ".github/workflows/other.yml" },
		{ repository: { full_name: "fork/repo" } },
	])
		assert.equal(pullRequestRun([{ ...pending, ...patch }], pr), undefined);
	assert.throws(
		() =>
			pullRequestRun([pending], {
				...pr,
				head: { ...pr.head, repo: { full_name: "fork/repo" } },
			}),
		/本仓库/,
	);
	assert.throws(
		() =>
			pullRequestRun([pending], {
				...pr,
				head: { ...pr.head, ref: "codex/feature" },
			}),
		/自动版本 PR/,
	);
});

test("审批前再次核对 PR，提交 / 基础分支 / 状态变化时不审批", async () => {
	for (const changed of [
		{ ...pr, state: "closed" },
		{ ...pr, head: { ...pr.head, sha: "c".repeat(40) } },
		{ ...pr, base: { ...pr.base, sha: "c".repeat(40) } },
	]) {
		let writes = 0;
		await assert.rejects(
			runPullRequestCI(
				async (path, options) => {
					if (options) writes++;
					return path === "/pulls/11" ? changed : { workflow_runs: [pending] };
				},
				pr,
				{ log: () => {} },
			),
			/发生变化/,
		);
		assert.equal(writes, 0);
	}
});

test("已经运行或通过的 PR CI 不重复审批；失败、权限拒绝和超时不放行", async () => {
	let writes = 0;
	const passed = await runPullRequestCI(
		async (_path, options) => {
			if (options) writes++;
			return { workflow_runs: [{ ...pending, conclusion: "success" }] };
		},
		pr,
		{ log: () => {} },
	);
	assert.equal(passed.conclusion, "success");
	assert.equal(writes, 0);
	for (const conclusion of ["failure", "cancelled", "skipped"]) {
		await assert.rejects(
			runPullRequestCI(
				async () => ({ workflow_runs: [{ ...pending, conclusion }] }),
				pr,
				{ log: () => {} },
			),
			/未通过/,
		);
	}
	await assert.rejects(
		runPullRequestCI(
			async (path, options) => {
				if (options) throw new Error("approval forbidden");
				return path === "/pulls/11" ? pr : { workflow_runs: [pending] };
			},
			pr,
			{ log: () => {} },
		),
		/approval forbidden/,
	);
	let time = 0;
	await assert.rejects(
		runPullRequestCI(async () => ({ workflow_runs: [] }), pr, {
			now: () => time,
			timeout: 10,
			sleep: async () => {
				time += 10;
			},
			log: () => {},
		}),
		/超时/,
	);
});

test("GitHub 审批 201 / dispatch 204 空响应正确处理，405 保留具体阻塞原因", async () => {
	for (const status of [201, 202, 204]) {
		const api = createGitHubAPI(
			"owner/repo",
			"test-token",
			async () => new Response(null, { status }),
		);
		assert.equal(
			await api("/actions/runs/200/approve", { method: "POST" }),
			null,
		);
	}
	const api = createGitHubAPI(
		"owner/repo",
		"test-token",
		async () =>
			new Response(
				JSON.stringify({
					message: 'Required status check "check" is expected.',
				}),
				{ status: 405 },
			),
	);
	await assert.rejects(
		api("/pulls/11/merge", { method: "PUT" }),
		(error) =>
			error.status === 405 &&
			error.message.includes('Required status check "check" is expected.'),
	);
});
