export type AttachmentContent = {
  type?: string;
  attrs?: Record<string, unknown>;
  content?: AttachmentContent[];
  marks?: { type: string; attrs?: Record<string, unknown> }[];
  text?: string;
};

export function normalizePortableAttachmentUrls<T extends AttachmentContent>(
  document: T,
): T {
  return normalizeNode(document) as T;
}

function normalizeNode(node: AttachmentContent): AttachmentContent {
  const content = node.content?.map(normalizeNode);
  const attachmentId = node.attrs?.attachmentId;
  const shouldNormalize =
    (node.type === "image" || node.type === "fileAttachment") &&
    typeof attachmentId === "string" &&
    attachmentId.length > 0;

  if (!shouldNormalize) {
    return content ? { ...node, content } : node;
  }

  const attrs = { ...node.attrs };
  if (isLocalFileUrl(attrs.src)) {
    delete attrs.src;
  }
  delete attrs.path;
  return { ...node, attrs, ...(content ? { content } : {}) };
}

export function isLocalFileUrl(value: unknown): boolean {
  return (
    typeof value === "string" &&
    (value.startsWith("asset:") ||
      value.startsWith("file:") ||
      value.startsWith("http://asset.localhost/") ||
      value.startsWith("https://asset.localhost/"))
  );
}

// Parens are additionally encoded so the src never breaks markdown link syntax.
export function toPortableAttachmentSrc(attachmentId: string): string {
  const encoded = encodeURIComponent(attachmentId)
    .replace(/\(/g, "%28")
    .replace(/\)/g, "%29");
  return `attachments/${encoded}`;
}

export function parsePortableAttachmentSrc(src: unknown): string | null {
  if (typeof src !== "string") {
    return null;
  }

  const match = src.match(/^(?:\.\/)?attachments\/([^/\\]+)$/);
  if (!match) {
    return null;
  }

  try {
    return decodeURIComponent(match[1]);
  } catch {
    return match[1];
  }
}
