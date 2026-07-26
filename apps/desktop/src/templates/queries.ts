import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback } from "react";

import type { TemplateSection } from "@hypr/store";

import {
  assertCanonicalTemplateSections,
  assertCanonicalTemplateTargets,
  parseStoredTemplateSections,
  parseStoredTemplateTargets,
} from "./codec";
import {
  DEFAULT_TEMPLATE_ICON,
  normalizeTemplateIcon,
  type TemplateIcon,
} from "./template-icon";

import {
  commands,
  type Result,
  type TemplateInput,
  type TemplateItem,
} from "~/types/tauri.gen";

export type UserTemplate = {
  id: string;
  title: string;
  description: string;
  pinned: boolean;
  pinOrder?: number;
  category?: string;
  icon: TemplateIcon;
  targets?: string[];
  sections: TemplateSection[];
};

export type UserTemplateDraft = Pick<
  UserTemplate,
  "title" | "description" | "category" | "targets" | "sections"
> & { icon?: TemplateIcon };

const templatesQueryKey = ["templates"];

function unwrap<T>(result: Result<T, string>): T {
  if (result.status === "error") {
    throw new Error(result.error);
  }
  return result.data;
}

function toUserTemplate(item: TemplateItem): UserTemplate {
  return {
    id: item.id,
    title: item.title ?? "",
    description: item.description ?? "",
    pinned: item.pinned ?? false,
    pinOrder: item.pin_order ?? undefined,
    category: item.category ?? undefined,
    icon: normalizeTemplateIcon(item.icon),
    targets: parseStoredTemplateTargets(item.targets, item.id),
    sections: parseStoredTemplateSections(item.sections, item.id),
  };
}

async function listTemplates(): Promise<UserTemplate[]> {
  const items = unwrap(await commands.templateList());
  return items.map(toUserTemplate);
}

export function useUserTemplates(): UserTemplate[] {
  const { data = [] } = useQuery({
    queryKey: templatesQueryKey,
    queryFn: listTemplates,
  });

  return data;
}

export function useUserTemplate(id: string | null | undefined) {
  return useQuery({
    queryKey: [...templatesQueryKey, id ?? ""],
    queryFn: () => getTemplateById(id ?? ""),
    enabled: Boolean(id),
  });
}

export async function getTemplateById(
  id: string,
): Promise<UserTemplate | null> {
  if (!id) {
    return null;
  }

  const item = unwrap(await commands.templateGet(id));
  return item ? toUserTemplate(item) : null;
}

async function upsertTemplate(template: UserTemplate): Promise<string> {
  const targets = assertCanonicalTemplateTargets(
    template.targets,
    `save template ${template.id} targets`,
  );
  const sections = assertCanonicalTemplateSections(
    template.sections,
    `save template ${template.id} sections`,
  );

  const input: TemplateInput = {
    id: template.id,
    title: template.title,
    description: template.description,
    pinned: template.pinned,
    pin_order: template.pinOrder ?? null,
    category: template.category ?? null,
    icon: normalizeTemplateIcon(template.icon) as TemplateInput["icon"],
    targets: (targets ?? null) as TemplateInput["targets"],
    sections: sections as TemplateInput["sections"],
  };
  unwrap(await commands.templateUpsert(input));
  return template.id;
}

export function useCreateTemplate() {
  const queryClient = useQueryClient();
  const { mutateAsync } = useMutation({
    mutationFn: (template: UserTemplateDraft) =>
      upsertTemplate({
        id: crypto.randomUUID(),
        title: template.title,
        description: template.description,
        pinned: false,
        category: template.category,
        icon: template.icon ?? DEFAULT_TEMPLATE_ICON,
        targets: template.targets,
        sections: template.sections,
      }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: templatesQueryKey });
    },
    onError: (error) => {
      console.error("[useCreateTemplate]", error);
    },
  });

  return mutateAsync;
}

export function useSaveTemplate() {
  const queryClient = useQueryClient();
  const { mutateAsync } = useMutation({
    mutationFn: (template: UserTemplate) => upsertTemplate(template),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: templatesQueryKey });
    },
    onError: (error) => {
      console.error("[useSaveTemplate]", error);
    },
  });

  return mutateAsync;
}

export function useDeleteTemplate() {
  const queryClient = useQueryClient();
  const { mutateAsync } = useMutation({
    mutationFn: async (id: string) => {
      unwrap(await commands.templateDelete(id));
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: templatesQueryKey });
    },
    onError: (error) => {
      console.error("[useDeleteTemplate]", error);
    },
  });

  return mutateAsync;
}

export function useToggleTemplateFavorite() {
  const saveTemplate = useSaveTemplate();

  return useCallback(
    async (templateId: string) => {
      const template = await getTemplateById(templateId);
      if (!template) {
        return;
      }

      if (template.pinned) {
        await saveTemplate({
          ...template,
          pinned: false,
          pinOrder: 0,
        });
        return;
      }

      const templates = await listTemplates();
      const maxOrder = templates
        .filter((other) => other.id !== templateId)
        .reduce((max, other) => Math.max(max, other.pinOrder ?? 0), 0);

      await saveTemplate({
        ...template,
        pinned: true,
        pinOrder: maxOrder + 1,
      });
    },
    [saveTemplate],
  );
}

export function getTemplateCopyTitle(title: string) {
  const value = title.trim();

  if (!value) return "Untitled (Copy)";
  if (value.endsWith("(Copy)")) return value;

  return `${value} (Copy)`;
}
