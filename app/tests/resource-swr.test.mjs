import assert from "node:assert/strict";
import { register } from "node:module";
import test from "node:test";

// store.ts 及其内部导入使用无扩展名相对路径（vite 风格），
// node 直接加载需要这个小钩子补 .ts 扩展名。
register("./ts-resolution-loader.mjs", import.meta.url);

const { peekSwr, pokeSwr, resetSwrCache, swrFetch } = await import(
	"../src/store.ts"
);

const deferred = () => {
	let resolve;
	let reject;
	const promise = new Promise((res, rej) => {
		resolve = res;
		reject = rej;
	});
	return { promise, resolve, reject };
};

const flush = () => new Promise((resolve) => setImmediate(resolve));

test("无缓存首次加载：先 loading，成功后写入缓存", async () => {
	resetSwrCache();
	const gate = deferred();
	const states = [];
	swrFetch({
		key: "p1:issues",
		cache: true,
		load: () => gate.promise,
		emit: (state) => states.push(state),
	});
	assert.deepEqual(states, [
		{ data: null, error: "", loading: true, refreshing: false },
	]);
	gate.resolve(["i1"]);
	await flush();
	assert.deepEqual(states, [
		{ data: null, error: "", loading: true, refreshing: false },
		{ data: ["i1"], error: "", loading: false, refreshing: false },
	]);
	assert.deepEqual(peekSwr("p1:issues"), ["i1"]);
});

test("命中缓存：立即显示上次结果并后台刷新，完成后替换", async () => {
	resetSwrCache();
	const first = deferred();
	const states = [];
	swrFetch({
		key: "p1:prs",
		cache: true,
		load: () => first.promise,
		emit: (state) => states.push(state),
	});
	first.resolve(["v1"]);
	await flush();
	states.length = 0;
	const second = deferred();
	swrFetch({
		key: "p1:prs",
		cache: true,
		load: () => second.promise,
		emit: (state) => states.push(state),
	});
	// 命中即显：同步拿到旧数据，不进入 loading，而是后台刷新
	assert.deepEqual(states, [
		{ data: ["v1"], error: "", loading: false, refreshing: true },
	]);
	second.resolve(["v2"]);
	await flush();
	assert.deepEqual(states, [
		{ data: ["v1"], error: "", loading: false, refreshing: true },
		{ data: ["v2"], error: "", loading: false, refreshing: false },
	]);
	assert.deepEqual(peekSwr("p1:prs"), ["v2"]);
});

test("错位响应丢弃：迟到请求不覆盖新结果", async () => {
	resetSwrCache();
	const slow = deferred();
	const fast = deferred();
	const seen = [];
	swrFetch({
		key: "p1:issue",
		cache: true,
		load: () => slow.promise,
		emit: (state) => seen.push(["slow", state]),
	});
	swrFetch({
		key: "p1:issue",
		cache: true,
		load: () => fast.promise,
		emit: (state) => seen.push(["fast", state]),
	});
	fast.resolve(["new"]);
	await flush();
	slow.resolve(["stale"]);
	await flush();
	// 缓存只保留新结果；慢请求只发过 initial emit，迟到结果被整体丢弃
	assert.deepEqual(peekSwr("p1:issue"), ["new"]);
	assert.deepEqual(
		seen.filter(([tag]) => tag === "slow"),
		[["slow", { data: null, error: "", loading: true, refreshing: false }]],
	);
	assert.deepEqual(seen[seen.length - 1], [
		"fast",
		{ data: ["new"], error: "", loading: false, refreshing: false },
	]);
});

test("刷新失败：保留旧数据并暴露错误，缓存不变", async () => {
	resetSwrCache();
	const ok = deferred();
	const states = [];
	swrFetch({
		key: "p2:workflows",
		cache: true,
		load: () => ok.promise,
		emit: (state) => states.push(state),
	});
	ok.resolve([{ id: 1 }]);
	await flush();
	states.length = 0;
	const bad = deferred();
	swrFetch({
		key: "p2:workflows",
		cache: true,
		load: () => bad.promise,
		emit: (state) => states.push(state),
	});
	bad.reject(new Error("boom"));
	await flush();
	// 先命中缓存立即显示旧数据，随后刷新失败：旧数据保留 + 错误暴露
	assert.deepEqual(states, [
		{ data: [{ id: 1 }], error: "", loading: false, refreshing: true },
		{
			data: [{ id: 1 }],
			error: "Error: boom",
			loading: false,
			refreshing: false,
		},
	]);
	assert.deepEqual(peekSwr("p2:workflows"), [{ id: 1 }]);
});

test("缓存按项目隔离", async () => {
	resetSwrCache();
	const a = deferred();
	const b = deferred();
	swrFetch({
		key: "pa:issues",
		cache: true,
		load: () => a.promise,
		emit: () => {},
	});
	swrFetch({
		key: "pb:issues",
		cache: true,
		load: () => b.promise,
		emit: () => {},
	});
	a.resolve(["from-a"]);
	b.resolve(["from-b"]);
	await flush();
	assert.deepEqual(peekSwr("pa:issues"), ["from-a"]);
	assert.deepEqual(peekSwr("pb:issues"), ["from-b"]);
});

test("卸载后返回的结果仍写入缓存，但不再通知调用方", async () => {
	resetSwrCache();
	const gate = deferred();
	const states = [];
	const cancel = swrFetch({
		key: "p3:runs",
		cache: true,
		load: () => gate.promise,
		emit: (state) => states.push(state),
	});
	cancel();
	gate.resolve(["done"]);
	await flush();
	assert.deepEqual(peekSwr("p3:runs"), ["done"]);
	assert.equal(states.length, 1);
});

test("cache: false 不读也不写缓存", async () => {
	resetSwrCache();
	const gate = deferred();
	const states = [];
	swrFetch({
		key: "p4:readme",
		cache: false,
		load: () => gate.promise,
		emit: (state) => states.push(state),
	});
	gate.resolve("content");
	await flush();
	assert.equal(peekSwr("p4:readme"), undefined);
	assert.deepEqual(states[states.length - 1], {
		data: "content",
		error: "",
		loading: false,
		refreshing: false,
	});
});

test("pokeSwr 写入缓存：后续请求命中先显，刷新成功后覆盖", async () => {
	resetSwrCache();
	// 本地乐观变更（如拖拽回填）直接进缓存
	pokeSwr("p5:pm:items", [{ id: "local" }]);
	const gate = deferred();
	const states = [];
	swrFetch({
		key: "p5:pm:items",
		cache: true,
		load: () => gate.promise,
		emit: (state) => states.push(state),
	});
	// 立即显示乐观变更后的内容，不进入 loading
	assert.deepEqual(states, [
		{
			data: [{ id: "local" }],
			error: "",
			loading: false,
			refreshing: true,
		},
	]);
	gate.resolve([{ id: "server" }]);
	await flush();
	assert.deepEqual(peekSwr("p5:pm:items"), [{ id: "server" }]);
});
