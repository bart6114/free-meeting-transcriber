import { useEffect, useState } from "react";

import { TZDate } from "@hypr/utils";

import { useConfigValue } from "~/shared/config";

export function useTimezone() {
  return useConfigValue("timezone") || undefined;
}

export function toTz(date: Date | string, tz?: string): Date {
  const d = typeof date === "string" ? new Date(date) : date;
  return tz ? new TZDate(d, tz) : d;
}

export function useNow() {
  const tz = useTimezone();
  const [now, setNow] = useState(() => toTz(new Date(), tz));

  useEffect(() => {
    const interval = setInterval(() => {
      setNow(toTz(new Date(), tz));
    }, 60000);
    return () => clearInterval(interval);
  }, [tz]);

  return now;
}
