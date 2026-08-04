const MIN_THUMB_SIZE = 36;
const WHEEL_SCROLL_MULTIPLIER = 1;

type HorizontalScrollElements = {
  scroll: HTMLElement;
  scrollbar: HTMLElement;
  thumb: HTMLElement;
};

type VerticalScrollElements = {
  scroll: HTMLElement;
  scrollbar: HTMLElement;
  thumb: HTMLElement;
};

type HorizontalDragState = HorizontalScrollElements & {
  pointerId: number;
  grabOffset: number;
};

type VerticalDragState = VerticalScrollElements & {
  pointerId: number;
  grabOffset: number;
};

let horizontalDragState: HorizontalDragState | null = null;
let verticalDragState: VerticalDragState | null = null;
let resizeObserver: ResizeObserver | null = null;

function getHorizontalElements(scroll: HTMLElement): HorizontalScrollElements | null {
  const scrollbar = scroll.parentElement?.querySelector<HTMLElement>("[data-horizontal-scrollbar]");
  const thumb = scrollbar?.querySelector<HTMLElement>("[data-horizontal-scrollbar-thumb]");

  return scrollbar && thumb ? { scroll, scrollbar, thumb } : null;
}

function getSidebarVerticalElements(scroll: HTMLElement): VerticalScrollElements | null {
  const scrollbar = scroll.parentElement?.querySelector<HTMLElement>("[data-sidebar-scrollbar]");
  const thumb = scrollbar?.querySelector<HTMLElement>("[data-sidebar-scrollbar-thumb]");

  return scrollbar && thumb ? { scroll, scrollbar, thumb } : null;
}

function maxScrollLeft(scroll: HTMLElement): number {
  return Math.max(0, scroll.scrollWidth - scroll.clientWidth);
}

function maxScrollTop(scroll: HTMLElement): number {
  return Math.max(0, scroll.scrollHeight - scroll.clientHeight);
}

function updateHorizontalScrollbar(scroll: HTMLElement): void {
  const elements = getHorizontalElements(scroll);
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
    Math.max(MIN_THUMB_SIZE, trackWidth * (scroll.clientWidth / scroll.scrollWidth)),
  );
  const thumbOffset = (scroll.scrollLeft / maxScroll) * (trackWidth - thumbWidth);
  thumb.style.width = `${thumbWidth}px`;
  thumb.style.transform = `translateX(${thumbOffset}px)`;
}

function updateSidebarVerticalScrollbar(scroll: HTMLElement): void {
  const elements = getSidebarVerticalElements(scroll);
  if (!elements) return;

  const { scrollbar, thumb } = elements;
  const maxScroll = maxScrollTop(scroll);
  const trackHeight = scrollbar.clientHeight;

  scrollbar.hidden = maxScroll === 0;
  scrollbar.tabIndex = maxScroll === 0 ? -1 : 0;
  scrollbar.setAttribute("aria-valuemax", String(Math.round(maxScroll)));
  scrollbar.setAttribute("aria-valuenow", String(Math.round(scroll.scrollTop)));

  if (maxScroll === 0 || trackHeight === 0) return;

  const thumbHeight = Math.min(
    trackHeight,
    Math.max(MIN_THUMB_SIZE, trackHeight * (scroll.clientHeight / scroll.scrollHeight)),
  );
  const thumbOffset = (scroll.scrollTop / maxScroll) * (trackHeight - thumbHeight);
  thumb.style.height = `${thumbHeight}px`;
  thumb.style.transform = `translateY(${thumbOffset}px)`;
}

function updatePageVerticalScrollbar(): void {
  const scrollbar = document.querySelector<HTMLElement>('[data-vertical-scrollbar="page"]');
  const thumb = scrollbar?.querySelector<HTMLElement>("[data-vertical-scrollbar-thumb]");
  if (!scrollbar || !thumb) return;

  const scrollHeight = document.documentElement.scrollHeight;
  const maxScroll = Math.max(0, scrollHeight - window.innerHeight);
  const trackHeight = scrollbar.clientHeight;
  const scrollTop = window.scrollY;

  scrollbar.hidden = maxScroll === 0;
  scrollbar.tabIndex = maxScroll === 0 ? -1 : 0;
  scrollbar.setAttribute("aria-valuemax", String(Math.round(maxScroll)));
  scrollbar.setAttribute("aria-valuenow", String(Math.round(scrollTop)));

  if (maxScroll === 0 || trackHeight === 0) return;

  const thumbHeight = Math.min(
    trackHeight,
    Math.max(MIN_THUMB_SIZE, trackHeight * (window.innerHeight / scrollHeight)),
  );
  const thumbOffset = (scrollTop / maxScroll) * (trackHeight - thumbHeight);
  thumb.style.height = `${thumbHeight}px`;
  thumb.style.transform = `translateY(${thumbOffset}px)`;
}

function scrollHorizontalToThumbPosition(
  elements: HorizontalScrollElements,
  thumbOffset: number,
): void {
  const { scroll, scrollbar, thumb } = elements;
  const maxScroll = maxScrollLeft(scroll);
  const maxThumbOffset = Math.max(0, scrollbar.clientWidth - thumb.getBoundingClientRect().width);

  if (maxScroll === 0 || maxThumbOffset === 0) return;
  scroll.scrollLeft =
    Math.max(0, Math.min(maxThumbOffset, thumbOffset)) * (maxScroll / maxThumbOffset);
}

function scrollVerticalToThumbPosition(
  elements: VerticalScrollElements,
  thumbOffset: number,
): void {
  const { scroll, scrollbar, thumb } = elements;
  const maxScroll = maxScrollTop(scroll);
  const maxThumbOffset = Math.max(0, scrollbar.clientHeight - thumb.getBoundingClientRect().height);

  if (maxScroll === 0 || maxThumbOffset === 0) return;
  scroll.scrollTop =
    Math.max(0, Math.min(maxThumbOffset, thumbOffset)) * (maxScroll / maxThumbOffset);
}

function scrollPageToThumbPosition(thumbOffset: number): void {
  const scrollbar = document.querySelector<HTMLElement>('[data-vertical-scrollbar="page"]');
  const thumb = scrollbar?.querySelector<HTMLElement>("[data-vertical-scrollbar-thumb]");
  if (!scrollbar || !thumb) return;

  const maxScroll = Math.max(0, document.documentElement.scrollHeight - window.innerHeight);
  const maxThumbOffset = Math.max(0, scrollbar.clientHeight - thumb.getBoundingClientRect().height);
  if (maxScroll === 0 || maxThumbOffset === 0) return;

  window.scrollTo({
    top: Math.max(0, Math.min(maxThumbOffset, thumbOffset)) * (maxScroll / maxThumbOffset),
  });
}

function syncSidebarVerticalScrollbars(scope: ParentNode = document): void {
  scope.querySelectorAll<HTMLElement>("[data-sidebar-scroll]").forEach((scroll) => {
    updateSidebarVerticalScrollbar(scroll);
    resizeObserver?.observe(scroll);
  });
}

function syncAllScrollbars(): void {
  document
    .querySelectorAll<HTMLElement>("[data-horizontal-scroll]")
    .forEach(updateHorizontalScrollbar);
  syncSidebarVerticalScrollbars();
  updatePageVerticalScrollbar();
}

export function syncHorizontalScrollbars(scope: ParentNode = document): void {
  scope.querySelectorAll<HTMLElement>("[data-horizontal-scroll]").forEach((scroll) => {
    updateHorizontalScrollbar(scroll);
    resizeObserver?.observe(scroll);
  });
  syncSidebarVerticalScrollbars(scope);
}

export function syncVerticalScrollbar(): void {
  syncSidebarVerticalScrollbars();
  updatePageVerticalScrollbar();
}

export function installScrollbarBehavior(): void {
  if (resizeObserver) return;

  resizeObserver = new ResizeObserver((entries) => {
    entries.forEach((entry) => {
      const scroll = entry.target as HTMLElement;
      if (scroll.matches("[data-horizontal-scroll]")) updateHorizontalScrollbar(scroll);
      if (scroll.matches("[data-sidebar-scroll]")) updateSidebarVerticalScrollbar(scroll);
    });
  });

  document.addEventListener(
    "scroll",
    (event) => {
      const scroll = event.target;
      if (!(scroll instanceof HTMLElement)) return;
      if (scroll.matches("[data-horizontal-scroll]")) updateHorizontalScrollbar(scroll);
      if (scroll.matches("[data-sidebar-scroll]")) updateSidebarVerticalScrollbar(scroll);
    },
    true,
  );

  document.addEventListener(
    "wheel",
    (event) => {
      const target = event.target as HTMLElement;
      const sidebarScroll = target.closest<HTMLElement>("[data-sidebar-scroll]");
      const sidebarScrollbar = target.closest<HTMLElement>("[data-sidebar-scrollbar]");
      const scroll =
        sidebarScroll ??
        sidebarScrollbar?.parentElement?.querySelector<HTMLElement>("[data-sidebar-scroll]");

      if (scroll && Math.abs(event.deltaY) >= Math.abs(event.deltaX)) {
        const maxScroll = maxScrollTop(scroll);
        const direction = Math.sign(event.deltaY);
        const canScroll =
          maxScroll > 0 &&
          ((direction > 0 && scroll.scrollTop < maxScroll) ||
            (direction < 0 && scroll.scrollTop > 0));

        if (canScroll) {
          event.preventDefault();
          scroll.scrollTop += event.deltaY * WHEEL_SCROLL_MULTIPLIER;
        }
        return;
      }

      const horizontalScroll = target.closest<HTMLElement>("[data-horizontal-scroll]");
      if (!horizontalScroll || Math.abs(event.deltaX) > Math.abs(event.deltaY)) return;

      const maxScroll = maxScrollLeft(horizontalScroll);
      const direction = Math.sign(event.deltaY);
      const canScroll =
        maxScroll > 0 &&
        ((direction > 0 && horizontalScroll.scrollLeft < maxScroll) ||
          (direction < 0 && horizontalScroll.scrollLeft > 0));

      if (!canScroll) return;

      event.preventDefault();
      horizontalScroll.scrollLeft += event.deltaY * WHEEL_SCROLL_MULTIPLIER;
    },
    { passive: false },
  );

  document.addEventListener("pointerdown", (event) => {
    const target = event.target as HTMLElement;
    const sidebarScrollbar = target.closest<HTMLElement>("[data-sidebar-scrollbar]");
    if (sidebarScrollbar && !sidebarScrollbar.hidden) {
      const scroll =
        sidebarScrollbar.parentElement?.querySelector<HTMLElement>("[data-sidebar-scroll]");
      const thumb = sidebarScrollbar.querySelector<HTMLElement>("[data-sidebar-scrollbar-thumb]");
      if (!scroll || !thumb) return;

      const elements = { scroll, scrollbar: sidebarScrollbar, thumb };
      const scrollbarBounds = sidebarScrollbar.getBoundingClientRect();
      const thumbBounds = thumb.getBoundingClientRect();
      const pointerOffset = event.clientY - scrollbarBounds.top;
      const startedOnThumb = thumb.contains(target);
      const grabOffset = startedOnThumb ? event.clientY - thumbBounds.top : thumbBounds.height / 2;

      if (!startedOnThumb) scrollVerticalToThumbPosition(elements, pointerOffset - grabOffset);

      sidebarScrollbar.setPointerCapture(event.pointerId);
      verticalDragState = { ...elements, pointerId: event.pointerId, grabOffset };
      event.preventDefault();
      return;
    }

    const pageScrollbar = target.closest<HTMLElement>('[data-vertical-scrollbar="page"]');
    if (pageScrollbar && !pageScrollbar.hidden) {
      const thumb = pageScrollbar.querySelector<HTMLElement>("[data-vertical-scrollbar-thumb]");
      if (!thumb) return;

      const scrollbarBounds = pageScrollbar.getBoundingClientRect();
      const thumbBounds = thumb.getBoundingClientRect();
      const pointerOffset = event.clientY - scrollbarBounds.top;
      const startedOnThumb = thumb.contains(target);
      const grabOffset = startedOnThumb ? event.clientY - thumbBounds.top : thumbBounds.height / 2;

      if (!startedOnThumb) scrollPageToThumbPosition(pointerOffset - grabOffset);

      pageScrollbar.setPointerCapture(event.pointerId);
      verticalDragState = {
        scroll: document.documentElement,
        scrollbar: pageScrollbar,
        thumb,
        pointerId: event.pointerId,
        grabOffset,
      };
      event.preventDefault();
      return;
    }

    const scrollbar = target.closest<HTMLElement>("[data-horizontal-scrollbar]");
    if (!scrollbar || scrollbar.hidden) return;

    const scroll = scrollbar.parentElement?.querySelector<HTMLElement>("[data-horizontal-scroll]");
    const thumb = scrollbar.querySelector<HTMLElement>("[data-horizontal-scrollbar-thumb]");
    if (!scroll || !thumb) return;

    const elements = { scroll, scrollbar, thumb };
    const scrollbarBounds = scrollbar.getBoundingClientRect();
    const thumbBounds = thumb.getBoundingClientRect();
    const pointerOffset = event.clientX - scrollbarBounds.left;
    const startedOnThumb = thumb.contains(target);
    const grabOffset = startedOnThumb ? event.clientX - thumbBounds.left : thumbBounds.width / 2;

    if (!startedOnThumb) scrollHorizontalToThumbPosition(elements, pointerOffset - grabOffset);

    scrollbar.setPointerCapture(event.pointerId);
    horizontalDragState = { ...elements, pointerId: event.pointerId, grabOffset };
    event.preventDefault();
  });

  document.addEventListener("pointermove", (event) => {
    if (verticalDragState && event.pointerId === verticalDragState.pointerId) {
      const scrollbarBounds = verticalDragState.scrollbar.getBoundingClientRect();
      const thumbOffset = event.clientY - scrollbarBounds.top - verticalDragState.grabOffset;

      if (verticalDragState.scroll === document.documentElement) {
        scrollPageToThumbPosition(thumbOffset);
      } else {
        scrollVerticalToThumbPosition(verticalDragState, thumbOffset);
      }
      event.preventDefault();
      return;
    }

    if (!horizontalDragState || event.pointerId !== horizontalDragState.pointerId) return;
    const scrollbarBounds = horizontalDragState.scrollbar.getBoundingClientRect();
    scrollHorizontalToThumbPosition(
      horizontalDragState,
      event.clientX - scrollbarBounds.left - horizontalDragState.grabOffset,
    );
    event.preventDefault();
  });

  document.addEventListener("pointerup", endDrag);
  document.addEventListener("pointercancel", endDrag);

  document.addEventListener("keydown", (event) => {
    const target = event.target as HTMLElement;
    const sidebarScrollbar = target.closest<HTMLElement>("[data-sidebar-scrollbar]");
    if (sidebarScrollbar && !sidebarScrollbar.hidden) {
      const scroll =
        sidebarScrollbar.parentElement?.querySelector<HTMLElement>("[data-sidebar-scroll]");
      if (!scroll) return;

      const increment = Math.max(40, scroll.clientHeight * 0.1);
      const pageIncrement = Math.max(increment, scroll.clientHeight * 0.9);
      const keyToOffset: Partial<Record<string, number>> = {
        ArrowUp: -increment,
        ArrowDown: increment,
        PageUp: -pageIncrement,
        PageDown: pageIncrement,
        Home: -maxScrollTop(scroll),
        End: maxScrollTop(scroll),
      };
      const offset = keyToOffset[event.key];

      if (offset === undefined) return;
      event.preventDefault();
      scroll.scrollTop += offset;
      return;
    }

    const pageScrollbar = target.closest<HTMLElement>('[data-vertical-scrollbar="page"]');
    if (pageScrollbar && !pageScrollbar.hidden) {
      const increment = Math.max(40, window.innerHeight * 0.1);
      const pageIncrement = Math.max(increment, window.innerHeight * 0.9);
      const keyToOffset: Partial<Record<string, number>> = {
        ArrowUp: -increment,
        ArrowDown: increment,
        PageUp: -pageIncrement,
        PageDown: pageIncrement,
        Home: -Math.max(0, document.documentElement.scrollHeight - window.innerHeight),
        End: Math.max(0, document.documentElement.scrollHeight - window.innerHeight),
      };
      const offset = keyToOffset[event.key];

      if (offset === undefined) return;
      event.preventDefault();
      window.scrollBy({ top: offset });
      return;
    }

    const scrollbar = target.closest<HTMLElement>("[data-horizontal-scrollbar]");
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
  window.addEventListener("scroll", updatePageVerticalScrollbar, { passive: true });
  syncAllScrollbars();
}

function endDrag(event: PointerEvent): void {
  if (verticalDragState && event.pointerId === verticalDragState.pointerId) {
    verticalDragState.scrollbar.releasePointerCapture(event.pointerId);
    verticalDragState = null;
    return;
  }

  if (!horizontalDragState || event.pointerId !== horizontalDragState.pointerId) return;
  horizontalDragState.scrollbar.releasePointerCapture(event.pointerId);
  horizontalDragState = null;
}
