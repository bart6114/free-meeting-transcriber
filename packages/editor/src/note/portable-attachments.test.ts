import { describe, expect, it } from "vitest";

import {
  normalizePortableAttachmentUrls,
  parsePortableAttachmentSrc,
  toPortableAttachmentSrc,
} from "./portable-attachments";

describe("normalizePortableAttachmentUrls", () => {
  it("removes device-local image and file locations while keeping identities", () => {
    expect(
      normalizePortableAttachmentUrls({
        type: "doc",
        content: [
          {
            type: "image",
            attrs: {
              attachmentId: "diagram.png",
              src: "asset://localhost/device-a/diagram.png",
              alt: "Diagram",
            },
          },
          {
            type: "fileAttachment",
            attrs: {
              attachmentId: "notes.pdf",
              src: "file:///device-a/notes.pdf",
              path: "/device-a/notes.pdf",
              name: "notes.pdf",
            },
          },
          {
            type: "image",
            attrs: {
              attachmentId: "windows-diagram.png",
              src: "http://asset.localhost/C%3A/Users/Example/windows-diagram.png",
              path: "C:\\Users\\Example\\windows-diagram.png",
              alt: "Windows diagram",
            },
          },
        ],
      }),
    ).toEqual({
      type: "doc",
      content: [
        {
          type: "image",
          attrs: { attachmentId: "diagram.png", alt: "Diagram" },
        },
        {
          type: "fileAttachment",
          attrs: { attachmentId: "notes.pdf", name: "notes.pdf" },
        },
        {
          type: "image",
          attrs: {
            attachmentId: "windows-diagram.png",
            alt: "Windows diagram",
          },
        },
      ],
    });
  });

  it("preserves remote URLs and nodes without a catalog identity", () => {
    const document = {
      type: "doc",
      content: [
        {
          type: "image",
          attrs: {
            attachmentId: "remote-image",
            src: "https://example.com/image.png",
          },
        },
        {
          type: "image",
          attrs: { src: "asset://localhost/uncatalogued.png" },
        },
      ],
    };

    expect(normalizePortableAttachmentUrls(document)).toEqual(document);
  });
});

describe("portable attachment srcs", () => {
  it("round-trips attachment ids with spaces and parentheses", () => {
    const id = "screenshot 1 (copy).png";
    const src = toPortableAttachmentSrc(id);

    expect(src).toBe("attachments/screenshot%201%20%28copy%29.png");
    expect(parsePortableAttachmentSrc(src)).toBe(id);
  });

  it("accepts a ./ prefix", () => {
    expect(parsePortableAttachmentSrc("./attachments/diagram.png")).toBe(
      "diagram.png",
    );
  });

  it("rejects srcs that are not vault-relative attachment paths", () => {
    expect(parsePortableAttachmentSrc(null)).toBeNull();
    expect(parsePortableAttachmentSrc("")).toBeNull();
    expect(parsePortableAttachmentSrc("attachments/")).toBeNull();
    expect(
      parsePortableAttachmentSrc("https://example.com/attachments/a.png"),
    ).toBeNull();
    expect(
      parsePortableAttachmentSrc("asset://localhost/attachments/a.png"),
    ).toBeNull();
    expect(parsePortableAttachmentSrc("/vault/attachments/a.png")).toBeNull();
    expect(parsePortableAttachmentSrc("attachments/nested/a.png")).toBeNull();
  });
});
