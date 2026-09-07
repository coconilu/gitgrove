import assert from "node:assert/strict";
import test from "node:test";
import { assertVersionFiles } from "./prepare-release.mjs";
import {
	completeVersionRelease,
	dispatchedRun,
	mergeVersionPR,
	runCI,
} from "./release-ci.mjs";
import { releaseTarget } from "./release-target.mjs";

const sha = "a".repeat(40);
const mergedSha = "b".repeat(40);
const branch = "codex/release-123";
const run = {
	id: 11,
	head_sha: sha,
	head_branch: branch,
	event: "workflow_dispatch",
	path: ".github/workflows/ci.yml",
	status: "completed",
	conclusion: "success",
	html_url: "https://github.com/owner/repo/actions/runs/11",
};
const pr = {
	number: 3,
	state: "open",
	merged: false,
	base: { ref: "master" },
	head: { ref: branch, sha },
};

test("只认本次 dispatch 后正确提交、分支和 CI 工作流的运行", () => {
	const otherRuns = [
		{ ...run, id: 9 },
		{ ...run, id: 12, head_sha: mergedSha },
		{ ...run, id: 13, head_branch: "master" },
		{ ...run, id: 14, event: "pull_request" },
		{ ...run, id: 15, path: ".github/workflows/other.yml" },
	];
	assert.equal(
		dispatchedRun(otherRuns, { afterId: 10, sha, branch }),
		undefined,
	);
	assert.equal(
		dispatchedRun([...otherRuns, run], { afterId: 10, sha, branch }),
		run,
	);
});

test("明确 dispatch CI 并等待新运行，旧成功与待批准的 PR 运行不会被当作结果", async () => {
	let requests = 0;
	const writes = [];
	const api = async (endpoint, options) => {
		if (options) {
			writes.push({ endpoint, ...options });
			return null;
		}
		requests++;
		return {
			workflow_runs:
				requests === 1
					? [{ ...run, id: 10 }]
					: requests === 2
						? [
								{ ...run, id: 10 },
								{ ...run, event: "pull_request", status: "waiting" },
							]
						: [run],
		};
	};
	const result = await runCI(api, branch, sha, {
		sleep: async () => {},
		log: () => {},
	});
	assert.equal(result.id, 11);
	assert.equal(requests, 3);
	assert.deepEqual(writes, [
		{
			endpoint: "/actions/workflows/ci.yml/dispatches",
			method: "POST",
			body: { ref: branch },
		},
	]);
});

for (const conclusion of ["failure", "cancelled", "skipped", "timed_out"]) {
	test("CI " + conclusion + " 时中止发布", async () => {
		let reads = 0;
		await assert.rejects(
			runCI(
				async (_endpoint, options) =>
					options
						? null
						: { workflow_runs: ++reads === 1 ? [] : [{ ...run, conclusion }] },
				branch,
				sha,
				{ log: () => {} },
			),
			/CI 未通过/,
		);
	});
}

test("等待不到目标提交会超时，不使用另一提交的成功检查", async () => {
	let time = 0;
	await assert.rejects(
		runCI(
			async (_endpoint, options) =>
				options ? null : { workflow_runs: [{ ...run, head_sha: mergedSha }] },
			branch,
			sha,
			{
				now: () => time,
				sleep: async () => {
					time += 10;
				},
				timeout: 20,
				log: () => {},
			},
		),
		/超时/,
	);
});

test("版本 PR CI → 按 head 合并 → 合并提交 CI → 返回同一发布 SHA", async () => {
	const calls = [];
	const result = await completeVersionRelease(pr, {
		check: async (...args) => calls.push(["check", ...args]),
		merge: async (...args) => {
			calls.push(["merge", ...args]);
			return mergedSha;
		},
		assertCurrent: async (...args) => calls.push(["current", ...args]),
	});
	assert.equal(result, mergedSha);
	assert.deepEqual(calls, [
		["check", branch, sha],
		["merge", 3, sha],
		["current", "master", mergedSha],
		["check", "master", mergedSha],
	]);
});

test("版本提交 CI 失败时不会调用合并", async () => {
	let merged = false;
	await assert.rejects(
		completeVersionRelease(pr, {
			check: async () => {
				throw new Error("CI failed");
			},
			merge: async () => {
				merged = true;
			},
			assertCurrent: async () => {},
		}),
		/CI failed/,
	);
	assert.equal(merged, false);
});

test("合并后主分支改变或 CI 失败都不能返回可发布提交", async () => {
	for (const failCurrent of [false, true]) {
		let checks = 0;
		await assert.rejects(
			completeVersionRelease(pr, {
				check: async () => {
					if (++checks === 2) throw new Error("merged CI failed");
				},
				merge: async () => mergedSha,
				assertCurrent: async () => {
					if (failCurrent) throw new Error("branch moved");
				},
			}),
			failCurrent ? /branch moved/ : /merged CI failed/,
		);
	}
});

test("同一运行重试复用已合并 PR，不重复合并；已关闭未合并则停止", async () => {
	const calls = [];
	const handlers = {
		check: async (...args) => calls.push(args),
		merge: async () => {
			throw new Error("must not merge twice");
		},
		assertCurrent: async () => {},
	};
	assert.equal(
		await completeVersionRelease(
			{ ...pr, state: "closed", merged: true, merge_commit_sha: mergedSha },
			handlers,
		),
		mergedSha,
	);
	assert.deepEqual(calls, [["master", mergedSha]]);
	await assert.rejects(
		completeVersionRelease({ ...pr, state: "closed" }, handlers),
		/已关闭/,
	);
});

test("CI 状态传播延迟时有限重试合并，每次都固定 PR head", async () => {
	let puts = 0;
	const api = async (_endpoint, options) => {
		if (!options) return { object: { sha } };
		assert.deepEqual(options.body, { sha: mergedSha, merge_method: "squash" });
		if (++puts === 1)
			throw Object.assign(new Error("checks propagating"), { status: 405 });
		return { merged: true, sha: "c".repeat(40) };
	};
	assert.equal(
		await mergeVersionPR(api, 3, mergedSha, "master", sha, {
			sleep: async () => {},
		}),
		"c".repeat(40),
	);
	assert.equal(puts, 2);
});

test("主分支改变时不尝试合并，权限失败或提交冲突时不重试", async () => {
	let writes = 0;
	await assert.rejects(
		mergeVersionPR(
			async (_endpoint, options) => {
				if (options) writes++;
				return { object: { sha: mergedSha } };
			},
			3,
			mergedSha,
			"master",
			sha,
		),
		/主分支/,
	);
	assert.equal(writes, 0);
	for (const status of [403, 409]) {
		writes = 0;
		await assert.rejects(
			mergeVersionPR(
				async (_endpoint, options) => {
					if (!options) return { object: { sha } };
					writes++;
					throw Object.assign(new Error("blocked"), { status });
				},
				3,
				mergedSha,
				"master",
				sha,
			),
			/blocked/,
		);
		assert.equal(writes, 1);
	}
});

test("保护规则一直拒绝时停止，不进行无限轮询或管理员绕过", async () => {
	let writes = 0;
	await assert.rejects(
		mergeVersionPR(
			async (_endpoint, options) => {
				if (!options) return { object: { sha } };
				writes++;
				throw Object.assign(new Error("protected"), { status: 405 });
			},
			3,
			mergedSha,
			"master",
			sha,
			{ attempts: 2, sleep: async () => {} },
		),
		/protected/,
	);
	assert.equal(writes, 2);
});

test("手动 patch/minor/major 必须发布自动合并并通过校验的 SHA", () => {
	const repository = { default_branch: "master", full_name: "owner/repo" };
	for (const bump of ["patch", "minor", "major"]) {
		assert.equal(
			releaseTarget(
				"workflow_dispatch",
				{ repository, inputs: { bump } },
				sha,
				"refs/heads/master",
				mergedSha,
			).sha,
			mergedSha,
		);
		assert.equal(
			releaseTarget(
				"workflow_dispatch",
				{ repository, inputs: { bump } },
				sha,
				"refs/heads/master",
			),
			null,
		);
	}
	assert.throws(() =>
		releaseTarget(
			"workflow_dispatch",
			{ repository, inputs: { bump: "patch" } },
			sha,
			"refs/heads/master",
			"invalid",
		),
	);
});

test("重试只能复用预期版本文件，拒绝夹带源码、依赖脚本或篡改版本", () => {
	const expected = {
		"app/package.json": '{"version":"1.0.1","scripts":{"build":"tsc"}}',
		"app/src-tauri/tauri.conf.json": '{"version":"1.0.1"}',
		"CHANGELOG.md": "# Changelog\n\n## v1.0.1（2026-09-07）\n\n- feature\n",
	};
	assert.doesNotThrow(() =>
		assertVersionFiles(
			Object.keys(expected),
			{
				...expected,
				"CHANGELOG.md": expected["CHANGELOG.md"].replace(
					"2026-09-07",
					"2026-09-08",
				),
			},
			expected,
		),
	);
	assert.throws(
		() =>
			assertVersionFiles(
				[...Object.keys(expected), "app/src/App.tsx"],
				expected,
				expected,
			),
		/版本文件以外/,
	);
	assert.throws(
		() =>
			assertVersionFiles(
				Object.keys(expected),
				{
					...expected,
					"app/package.json":
						'{"version":"1.0.1","scripts":{"build":"malicious"}}',
				},
				expected,
			),
		/不一致/,
	);
	assert.throws(
		() =>
			assertVersionFiles(
				Object.keys(expected),
				{ ...expected, "app/src-tauri/tauri.conf.json": '{"version":"2.0.0"}' },
				expected,
			),
		/不一致/,
	);
});
