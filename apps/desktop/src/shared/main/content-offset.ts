import { useEffect, useState } from "react";

const MAIN_SHELL_SELECTOR = "[data-testid='main-app-shell']";
const MAIN_CONTENT_SELECTOR = "[data-main-content-panel]";

export function useMainContentCenterOffset() {
  const [contentOffset, setContentOffset] = useState(0);

  useEffect(() => {
    const computeOffset = () => {
      const shell = document.querySelector(MAIN_SHELL_SELECTOR);
      if (!shell) {
        setContentOffset(0);
        return;
      }

      const bodyPanel = shell.querySelector(MAIN_CONTENT_SELECTOR);
      if (!bodyPanel) {
        setContentOffset(0);
        return;
      }

      const bodyRect = bodyPanel.getBoundingClientRect();
      const bodyCenter = bodyRect.left + bodyRect.width / 2;
      const windowCenter = window.innerWidth / 2;
      setContentOffset(bodyCenter - windowCenter);
    };

    computeOffset();
    window.addEventListener("resize", computeOffset);

    const resizeObserver = new ResizeObserver(computeOffset);
    const bodyPanel = document.querySelector(MAIN_CONTENT_SELECTOR);
    if (bodyPanel) {
      resizeObserver.observe(bodyPanel);
    }

    return () => {
      window.removeEventListener("resize", computeOffset);
      resizeObserver.disconnect();
    };
  }, []);

  return contentOffset;
}
