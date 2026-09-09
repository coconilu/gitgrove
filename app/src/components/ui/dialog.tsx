import * as DialogPrimitive from "@radix-ui/react-dialog";
import type { ComponentProps, ReactNode } from "react";

export const DialogRoot = DialogPrimitive.Root;

/** 遮罩 + 内容，含进出场动画与焦点陷阱。样式复用 .dialog 的内部排版。 */
export function DialogContent({
	className = "",
	children,
	...props
}: ComponentProps<typeof DialogPrimitive.Content>) {
	return (
		<DialogPrimitive.Portal>
			<DialogPrimitive.Overlay className="ui-dialog-overlay" />
			<DialogPrimitive.Content
				className={"dialog ui-dialog-content " + className}
				{...props}
			>
				{children}
			</DialogPrimitive.Content>
		</DialogPrimitive.Portal>
	);
}

export function DialogTitle({
	className = "",
	children,
}: {
	className?: string;
	children: ReactNode;
}) {
	return (
		<DialogPrimitive.Title asChild>
			<h3 className={className}>{children}</h3>
		</DialogPrimitive.Title>
	);
}

export function DialogDescription({
	className = "",
	children,
}: {
	className?: string;
	children: ReactNode;
}) {
	return (
		<DialogPrimitive.Description asChild>
			<div className={className}>{children}</div>
		</DialogPrimitive.Description>
	);
}
