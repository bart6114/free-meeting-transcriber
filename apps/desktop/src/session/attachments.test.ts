import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  audioDelete: vi.fn(),
  audioMetadata: vi.fn(),
  execute: vi.fn(),
  executeTransaction: vi.fn().mockResolvedValue([0, 1, 1]),
  enqueueDatabaseWrite: vi.fn(
    async (_key: string, write: () => Promise<number[]>) => write(),
  ),
}));

vi.mock("@hypr/plugin-fs-sync", () => ({
  commands: {
    audioDelete: mocks.audioDelete,
    audioMetadata: mocks.audioMetadata,
  },
}));

vi.mock("~/db", () => ({
  executeTransaction: mocks.executeTransaction,
  liveQueryClient: { execute: mocks.execute },
}));

vi.mock("~/db/write-queue", () => ({
  enqueueDatabaseWrite: mocks.enqueueDatabaseWrite,
}));

import {
  catalogLocalNoteAttachment,
  catalogLocalSessionAudio,
  cleanupDeletedSessionAudio,
  deleteLocalSessionAudio,
  deleteSessionAudio,
  sha256Hex,
} from "./attachments";

describe("attachment catalog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.executeTransaction.mockResolvedValue([0, 1, 1]);
    mocks.audioDelete.mockResolvedValue({ status: "ok", data: true });
    mocks.audioMetadata.mockResolvedValue({
      status: "ok",
      data: {
        filename: "audio.mp3",
        contentType: "audio/mpeg",
        sizeBytes: 84,
        sha256: "d".repeat(64),
      },
    });
  });

  // catalogLocalNoteAttachment/catalogLocalSessionAudio are a graceful no-op
  // pending Task 9 (Session store scaffold) — session_attachments/
  // attachment_local_state were dropped in Task 4. See the comment above
  // their definitions in attachments.ts.
  it("resolves without touching the database", async () => {
    await expect(
      catalogLocalNoteAttachment({
        sessionId: "session-1",
        attachmentId: "diagram 1.png",
        filename: "diagram.png",
        contentType: "image/png",
        sizeBytes: 42,
        sha256: "a".repeat(64),
      }),
    ).resolves.toBeUndefined();

    expect(mocks.enqueueDatabaseWrite).not.toHaveBeenCalled();
    expect(mocks.executeTransaction).not.toHaveBeenCalled();
  });

  it("computes a stable lowercase SHA-256 checksum", async () => {
    const bytes = new TextEncoder().encode("hello").buffer;

    await expect(sha256Hex(bytes)).resolves.toBe(
      "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
    );
  });

  it("resolves without querying audio metadata or the database", async () => {
    await expect(
      catalogLocalSessionAudio("session-1"),
    ).resolves.toBeUndefined();

    expect(mocks.audioMetadata).not.toHaveBeenCalled();
    expect(mocks.executeTransaction).not.toHaveBeenCalled();
  });

  // markSessionAudioAvailability/tombstoneSessionAudioMetadata are a
  // graceful no-op pending Task 9 (Session store scaffold) —
  // attachment_local_state/session_attachments were dropped in Task 4. The
  // on-disk file deletion they accompany keeps working unchanged. See the
  // comments above their definitions in attachments.ts.
  it("deletes local audio bytes without touching the database", async () => {
    await expect(
      deleteLocalSessionAudio("session-1", () => true),
    ).resolves.toBe(true);
    expect(mocks.audioDelete).toHaveBeenCalledWith("session-1");
    expect(mocks.executeTransaction).not.toHaveBeenCalled();
  });

  it("deletes the recording without touching the database", async () => {
    await expect(deleteSessionAudio("session-1", () => true)).resolves.toBe(
      true,
    );
    expect(mocks.audioDelete).toHaveBeenCalledWith("session-1");
    expect(mocks.executeTransaction).not.toHaveBeenCalled();
  });

  it("completes logical deletion when local audio is already absent", async () => {
    mocks.audioDelete.mockResolvedValue({ status: "ok", data: false });

    await expect(deleteSessionAudio("session-1", () => true)).resolves.toBe(
      true,
    );
    expect(mocks.executeTransaction).not.toHaveBeenCalled();
  });

  it("resolves false when retention finds no local audio to delete", async () => {
    mocks.audioDelete.mockResolvedValue({ status: "ok", data: false });

    await expect(
      deleteLocalSessionAudio("session-1", () => true),
    ).resolves.toBe(false);
    expect(mocks.executeTransaction).not.toHaveBeenCalled();
  });

  // cleanupDeletedSessionAudio is likewise a graceful no-op — there is no
  // longer a way to detect a logically-deleted-but-locally-present
  // attachment to retry cleanup on. See the comment above its definition.
  it("resolves without deleting anything (backing table dropped)", async () => {
    await expect(
      cleanupDeletedSessionAudio("session-1", () => true),
    ).resolves.toBe(false);
    expect(mocks.execute).not.toHaveBeenCalled();
    expect(mocks.audioDelete).not.toHaveBeenCalled();
  });

  it("rechecks capture safety inside the serialized delete operation", async () => {
    await expect(deleteSessionAudio("session-1", () => false)).resolves.toBe(
      false,
    );
    expect(mocks.executeTransaction).not.toHaveBeenCalled();
    expect(mocks.audioDelete).not.toHaveBeenCalled();
  });
});
