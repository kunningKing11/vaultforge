const MIN_THUMB_WIDTH = 36;
const WHEEL_SCROLL_MULTIPLIER = 1;

type HorizontalScrollElements = {
  scroll: HTMLElement;
  scrollbar: HTMLElement;
  thumb: HTMLElement;
};

type DragState = HorizontalScrollElements & {
  pointerId: number;
  grabOffset: number;
};

let dragState: DragState | null = null;
let resizeObserver: ResizeObserver | null = null;
let verticalDragState: { pointerId: number; grabOffset: number } | null = null;

function getElements(scroll: HTMLElement): HorizontalScrollElements | null {
  const scrollbar = scroll.parentElement?.querySelector<HTMLElement>("[data-horizontal-scrollbar]");
  const thumb = scrollbar?.querySelector<HTMLElement>("[data-horizontal-scrollbar-thumb]");

  return scrollbar && thumb ? { scroll, scrollbar, thumb } : null;
}

function maxScrollLeft(scroll: HTMLElement): number {
  return Math.max(0, scroll.scrollWidth - scroll.clientWidth);
}

function updateScrollbar(scroll: HTMLElement): void {
  const elements = getElements(scroll);
  if (!elements) return;

  const { scrollbar, thumb } = elements;
  const maxScroll = maxScrollLeft(scroll);
  const trackWidth = scrollbar.clientWidth;

  scrollbar.hidden = maxScroll === 0;
  scrollbar.tabIndex = maxScroll === 0 ? -1 : 0;
  scrollbar.setAttribute("aria-valuemax", String(Math.round(maxScroll)));
  scrollbar.setAttribute("aria-valuenow", String(Math.round(scroll.scrollLeft)));

  if (maxScroll === 0 || trackWidth === 0) return;

  const thumbWidth = Math.min(
    trackWidth,
    Math.max(MIN_THUMB_WIDTH, trackWidth * (scroll.clientWidth / scroll.scrollWidth)),
  );
  const thumbOffset = (scroll.scrollLeft / maxScroll) * (trackWidth - thumbWidth);
  thumb.style.width = `${thumbWidth}px`;
  thumb.style.transform = `translateX(${thumbOffset}px)`;
}

function scrollToThumbPosition(elements: HorizontalScrollElements, thumbOffset: number): void {
  const { scroll, scrollbar, thumb } = elements;
  const maxScroll = maxScrollLeft(scroll);
  const trackWidth = scrollbar.clientWidth;
  const thumbWidth = thumb.getBoundingClientRect().width;
  const maxThumbOffset = Math.max(0, trackWidth - thumbWidth);

  if (maxScroll === 0 || maxThumbOffset === 0) return;
  scroll.scrollLeft =
    Math.max(0, Math.min(maxThumbOffset, thumbOffset)) * (maxScroll / maxThumbOffset);
}

function syncAllScrollbars(): void {
  document.querySelectorAll<HTMLElement>("[data-horizontal-scroll]").forEach(updateScrollbar);
  syncVerticalScrollbar();
}

function verticalScrollbarElements(): { scrollbar: HTMLElement; thumb: HTMLElement } | null {
  const scrollbar = document.querySelector<HTMLElement>("[data-vertical-scrollbar]");
  const thumb = scrollbar?.querySelector<HTMLElement>("[data-vertical-scrollbar-thumb]");
  return scrollbar && thumb ? { scrollbar, thumb } : null;
}

function verticalMaxScroll(): number {
  return Math.max(0, document.documentElement.scrollHeight - window.innerHeight);
}

export function syncVerticalScrollbar(): void {
  const elements = verticalScrollbarElements();
  if (!elements) return;

  const { scrollbar, thumb } = elements;
  const maxScroll = verticalMaxScroll();
  const trackHeight = scrollbar.clientHeight;
  const scrollTop = window.scrollY;

  scrollbar.hidden = maxScroll === 0;
  scrollbar.tabIndex = maxScroll === 0 ? -1 : 0;
  scrollbar.setAttribute("aria-valuemax", String(Math.round(maxScroll)));
  scrollbar.setAttribute("aria-valuenow", String(Math.round(scrollTop)));

  if (maxScroll === 0 || trackHeight === 0) return;

  const thumbHeight = Math.min(
    trackHeight,
    Math.max(
      MIN_THUMB_WIDTH,
      trackHeight * (window.innerHeight / document.documentElement.scrollHeight),
    ),
  );
  const thumbOffset = (scrollTop / maxScroll) * (trackHeight - thumbHeight);
  thumb.style.height = `${thumbHeight}px`;
  thumb.style.transform = `translateY(${thumbOffset}px)`;
}

function scrollPageToThumbPosition(thumbOffset: number): void {
  const elements = verticalScrollbarElements();
  if (!elements) return;

  const { scrollbar, thumb } = elements;
  const maxScroll = verticalMaxScroll();
  const maxThumbOffset = Math.max(0, scrollbar.clientHeight - thumb.getBoundingClientRect().height);
  if (maxScroll === 0 || maxThumbOffset === 0) return;

  window.scrollTo({
    top: Math.max(0, Math.min(maxThumbOffset, thumbOffset)) * (maxScroll / maxThumbOffset),
  });
}

export function syncHorizontalScrollbars(scope: ParentNode = document): void {
  scope.querySelectorAll<HTMLElement>("[data-horizontal-scroll]").forEach((scroll) => {
    updateScrollbar(scroll);
    resizeObserver?.observe(scroll);
  });
}

export function installScrollbarBehavior(): void {
  if (resizeObserver) return;

  resizeObserver = new ResizeObserver((entries) => {
    entries.forEach((entry) => updateScrollbar(entry.target as HTMLElement));
  });

  document.addEventListener(
    "scroll",
    (event) => {
      const scroll = event.target;
      if (scroll instanceof HTMLElement && scroll.matches("[data-horizontal-scroll]")) {
        updateScrollbar(scroll);
      }
    },
    true,
  );

  document.addEventListener(
    "wheel",
    (event) => {
      const scroll = (event.target as HTMLElement).closest<HTMLElement>("[data-horizontal-scroll]");
      if (!scroll || Math.abs(event.deltaX) > Math.abs(event.deltaY)) return;

      const maxScroll = maxScrollLeft(scroll);
      const direction = Math.sign(event.deltaY);
      const canScroll =
        maxScroll > 0 &&
        ((direction > 0 && scroll.scrollLeft < maxScroll) ||
          (direction < 0 && scroll.scrollLeft > 0));

      if (!canScroll) return;

      event.preventDefault();
      scroll.scrollLeft += event.deltaY * WHEEL_SCROLL_MULTIPLIER;
    },
    { passive: false },
  );

  document.addEventListener("pointerdown", (event) => {
    const verticalScrollbar = (event.target as HTMLElement).closest<HTMLElement>(
      "[data-vertical-scrollbar]",
    );
    if (verticalScrollbar && !verticalScrollbar.hidden) {
      const thumb = verticalScrollbar.querySelector<HTMLElement>("[data-vertical-scrollbar-thumb]");
      if (!thumb) return;

      const scrollbarBounds = verticalScrollbar.getBoundingClientRect();
      const thumbBounds = thumb.getBoundingClientRect();
      const pointerOffset = event.clientY - scrollbarBounds.top;
      const startedOnThumb = thumb.contains(event.target as Node);
      const grabOffset = startedOnThumb ? event.clientY - thumbBounds.top : thumbBounds.height / 2;

      if (!startedOnThumb) scrollPageToThumbPosition(pointerOffset - grabOffset);

      verticalScrollbar.setPointerCapture(event.pointerId);
      verticalDragState = { pointerId: event.pointerId, grabOffset };
      event.preventDefault();
      return;
    }

    const scrollbar = (event.target as HTMLElement).closest<HTMLElement>(
      "[data-horizontal-scrollbar]",
    );
    if (!scrollbar || scrollbar.hidden) return;

    const scroll = scrollbar.parentElement?.querySelector<HTMLElement>("[data-horizontal-scroll]");
    const thumb = scrollbar.querySelector<HTMLElement>("[data-horizontal-scrollbar-thumb]");
    if (!scroll || !thumb) return;

    const elements = { scroll, scrollbar, thumb };
    const scrollbarBounds = scrollbar.getBoundingClientRect();
    const thumbBounds = thumb.getBoundingClientRect();
    const pointerOffset = event.clientX - scrollbarBounds.left;
    const startedOnThumb = thumb.contains(event.target as Node);
    const grabOffset = startedOnThumb ? event.clientX - thumbBounds.left : thumbBounds.width / 2;

    if (!startedOnThumb) scrollToThumbPosition(elements, pointerOffset - grabOffset);

    scrollbar.setPointerCapture(event.pointerId);
    dragState = { ...elements, pointerId: event.pointerId, grabOffset };
    event.preventDefault();
  });

  document.addEventListener("pointermove", (event) => {
    if (verticalDragState && event.pointerId === verticalDragState.pointerId) {
      const scrollbar = document.querySelector<HTMLElement>("[data-vertical-scrollbar]");
      if (!scrollbar) return;
      const scrollbarBounds = scrollbar.getBoundingClientRect();
      scrollPageToThumbPosition(event.clientY - scrollbarBounds.top - verticalDragState.grabOffset);
      event.preventDefault();
      return;
    }

    if (!dragState || event.pointerId !== dragState.pointerId) return;
    const scrollbarBounds = dragState.scrollbar.getBoundingClientRect();
    scrollToThumbPosition(dragState, event.clientX - scrollbarBounds.left - dragState.grabOffset);
    event.preventDefault();
  });

  document.addEventListener("pointerup", endDrag);
  document.addEventListener("pointercancel", endDrag);

  document.addEventListener("keydown", (event) => {
    const verticalScrollbar = (event.target as HTMLElement).closest<HTMLElement>(
      "[data-vertical-scrollbar]",
    );
    if (verticalScrollbar && !verticalScrollbar.hidden) {
      const increment = Math.max(40, window.innerHeight * 0.1);
      const pageIncrement = Math.max(increment, window.innerHeight * 0.9);
      const keyToOffset: Partial<Record<string, number>> = {
        ArrowUp: -increment,
        ArrowDown: increment,
        PageUp: -pageIncrement,
        PageDown: pageIncrement,
        Home: -verticalMaxScroll(),
        End: verticalMaxScroll(),
      };
      const offset = keyToOffset[event.key];

      if (offset === undefined) return;
      event.preventDefault();
      window.scrollBy({ top: offset });
      return;
    }

    const scrollbar = (event.target as HTMLElement).closest<HTMLElement>(
      "[data-horizontal-scrollbar]",
    );
    if (!scrollbar || scrollbar.hidden) return;

    const scroll = scrollbar.parentElement?.querySelector<HTMLElement>("[data-horizontal-scroll]");
    if (!scroll) return;

    const increment = Math.max(40, scroll.clientWidth * 0.1);
    const pageIncrement = Math.max(increment, scroll.clientWidth * 0.9);
    const keyToOffset: Partial<Record<string, number>> = {
      ArrowLeft: -increment,
      ArrowRight: increment,
      PageUp: -pageIncrement,
      PageDown: pageIncrement,
      Home: -maxScrollLeft(scroll),
      End: maxScrollLeft(scroll),
    };
    const offset = keyToOffset[event.key];

    if (offset === undefined) return;
    event.preventDefault();
    scroll.scrollLeft += offset;
  });

  window.addEventListener("resize", syncAllScrollbars);
  window.addEventListener("scroll", syncVerticalScrollbar, { passive: true });
  syncAllScrollbars();
}

function endDrag(event: PointerEvent): void {
  if (verticalDragState && event.pointerId === verticalDragState.pointerId) {
    document
      .querySelector<HTMLElement>("[data-vertical-scrollbar]")
      ?.releasePointerCapture(event.pointerId);
    verticalDragState = null;
    return;
  }

  if (!dragState || event.pointerId !== dragState.pointerId) return;
  dragState.scrollbar.releasePointerCapture(event.pointerId);
  dragState = null;
}
