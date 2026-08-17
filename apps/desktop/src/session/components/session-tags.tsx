import { useLingui } from "@lingui/react/macro";
import { XIcon } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import {
  Popover,
  PopoverAnchor,
  PopoverContent,
} from "@hypr/ui/components/ui/popover";
import { sonnerToast } from "@hypr/ui/components/ui/toast";
import { cn } from "@hypr/utils";

import { useSession, useUpdateSession } from "~/session/queries";
import { normalizeTagNames } from "~/tags/normalize";
import { ensureTag, useInUseTags, useTags } from "~/tags/queries";

export function SessionTags({
  sessionId,
  className,
}: {
  sessionId: string;
  className?: string;
}) {
  const { t } = useLingui();
  const savedTags = useSession(sessionId)?.tags ?? EMPTY_TAGS;
  const updateSession = useUpdateSession(sessionId);
  // Shown between a commit and the index re-emitting, so chips never flash the
  // pre-edit set. Cleared once the live value catches up (or the write fails).
  const [pendingTags, setPendingTags] = useState<string[] | null>(null);

  useEffect(() => {
    if (pendingTags !== null && tagsEqual(savedTags, pendingTags)) {
      setPendingTags(null);
    }
  }, [pendingTags, savedTags]);

  const tags = pendingTags ?? savedTags;

  const commit = (nextTags: string[]) => {
    const sorted = [...new Set(nextTags)].sort();
    if (tagsEqual(sorted, tags)) {
      return;
    }
    setPendingTags(sorted);
    updateSession({ tags: sorted }).catch((error) => {
      console.error("[session-tags] failed to update tags", error);
      sonnerToast.error(t`Could not update tags.`);
      setPendingTags(null);
    });
  };

  const addTag = (name: string) => {
    const normalized = normalizeTagNames([name])[0];
    if (!normalized || tags.includes(normalized)) {
      return;
    }
    commit([...tags, normalized]);
    // Best-effort registry sync: the chip write is canonical; a `tags.json`
    // failure only degrades future typeahead.
    void ensureTag(normalized).catch((error) => {
      console.error("[session-tags] failed to register tag", normalized, error);
    });
  };

  return (
    <div className={cn(["flex flex-wrap items-center gap-1.5", className])}>
      {tags.map((tag) => (
        <span
          key={tag}
          className={cn([
            "group/tag flex items-center gap-1",
            "bg-accent/60 text-muted-foreground rounded-full py-0.5 pr-1.5 pl-2 text-xs font-medium",
          ])}
        >
          #{tag}
          <button
            type="button"
            aria-label={t`Remove tag ${tag}`}
            onClick={() => commit(tags.filter((other) => other !== tag))}
            className={cn([
              "rounded-full opacity-0 transition-opacity",
              "group-hover/tag:opacity-60 hover:!opacity-100 focus-visible:opacity-100",
            ])}
          >
            <XIcon size={12} />
          </button>
        </span>
      ))}
      <TagAddControl attachedTags={tags} onAdd={addTag} />
    </div>
  );
}

function TagAddControl({
  attachedTags,
  onAdd,
}: {
  attachedTags: string[];
  onAdd: (name: string) => void;
}) {
  const { t } = useLingui();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [highlightIndex, setHighlightIndex] = useState(-1);
  const inputRef = useRef<HTMLInputElement>(null);
  const committedRef = useRef(false);
  const registryTags = useTags();
  const inUseTags = useInUseTags();

  const suggestions = useMemo(() => {
    if (!editing) {
      return [];
    }
    const query = draft.replace(/^#/, "").trim().toLowerCase();
    const known = [
      ...new Set([...registryTags.map((tag) => tag.name), ...inUseTags]),
    ].sort();
    return known
      .filter(
        (name) =>
          !attachedTags.includes(name) && (!query || name.includes(query)),
      )
      .slice(0, 8);
  }, [attachedTags, draft, editing, inUseTags, registryTags]);

  useEffect(() => {
    setHighlightIndex(-1);
  }, [draft]);

  const startEditing = () => {
    committedRef.current = false;
    setDraft("");
    setHighlightIndex(-1);
    setEditing(true);
  };

  const commitDraft = (name: string) => {
    if (committedRef.current) {
      return;
    }
    committedRef.current = true;
    setEditing(false);
    if (name.trim()) {
      onAdd(name);
    }
  };

  if (!editing) {
    return (
      <button
        type="button"
        onClick={startEditing}
        className={cn([
          "text-muted-foreground/70 hover:text-foreground rounded-full py-0.5 text-xs transition-colors",
          attachedTags.length === 0 ? "pr-2" : "px-1",
        ])}
      >
        {attachedTags.length === 0 ? t`+ Add tag` : "+"}
      </button>
    );
  }

  return (
    <Popover open={suggestions.length > 0}>
      <PopoverAnchor asChild>
        <input
          ref={inputRef}
          autoFocus
          type="text"
          value={draft}
          placeholder={t`Tag name`}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={() => commitDraft(draft)}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") {
              e.preventDefault();
              setHighlightIndex((index) =>
                suggestions.length === 0
                  ? -1
                  : (index + 1) % suggestions.length,
              );
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setHighlightIndex((index) =>
                suggestions.length === 0
                  ? -1
                  : (index <= 0 ? suggestions.length : index) - 1,
              );
            } else if (e.key === "Enter") {
              e.preventDefault();
              commitDraft(suggestions[highlightIndex] ?? draft);
            } else if (e.key === "Escape") {
              e.preventDefault();
              committedRef.current = true;
              setEditing(false);
            }
          }}
          className={cn([
            "text-muted-foreground w-24 min-w-0 rounded-full py-0.5 text-xs",
            "border-none bg-transparent outline-hidden",
          ])}
        />
      </PopoverAnchor>
      <PopoverContent
        align="start"
        className="w-48 p-1"
        onOpenAutoFocus={(e) => e.preventDefault()}
      >
        <ul role="listbox">
          {suggestions.map((name, index) => (
            <li key={name}>
              <button
                type="button"
                role="option"
                aria-selected={index === highlightIndex}
                onPointerDown={(e) => e.preventDefault()}
                onClick={() => commitDraft(name)}
                onMouseEnter={() => setHighlightIndex(index)}
                className={cn([
                  "w-full rounded-sm px-2 py-1 text-left text-sm",
                  index === highlightIndex && "bg-accent",
                ])}
              >
                #{name}
              </button>
            </li>
          ))}
        </ul>
      </PopoverContent>
    </Popover>
  );
}

function tagsEqual(a: string[], b: string[]): boolean {
  return a.length === b.length && a.every((tag, index) => tag === b[index]);
}

const EMPTY_TAGS: string[] = [];
