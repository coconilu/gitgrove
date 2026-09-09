import * as DropdownMenuPrimitive from "@radix-ui/react-dropdown-menu";
import type { ComponentProps } from "react";

export const DropdownMenu = DropdownMenuPrimitive.Root;

export function DropdownMenuTrigger({
	className = "",
	children,
	...props
}: ComponentProps<typeof DropdownMenuPrimitive.Trigger>) {
	return (
		<DropdownMenuPrimitive.Trigger
			className={"ui-menu-trigger " + className}
			{...props}
		>
			{children}
		</DropdownMenuPrimitive.Trigger>
	);
}

export function DropdownMenuContent({
	className = "",
	children,
	...props
}: ComponentProps<typeof DropdownMenuPrimitive.Content>) {
	return (
		<DropdownMenuPrimitive.Portal>
			<DropdownMenuPrimitive.Content
				className={"ui-menu-content " + className}
				align="end"
				sideOffset={5}
				{...props}
			>
				{children}
			</DropdownMenuPrimitive.Content>
		</DropdownMenuPrimitive.Portal>
	);
}

export function DropdownMenuItem({
	className = "",
	children,
	...props
}: ComponentProps<typeof DropdownMenuPrimitive.Item>) {
	return (
		<DropdownMenuPrimitive.Item
			className={"ui-menu-item " + className}
			{...props}
		>
			{children}
		</DropdownMenuPrimitive.Item>
	);
}

export function DropdownMenuSeparator() {
	return <DropdownMenuPrimitive.Separator className="ui-menu-separator" />;
}
