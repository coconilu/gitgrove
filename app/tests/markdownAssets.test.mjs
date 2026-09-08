import assert from "node:assert/strict";
import test from "node:test";
import {
	isLocalImageSrc,
	resolveLocalPath,
	rewriteLocalImages,
} from "../src/markdownAssets.ts";

test("isLocalImageSrc: 相对路径视为本地", () => {
	assert.equal(isLocalImageSrc("docs/assets/icon.png"), true);
	assert.equal(isLocalImageSrc("./a.png"), true);
	assert.equal(isLocalImageSrc("../a.png"), true);
	assert.equal(isLocalImageSrc("/docs/a.png"), true);
});

test("isLocalImageSrc: 远程与特殊地址保持不变", () => {
	assert.equal(isLocalImageSrc("https://img.shields.io/badge/x"), false);
	assert.equal(isLocalImageSrc("http://example.com/a.png"), false);
	assert.equal(isLocalImageSrc("//cdn.example.com/a.png"), false);
	assert.equal(isLocalImageSrc("data:image/png;base64,AAAA"), false);
	assert.equal(isLocalImageSrc("asset://localhost/a.png"), false);
	assert.equal(isLocalImageSrc("#anchor"), false);
});

test("resolveLocalPath: 拼接相对路径", () => {
	assert.equal(
		resolveLocalPath("C:/proj/repo", "docs/assets/icon.png"),
		"C:/proj/repo/docs/assets/icon.png",
	);
	assert.equal(
		resolveLocalPath("C:/proj/repo", "./a.png"),
		"C:/proj/repo/a.png",
	);
	assert.equal(
		resolveLocalPath("C:/proj/repo/docs", "../assets/a.png"),
		"C:/proj/repo/assets/a.png",
	);
});

test("resolveLocalPath: Windows 反斜杠基底保持分隔符", () => {
	assert.equal(
		resolveLocalPath("C:\\proj\\repo", "docs/a.png"),
		"C:\\proj\\repo\\docs\\a.png",
	);
});

test("resolveLocalPath: 前导斜杠按基底目录处理", () => {
	assert.equal(
		resolveLocalPath("C:/proj/repo", "/docs/a.png"),
		"C:/proj/repo/docs/a.png",
	);
});

test("resolveLocalPath: 剥离查询串并解码 % 转义", () => {
	assert.equal(resolveLocalPath("C:/proj", "a.png?raw=true"), "C:/proj/a.png");
	assert.equal(resolveLocalPath("C:/proj", "a%20b.png"), "C:/proj/a b.png");
});

test("resolveLocalPath: POSIX 基底保留根斜杠", () => {
	assert.equal(
		resolveLocalPath("/home/u/proj", "docs/a.png"),
		"/home/u/proj/docs/a.png",
	);
});

test("resolveLocalPath: .. 不会越过基底根", () => {
	assert.equal(resolveLocalPath("C:/proj", "../../a.png"), "C:/a.png");
});

test("rewriteLocalImages: 改写相对 src 并保留其他属性", () => {
	const html =
		'<p align="center"><img src="docs/assets/icon.png" width="96" alt="logo" /></p>';
	const out = rewriteLocalImages(html, "C:/proj/repo", (p) => `asset://${p}`);
	assert.equal(
		out,
		'<p align="center"><img src="asset://C:/proj/repo/docs/assets/icon.png" width="96" alt="logo" /></p>',
	);
});

test("rewriteLocalImages: 远程图片与无 src 的 img 不动", () => {
	const html =
		'<img src="https://img.shields.io/badge/x" alt="b"><img src="data:image/png;base64,AA"><img alt="none">';
	assert.equal(
		rewriteLocalImages(html, "C:/proj", (p) => `asset://${p}`),
		html,
	);
});

test("rewriteLocalImages: 单引号与无引号 src", () => {
	const toUrl = (p) => `asset://${p}`;
	assert.equal(
		rewriteLocalImages("<img src='a.png'>", "C:/proj", toUrl),
		"<img src='asset://C:/proj/a.png'>",
	);
	assert.equal(
		rewriteLocalImages("<img src=a.png>", "C:/proj", toUrl),
		'<img src="asset://C:/proj/a.png">',
	);
});

test("rewriteLocalImages: 逐张解析，多张图片互不影响", () => {
	const html =
		'<img src="a.png"><img src="sub/b.png"><img src="//cdn.x/y.png">';
	assert.equal(
		rewriteLocalImages(html, "/repo", (p) => `asset://${p}`),
		'<img src="asset:///repo/a.png"><img src="asset:///repo/sub/b.png"><img src="//cdn.x/y.png">',
	);
});
