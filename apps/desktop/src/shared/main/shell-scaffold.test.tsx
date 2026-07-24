import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { MainShellScaffold } from "./shell-scaffold";

describe("MainShellScaffold", () => {
  afterEach(() => {
    cleanup();
  });

  it("keeps the top border for regular top chrome", () => {
    render(
      <MainShellScaffold mainSurfaceChrome="top">
        <div data-main-surface data-testid="main-surface" />
      </MainShellScaffold>,
    );

    const shell = screen.getByTestId("main-app-shell");

    expect(shell.className).toContain("[&_[data-main-surface]]:border-t");
    expect(shell.className).not.toContain(
      "[&_[data-main-surface]]:!border-t-0",
    );
  });

  it("removes the top border for borderless top chrome", () => {
    render(
      <MainShellScaffold mainSurfaceChrome="top-borderless">
        <div data-main-surface data-testid="main-surface" />
      </MainShellScaffold>,
    );

    const shell = screen.getByTestId("main-app-shell");

    expect(shell.className).toContain("[&_[data-main-surface]]:!border-t-0");
    expect(shell.className).not.toContain("pl-1");
  });
});
