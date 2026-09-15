import assert from "node:assert/strict";
import { register } from "node:module";
import test from "node:test";

// store.ts 及其内部导入使用无扩展名相对路径（vite 风格），
// node 直接加载需要这个小钩子补 .ts 扩展名。
register("./ts-resolution-loader.mjs", import.meta.url);

// node 环境可能没有 localStorage，先垫一个内存实现，
// 让 store 模块初始化时的 readLayout 与动作里的 setLayout 都走真实路径。
const backing = new Map();
globalThis.localStorage ??= {
	getItem: (key) => (backing.has(key) ? backing.get(key) : null),
	setItem: (key, value) => backing.set(key, String(value)),
	removeItem: (key) => backing.delete(key),
	clear: () => backing.clear(),
};

const { expandedIds, normalizeExpanded, readExpandedProjects, useStore } =
	await import("../src/store.ts");

const persisted = () => {
	const raw = localStorage.getItem("gitgrove.layout.v1");
	return raw ? JSON.parse(raw) : null;
};

test("normalizeExpanded：缺省即收起，只保留仍存在且展开中的项目", () => {
	assert.deepEqual(
		normalizeExpanded(
			{ a: true, b: false, stale: true, gone: true },
			["a", "b", "c"],
		),
		{ a: true },
	);
	// 空记录 → 全部收起
	assert.deepEqual(normalizeExpanded({}, ["a", "b"]), {});
	assert.deepEqual(normalizeExpanded({ a: true }, []), {});
});

test("expandedIds：只取展开中的 id，顺序稳定且去重", () => {
	assert.deepEqual(expandedIds({ a: true, b: false, c: true }), ["a", "c"]);
	assert.deepEqual(expandedIds({}), []);
});

test("readExpandedProjects：坏数据整体丢弃，正常数据去重", () => {
	assert.deepEqual(readExpandedProjects(["a", "b"]), ["a", "b"]);
	assert.deepEqual(readExpandedProjects(["a", "a"]), ["a"]);
	assert.deepEqual(readExpandedProjects("a"), []);
	assert.deepEqual(readExpandedProjects(["a", 1]), []);
	assert.deepEqual(readExpandedProjects(null), []);
	assert.deepEqual(readExpandedProjects(undefined), []);
});

test("toggleProject：未记录的项目第一次展开（缺省收起语义）", () => {
	const s = useStore.getState();
	s.toggleProject("p1");
	assert.equal(useStore.getState().expanded.p1, true);
	// 展开状态持久化到本地设置
	assert.deepEqual(persisted().expandedProjects, ["p1"]);
	useStore.getState().toggleProject("p1");
	assert.equal(useStore.getState().expanded.p1, false);
	assert.deepEqual(persisted().expandedProjects, []);
});

test("setAllExpanded：作用于当前项目列表并持久化，不触发选中", () => {
	useStore.setState({
		projects: [
			{ id: "p1", checkouts: [] },
			{ id: "p2", checkouts: [] },
		],
		sel: { kind: "project", pid: "p2" },
	});
	useStore.getState().setAllExpanded(true);
	assert.deepEqual(useStore.getState().expanded, { p1: true, p2: true });
	assert.deepEqual(persisted().expandedProjects, ["p1", "p2"]);
	assert.equal(useStore.getState().sel.kind, "project");

	useStore.getState().setAllExpanded(false);
	assert.deepEqual(useStore.getState().expanded, {});
	assert.deepEqual(persisted().expandedProjects, []);
});

test("setGroupBy：状态与本地设置同步持久化", () => {
	useStore.getState().setGroupBy("status");
	assert.equal(useStore.getState().groupBy, "status");
	assert.equal(persisted().groupBy, "status");
	useStore.getState().setGroupBy("repo");
	assert.equal(persisted().groupBy, "repo");
});
