import { useImperativeHandle, useLayoutEffect, useRef } from "react";
import type { ComponentProps } from "react";
import * as Primitive from "@radix-ui/react-dialog";

export * from "@radix-ui/react-dialog";

export function Content({ ref, onOpenAutoFocus, onCloseAutoFocus, onKeyDownCapture, ...props }: ComponentProps<typeof Primitive.Content> & { inert: boolean }) {
  const element = useRef<HTMLDivElement>(null);
  const opener = useRef<HTMLElement | null>(null);
  const wasClosed = useRef(props.inert);
  // Portal's Presence needs the actual node to retain it through the exit animation.
  useImperativeHandle(ref, () => element.current!, []);
  useLayoutEffect(() => {
    if (wasClosed.current && !props.inert && !element.current?.contains(document.activeElement)) element.current?.focus();
    wasClosed.current = props.inert;
  }, [props.inert]);

  return <Primitive.Content {...props} ref={element}
    onKeyDownCapture={(event) => {
      // A key can arrive before the browser blurs a newly inert closing control.
      if (props.inert) { event.preventDefault(); event.stopPropagation(); }
      else onKeyDownCapture?.(event);
    }}
    onOpenAutoFocus={(event) => {
      opener.current = document.activeElement as HTMLElement;
      onOpenAutoFocus?.(event);
    }}
    onCloseAutoFocus={(event) => {
      const id = (event.target as HTMLElement).id;
      if (!props.inert || document.getElementById(id)?.dataset.state === "open") { event.preventDefault(); return; }
      onCloseAutoFocus?.(event);
      if (event.defaultPrevented) return;
      event.preventDefault();
      const trigger = document.querySelector<HTMLElement>(`[aria-haspopup="dialog"][aria-controls="${CSS.escape(id)}"]`);
      // Menu-launched tools have no Dialog.Trigger, and their menu item has unmounted.
      const target = trigger ?? (opener.current?.isConnected && opener.current !== document.body ? opener.current : document.querySelector<HTMLElement>('[aria-label="More tools"]'));
      const remaining = [...document.querySelectorAll<HTMLElement>('[role="dialog"][data-state="open"], [role="alertdialog"][data-state="open"]')].at(-1);
      if (remaining && (remaining.contains(document.activeElement) || !remaining.contains(target))) return;
      target?.focus();
    }} />;
}
