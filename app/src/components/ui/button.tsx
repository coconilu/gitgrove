import type { ButtonHTMLAttributes } from "react";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
	variant?: "default" | "primary" | "danger";
	size?: "default" | "sm";
}

/** 统一 hover / focus-visible / disabled 状态（见 styles.css 的 .btn 规则）。 */
export function Button({
	variant = "default",
	size = "default",
	className = "",
	type,
	...props
}: ButtonProps) {
	const cls = [
		"btn",
		variant !== "default" && variant,
		size === "sm" && "sm",
		className,
	]
		.filter(Boolean)
		.join(" ");
	return <button type={type ?? "button"} className={cls} {...props} />;
}
