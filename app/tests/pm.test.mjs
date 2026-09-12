import assert from "node:assert/strict";
import test from "node:test";
import {
	computeMove,
	DEFAULT_STATUSES,
	DONE_COLLAPSE_BATCH,
	dayDiff,
	doneColumnView,
	doneRecencyKey,
	doneRecencySort,
	doneStatusId,
	dueInfo,
	filterActive,
	filterItems,
	focusSummary,
	groupByStatus,
	issueUrl,
	milestoneProgress,
	parseGithubRef,
	pmSyncWithTimeout,
	repoName,
	sortMilestones,
	syncBannerFromOutcome,
	todayString,
} from "../src/components/pm/model.ts";

const item = (over) => ({
	id: "i1",
	title: "任务",
	body: "",
	status: "todo",
	priority: "none",
	milestoneId: null,
	labels: [],
	repoPath: null,
	branch: null,
	dueDate: null,
	order: "a",
	githubRef: null,
	manualLock: false,
	createdAt: 1,
	updatedAt: 1,
	...over,
});

test("filterItems 按里程碑/仓库/标签/优先级/搜索过滤", () => {
	const items = [
		item({ id: "a", milestoneId: "m1", repoPath: "/r/x", labels: ["ui"] }),
		item({ id: "b", priority: "high", title: "修 bug", body: "crash" }),
		item({ id: "c", milestoneId: "m2", labels: ["ui", "infra"] }),
	];
	const f = {
		milestoneId: null,
		repoPath: null,
		label: null,
		priority: null,
		search: "",
	};
	assert.equal(filterItems(items, f).length, 3);
	assert.deepEqual(
		filterItems(items, { ...f, milestoneId: "m1" }).map((i) => i.id),
		["a"],
	);
	assert.deepEqual(
		filterItems(items, { ...f, repoPath: "/r/x" }).map((i) => i.id),
		["a"],
	);
	assert.deepEqual(
		filterItems(items, { ...f, label: "ui" }).map((i) => i.id),
		["a", "c"],
	);
	assert.deepEqual(
		filterItems(items, { ...f, priority: "high" }).map((i) => i.id),
		["b"],
	);
	assert.deepEqual(
		filterItems(items, { ...f, search: "CRASH" }).map((i) => i.id),
		["b"],
	);
});

test("groupByStatus 按列分组并按 order 排序，未知状态被丢弃", () => {
	const items = [
		item({ id: "a", status: "todo", order: "b" }),
		item({ id: "b", status: "todo", order: "a" }),
		item({ id: "c", status: "done" }),
		item({ id: "d", status: "archived" }),
	];
	const groups = groupByStatus(items, DEFAULT_STATUSES);
	assert.deepEqual(
		groups.get("todo").map((i) => i.id),
		["b", "a"],
	);
	assert.deepEqual(
		groups.get("done").map((i) => i.id),
		["c"],
	);
	assert.equal(groups.get("backlog").length, 0);
});

test("doneStatusId 取最后一列，milestoneProgress 分段聚合", () => {
	assert.equal(doneStatusId(DEFAULT_STATUSES), "done");
	const items = [
		item({ id: "a", milestoneId: "m1", status: "done" }),
		item({ id: "b", milestoneId: "m1", status: "doing" }),
		item({ id: "c", milestoneId: "m1", status: "backlog" }),
		item({ id: "d", milestoneId: "m1", status: "todo" }),
		item({ id: "e", milestoneId: "m2", status: "done" }),
	];
	const p = milestoneProgress(items, "m1", "done");
	assert.deepEqual(
		{ total: p.total, done: p.done, doing: p.doing, rest: p.rest },
		{ total: 4, done: 1, doing: 1, rest: 2 },
	);
	assert.equal(p.pct, 25);
	assert.equal(milestoneProgress(items, "m3", "done").pct, 0);
});

test("dueInfo 倒计时与逾期", () => {
	const today = "2026-09-10";
	assert.deepEqual(dueInfo("2026-09-10", today), {
		short: "09-10",
		text: "今天到期",
		overdue: false,
		dueToday: true,
	});
	const future = dueInfo("2026-09-20", today);
	assert.equal(future.text, "还剩 10 天");
	assert.equal(future.overdue, false);
	const past = dueInfo("2026-09-08", today);
	assert.equal(past.text, "已逾期 2 天");
	assert.equal(past.overdue, true);
	assert.equal(dayDiff("2026-01-01", "2026-12-31"), 364);
});

test("focusSummary 聚合 doing 与今日到期（排除完成列）", () => {
	const today = "2026-09-10";
	const items = [
		item({ id: "a", status: "doing" }),
		item({ id: "b", status: "todo", dueDate: today }),
		item({ id: "c", status: "done", dueDate: today }),
	];
	const focus = focusSummary(items, today, "done");
	assert.deepEqual(
		focus.doing.map((i) => i.id),
		["a"],
	);
	assert.deepEqual(
		focus.dueToday.map((i) => i.id),
		["b"],
	);
});

test("sortMilestones：开放在前、按截止日升序、无截止最后、关闭垫底", () => {
	const ms = (over) => ({
		id: "m",
		title: "m",
		description: "",
		dueDate: null,
		status: "open",
		githubRef: null,
		createdAt: 1,
		updatedAt: 1,
		total: 0,
		done: 0,
		...over,
	});
	const sorted = sortMilestones([
		ms({ id: "noDue" }),
		ms({ id: "closed", status: "closed", dueDate: "2026-01-01" }),
		ms({ id: "late", dueDate: "2026-10-01" }),
		ms({ id: "soon", dueDate: "2026-09-20" }),
	]);
	assert.deepEqual(
		sorted.map((m) => m.id),
		["soon", "late", "noDue", "closed"],
	);
});

test("computeMove：跨列落列尾、插到目标卡片前、原地不动返回 null", () => {
	const items = [
		item({ id: "a", status: "todo", order: "a" }),
		item({ id: "b", status: "todo", order: "b" }),
		item({ id: "c", status: "doing", order: "a" }),
	];
	assert.deepEqual(computeMove(items, DEFAULT_STATUSES, "a", "doing"), {
		toStatus: "doing",
		beforeItemId: null,
	});
	assert.deepEqual(computeMove(items, DEFAULT_STATUSES, "a", "c"), {
		toStatus: "doing",
		beforeItemId: "c",
	});
	assert.deepEqual(computeMove(items, DEFAULT_STATUSES, "a", "b"), {
		toStatus: "todo",
		beforeItemId: "b",
	});
	assert.equal(computeMove(items, DEFAULT_STATUSES, "a", "a"), null);
	assert.equal(computeMove(items, DEFAULT_STATUSES, "a", "todo"), null);
	assert.equal(computeMove(items, DEFAULT_STATUSES, "a", null), null);
	assert.equal(computeMove(items, DEFAULT_STATUSES, "missing", "todo"), null);
});

test("repoName 取路径末段", () => {
	assert.equal(repoName("C:\\work\\gitgrove"), "gitgrove");
	assert.equal(repoName("/repos/demo/"), "demo");
	assert.equal(todayString(new Date(2026, 8, 10)), "2026-09-10");
});

test("parseGithubRef 解析 owner/repo#number，非法格式返回 null", () => {
	assert.deepEqual(parseGithubRef("coconilu/gitgrove#57"), {
		owner: "coconilu",
		repo: "gitgrove",
		number: 57,
	});
	assert.deepEqual(parseGithubRef("o/r#1"), {
		owner: "o",
		repo: "r",
		number: 1,
	});
	// 非法格式：缺 number / 多段 / 空串 / 杂质
	assert.equal(parseGithubRef("coconilu/gitgrove#"), null);
	assert.equal(parseGithubRef("coconilu/gitgrove"), null);
	assert.equal(parseGithubRef("#57"), null);
	assert.equal(parseGithubRef("a/b/c#12"), null);
	assert.equal(parseGithubRef(""), null);
});

test("issueUrl 拼出 issue 链接，解析失败返回 null", () => {
	assert.equal(
		issueUrl("coconilu/gitgrove#57"),
		"https://github.com/coconilu/gitgrove/issues/57",
	);
	assert.equal(issueUrl("bad-ref"), null);
});

test("sync 悬挂由前端超时兜底收敛为 timeout，永不阻塞看板（#61）", async () => {
	const hang = () => new Promise(() => {}); // 模拟同步悬挂：永不 settle
	// 本地数据加载与同步无关（PmPanel 里同步是 fire-and-forget，不参与首屏渲染）
	assert.deepEqual(await pmSyncWithTimeout("p1", { syncGithub: hang }, 10), {
		kind: "timeout",
	});
});

test("pmSyncWithTimeout：失败与成功分别收敛为 failed/synced", async () => {
	const failed = await pmSyncWithTimeout(
		"p1",
		{
			syncGithub: async () => {
				throw new Error("boom");
			},
		},
		10,
	);
	assert.deepEqual(failed, { kind: "failed", error: "Error: boom" });
	const synced = await pmSyncWithTimeout(
		"p1",
		{
			syncGithub: async () => ({ created: 1, updated: 2, moved: 3 }),
		},
		10,
	);
	assert.deepEqual(synced, {
		kind: "synced",
		result: { created: 1, updated: 2, moved: 3 },
	});
});

test("syncBannerFromOutcome：失败/超时映射为常驻错误横幅（#66 静默吞错回归）", () => {
	assert.deepEqual(
		syncBannerFromOutcome({ kind: "failed", error: "未登录：无可用 token" }),
		{ kind: "error", message: "GitHub 同步失败：未登录：无可用 token" },
	);
	assert.deepEqual(syncBannerFromOutcome({ kind: "timeout" }), {
		kind: "error",
		message: "GitHub 同步超时，可稍后手动刷新",
	});
});

test("syncBannerFromOutcome：有变更提示成功，无变更返回 null", () => {
	assert.deepEqual(
		syncBannerFromOutcome({
			kind: "synced",
			result: { created: 3, updated: 1, moved: 2 },
		}),
		{ kind: "changed", message: "GitHub 同步：新增 3 · 更新 1 · 迁移 2" },
	);
	assert.equal(
		syncBannerFromOutcome({
			kind: "synced",
			result: { created: 0, updated: 0, moved: 0 },
		}),
		null,
	);
});

test("filterActive：任一筛选器非「全部」或搜索框有输入时折叠失效（#80）", () => {
	const f = {
		milestoneId: null,
		repoPath: null,
		label: null,
		priority: null,
		search: "",
	};
	assert.equal(filterActive(f), false);
	assert.equal(filterActive({ ...f, search: "   " }), false); // 纯空白不算输入
	assert.equal(filterActive({ ...f, search: "bug" }), true);
	assert.equal(filterActive({ ...f, milestoneId: "m1" }), true);
	assert.equal(filterActive({ ...f, repoPath: "/r/x" }), true);
	assert.equal(filterActive({ ...f, label: "ui" }), true);
	assert.equal(filterActive({ ...f, priority: "high" }), true);
});

test("doneRecencySort：closedAt 优先，老数据（无 closedAt）回退 updatedAt", () => {
	// closedAt 较新但 updatedAt 较旧（标题/标签刷新会扰动 updatedAt）→ 仍排前面
	const items = [
		item({ id: "label-refreshed", updatedAt: 900 }),
		item({ id: "recently-done", closedAt: 800, updatedAt: 100 }),
		item({ id: "old-done", closedAt: 100, updatedAt: 200 }),
		item({ id: "legacy", updatedAt: 500 }), // v4 前老数据：closedAt 缺失
	];
	assert.deepEqual(
		doneRecencySort(items).map((i) => i.id),
		["label-refreshed", "recently-done", "legacy", "old-done"],
	);
	assert.equal(doneRecencyKey(items[3]), 500);
	// closedAt 为 null（后端老行显式 null）同样回退 updatedAt
	assert.equal(doneRecencyKey({ ...items[3], closedAt: null }), 500);
});

test("doneColumnView：默认只留最近 20 张（倒序），分批展开，有筛选/搜索时全显", () => {
	const done = Array.from({ length: 45 }, (_, n) =>
		item({
			id: `d${n}`,
			status: "done",
			order: `k${String(n).padStart(3, "0")}`,
			// closedAt 与列序相反：order 靠前的是老卡
			closedAt: 1000 - n,
			updatedAt: 1000 - n,
		}),
	);
	const none = {
		milestoneId: null,
		repoPath: null,
		label: null,
		priority: null,
		search: "",
	};

	// 默认折叠：最近 20 张（closedAt 倒序 → d0..d19），其余 25 张隐藏
	const folded = doneColumnView(done, none, DONE_COLLAPSE_BATCH);
	assert.equal(folded.visible.length, DONE_COLLAPSE_BATCH);
	assert.equal(folded.hiddenCount, 25);
	assert.deepEqual(folded.visible.map((i) => i.id).slice(0, 3), [
		"d0",
		"d1",
		"d2",
	]);

	// 每批展开 20：两批后 40 张可见、剩 5 张
	const oneBatch = doneColumnView(done, none, DONE_COLLAPSE_BATCH * 2);
	assert.equal(oneBatch.visible.length, 40);
	assert.equal(oneBatch.hiddenCount, 5);
	const allShown = doneColumnView(done, none, DONE_COLLAPSE_BATCH * 3);
	assert.equal(allShown.visible.length, 45);
	assert.equal(allShown.hiddenCount, 0);
	// 完全展开后保持列内 order 序（拖拽排序不被倒序视图覆盖）
	assert.deepEqual(
		allShown.visible.map((i) => i.id).slice(0, 3),
		["d0", "d1", "d2"].map((id) => id), // order 递增 = closedAt 递减，此处一致
	);

	// 搜索/筛选绕过折叠：全部命中卡片按原序渲染
	assert.deepEqual(doneColumnView(done, { ...none, search: "任务" }, 20), {
		visible: done,
		hiddenCount: 0,
	});
	assert.deepEqual(doneColumnView(done, { ...none, priority: "high" }, 20), {
		visible: done,
		hiddenCount: 0,
	});

	// 不超限时不折叠
	assert.deepEqual(doneColumnView(done.slice(0, 20), none, 20), {
		visible: done.slice(0, 20),
		hiddenCount: 0,
	});
});
