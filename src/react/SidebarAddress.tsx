import { useLayoutEffect, useRef, useState } from "react";

/** Remove whole characters, keeping the ellipsis between the address ends. */
export function fitSidebarAddress(
  address: string,
  width: number,
  measure: (text: string) => number,
): string {
  if (address.length <= 18 && measure(address) <= width) return address;
  let prefix = Math.min(10, Math.max(1, address.length - 8));
  let suffix = Math.min(8, address.length - prefix);
  const text = () => `${address.slice(0, prefix)}...${address.slice(-suffix)}`;
  while (prefix > 1 && measure(text()) > width) prefix--;
  while (suffix > 1 && measure(text()) > width) suffix--;
  return text();
}

export function SidebarAddress({ address }: { address: string }) {
  const ref = useRef<HTMLSpanElement>(null);
  const [text, setText] = useState(address);
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    const context = document.createElement("canvas").getContext("2d");
    if (!context) return;
    let disposed = false;
    const update = () => {
      if (disposed) return;
      context.font = getComputedStyle(element).font;
      setText(
        fitSidebarAddress(
          address,
          Math.max(0, element.clientWidth - 1),
          (value) => context.measureText(value).width,
        ),
      );
    };
    const observer = new ResizeObserver(update);
    observer.observe(element);
    void document.fonts.ready.then(update);
    update();
    return () => {
      disposed = true;
      observer.disconnect();
    };
  }, [address]);
  return (
    <span className="sidebar-address-value" ref={ref}>
      {text}
    </span>
  );
}
