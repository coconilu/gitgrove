import type { InputHTMLAttributes, TextareaHTMLAttributes } from "react";

/** 统一 hover / focus / disabled 状态（见 styles.css 的 .input 规则）。 */
export function Input({
	className = "",
	...props
}: InputHTMLAttributes<HTMLInputElement>) {
	return <input className={"input " + className} {...props} />;
}

export function Textarea({
	className = "",
	...props
}: TextareaHTMLAttributes<HTMLTextAreaElement>) {
	return <textarea className={"input " + className} {...props} />;
}
