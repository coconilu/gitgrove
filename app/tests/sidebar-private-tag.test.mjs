import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

// Sidebar.tsx 整体含 JSX，node 无法直接加载；
// 这里提取其中纯逻辑的 privateTag 定义求值后做行为断言。
const sidebarSrc = readFileSync(
	new URL("../src/components/Sidebar.tsx", import.meta.url),
	"utf8",
);
const start = sidebarSrc.indexOf("export const privateTag");
assert.ok(start >= 0, "Sidebar.tsx 应导出 privateTag");
const end = sidebarSrc.indexOf("\nexport ", start);
const defSrc = sidebarSrc
	.slice(start, end)
	.replace("export ", "")
	.replace(/: Pick<Project, "isPrivate">/, "");
const privateTag = new Function(`${defSrc}\nreturn privateTag;`)();

test("privateTag：私有项目得到 .priv 标签描述，文案为「私有」", () => {
	assert.deepEqual(privateTag({ isPrivate: true }), {
		className: "priv",
		label: "私有",
	});
});

test("privateTag：非私有项目不显示标签", () => {
	assert.equal(privateTag({ isPrivate: false }), null);
});

test("repo 分组的行把标签渲染在项目名与数量徽标之间", () => {
	assert.match(sidebarSrc, /const tag = privateTag\(p\);/);
	const row = sidebarSrc.match(
		/<span className="project-name">\{p\.name\}<\/span>([\s\S]*?)<span className="count-pill">/,
	);
	assert.ok(row, "应存在项目行结构");
	assert.match(row[1], /tag\.className/);
	assert.match(row[1], /tag\.label/);
});

test(".priv 样式不参与 flex 收缩，保证名称省略号不被挤压", () => {
	const css = readFileSync(
		new URL("../src/styles.css", import.meta.url),
		"utf8",
	);
	const rule = css.match(/\.priv \{([^}]*)\}/);
	assert.ok(rule, "styles.css 应有 .priv 规则");
	assert.match(rule[1], /flex:\s*none/);
});
