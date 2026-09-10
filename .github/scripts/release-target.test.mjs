import assert from "node:assert/strict";
import test from "node:test";
import {
	hasRequiredCheck,
	releaseTag,
	releaseTarget,
} from "./release-target.mjs";

const sha = "a".repeat(40);
const repository = { default_branch: "master", full_name: "owner/repo" };
test("手动升级版本等待自动准备结果，随后发布合并提交；none 使用触发提交", () => {
	assert.equal(
		releaseTarget(
			"workflow_dispatch",
			{ repository, inputs: { bump: "patch" } },
			sha,
			"refs/heads/master",
		),
		null,
	);
	assert.equal(
		releaseTarget(
			"workflow_dispatch",
			{ repository, inputs: { bump: "none" } },
			sha,
			"refs/heads/master",
		).sha,
		sha,
	);
	assert.throws(() =>
		releaseTarget(
			"workflow_dispatch",
			{ repository, inputs: { bump: "none" } },
			sha,
			"refs/heads/feature",
		),
	);
});
test("完成 Issue 出隔离的预发布，取消 Issue 不发版", () => {
	const event = {
		repository,
		action: "closed",
		issue: { number: 7, state_reason: "completed" },
	};
	assert.deepEqual(releaseTarget("issues", event, sha), {
		sha,
		prerelease: true,
		issue: 7,
	});
	assert.equal(
		releaseTarget(
			"issues",
			{ ...event, issue: { state_reason: "not_planned" } },
			sha,
		),
		null,
	);
	assert.equal(releaseTag("1.2.3", "1.2.3", 7), "v1.2.3-issue7");
	assert.equal(releaseTag("1.2.3", "1.2.3", null), "v1.2.3");
	assert.throws(() => releaseTag("1.2.3", "1.2.4"));
});
test("只接受 GitHub Actions 最新的 check 成功，不接受旧成功/第三方同名/跳过", () => {
	const success = {
		id: 1,
		name: "check",
		app: { id: 15368 },
		status: "completed",
		conclusion: "success",
	};
	assert.equal(hasRequiredCheck([success]), true);
	assert.equal(hasRequiredCheck([{ ...success, app: { id: 123 } }]), false);
	assert.equal(
		hasRequiredCheck([success, { ...success, id: 2, conclusion: "failure" }]),
		false,
	);
	assert.equal(
		hasRequiredCheck([{ ...success, conclusion: "skipped" }]),
		false,
	);
	assert.equal(hasRequiredCheck([]), false);
});
