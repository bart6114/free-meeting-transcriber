import { renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useMainContentCenterOffset } from "./content-offset";

function stubRect(element: HTMLElement, left: number, width: number) {
  element.getBoundingClientRect = () =>
    ({
      left,
      width,
      right: left + width,
      top: 0,
      bottom: 0,
      height: 0,
      x: left,
      y: 0,
      toJSON: () => ({}),
    }) as DOMRect;
}

describe("useMainContentCenterOffset", () => {
  beforeEach(() => {
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    );
    window.innerWidth = 1000;
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  it("measures the main content panel, not the left sidebar panel", () => {
    document.body.innerHTML = `
      <div data-testid="main-app-shell">
        <div data-panel-id="classic-main-sidebar-left"></div>
        <div data-panel-id="classic-main-content">
          <div data-main-content-panel></div>
        </div>
      </div>
    `;

    const sidebar = document.querySelector<HTMLElement>(
      "[data-panel-id='classic-main-sidebar-left']",
    )!;
    const content = document.querySelector<HTMLElement>(
      "[data-main-content-panel]",
    )!;
    stubRect(sidebar, 0, 200);
    stubRect(content, 200, 800);

    const { result } = renderHook(() => useMainContentCenterOffset());

    expect(result.current).toBe(100);
  });

  it("returns 0 when the main shell is not mounted", () => {
    const { result } = renderHook(() => useMainContentCenterOffset());

    expect(result.current).toBe(0);
  });
});
