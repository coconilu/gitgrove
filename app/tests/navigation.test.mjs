import assert from "node:assert/strict";
import test from "node:test";
import {
	activeCheckout,
	ciStatus,
	navigate,
	previewParent,
	projectVisit,
	selectionProject,
	worktreePath,
} from "../src/navigation.ts";

const projects = [
	{
		id: "one",
		checkouts: [
			{ id: "one-main", isPrimary: true },
			{ id: "one-task", isPrimary: false },
		],
	},
	{ id: "two", checkouts: [{ id: "two-main", isPrimary: true }] },
];
test("文件预览经过其他项目后，恢复所属项目、工作树和页签", () => {
	const initial = {
		sel: { kind: "file", co: "one-task", key: "one-task/src/app.ts" },
		tab: "Actions",
		visits: {},
	};
	const second = navigate(projects, initial, {
		kind: "checkout",
		cid: "two-main",
	});
	assert.equal(second.tab, "Overview");
	const restored = projectVisit(projects, second.visits, "one");
	assert.deepEqual(restored, { sel: initial.sel, tab: "Actions" });
	assert.deepEqual(previewParent(restored.sel), {
		kind: "checkout",
		cid: "one-task",
	});
	assert.equal(selectionProject(projects, restored.sel).id, "one");
});
test("每个项目单独保留页签，删除的工作树不会被恢复为悬空选中项", () => {
	const visits = {
		one: { sel: { kind: "checkout", cid: "deleted" }, tab: "Issues" },
		two: { sel: { kind: "checkout", cid: "two-main" }, tab: "Pull Requests" },
	};
	assert.deepEqual(projectVisit(projects, visits, "one"), {
		sel: { kind: "project", pid: "one" },
		tab: "Issues",
	});
	assert.equal(
		navigate(
			projects,
			{ sel: { kind: "project", pid: "one" }, tab: "Actions", visits },
			{ kind: "project", pid: "two" },
		).tab,
		"Pull Requests",
	);
});
test("关联任务的 checkout 能解析到自己的项目，文件也不会退回第一个项目", () => {
	assert.equal(
		selectionProject(projects, { kind: "checkout", cid: "two-main" }).id,
		"two",
	);
	assert.equal(
		selectionProject(projects, {
			kind: "file",
			co: "two-main",
			key: "two-main/README.md",
		}).id,
		"two",
	);
	assert.equal(
		selectionProject(projects, { kind: "checkout", cid: "missing" }),
		undefined,
	);
});
test("取消、跳过、没有运行与失败保持不同语义", () => {
	assert.equal(ciStatus(null), "none");
	assert.equal(ciStatus({ status: "queued", conclusion: null }), "run");
	assert.equal(
		ciStatus({ status: "completed", conclusion: "cancelled" }),
		"cancelled",
	);
	assert.equal(
		ciStatus({ status: "completed", conclusion: "skipped" }),
		"skipped",
	);
	assert.equal(
		ciStatus({ status: "completed", conclusion: "timed_out" }),
		"fail",
	);
	assert.equal(ciStatus({ status: "completed", conclusion: null }), "unknown");
});
test("工作树目标目录预览与后端 ASCII slug 规则一致", () => {
	assert.equal(
		worktreePath("C:\\work\\Repo", "feat/My task"),
		"C:/work/Repo.worktrees/feat-my-task",
	);
	assert.equal(
		worktreePath("/repos/demo/", "功能"),
		"/repos/demo.worktrees/wt",
	);
	assert.equal(
		worktreePath("/repos/demo", "a".repeat(60)),
		"/repos/demo.worktrees/" + "a".repeat(40),
	);
});

test("当前工作树在项目入口、工作树选择、文件预览往返中保持一致", () => {
	assert.equal(
		activeCheckout(projects, { kind: "project", pid: "one" }).c.id,
		"one-main",
	);
	const file = { kind: "file", co: "one-task", key: "one-task/README.md" };
	assert.equal(activeCheckout(projects, file).c.id, "one-task");
	assert.equal(activeCheckout(projects, previewParent(file)).c.id, "one-task");
	assert.equal(
		activeCheckout(projects, { kind: "checkout", cid: "two-main" }).c.id,
		"two-main",
	);
	assert.equal(
		activeCheckout(projects, {
			kind: "file",
			co: "deleted",
			key: "deleted/README.md",
		}),
		null,
	);
	assert.equal(
		activeCheckout(projects, { kind: "project", pid: "deleted" }),
		null,
	);
	assert.equal(activeCheckout(projects, null), null);
});
